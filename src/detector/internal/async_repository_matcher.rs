// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Bounded cooperative matcher for immutable MIME repository rules.

use std::collections::HashSet;
use std::future::poll_fn;
use std::task::Poll;

use qubit_operation::OperationScope;

use crate::MimeCandidate;
use crate::MimeDetectionPolicy;
use crate::MimeError;
use crate::MimeRepository;
use crate::MimeResult;

/// Maximum number of repository rules evaluated between cooperative yields.
pub(super) const RULES_PER_BATCH: usize = 32;

/// Match content rules in bounded batches while preserving the synchronous
/// repository's highest-priority and insertion-order behavior.
pub(in crate::detector) async fn match_content(
    scope: &OperationScope,
    repository: &MimeRepository,
    bytes: &[u8],
) -> MimeResult<Vec<MimeCandidate>> {
    let mut best_priority = None;
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    let mut rules_in_batch = 0usize;
    for (mime_index, mime) in repository.all().iter().enumerate() {
        for magic in mime.magics() {
            if rules_in_batch == 0 {
                if let Some(reason) = scope.context().stop_reason() {
                    return Err(MimeError::DetectorBackend {
                        backend: "repository".to_owned(),
                        reason: format!("operation stopped: {reason:?}"),
                    });
                }
            }
            rules_in_batch += 1;
            if best_priority.is_some_and(|best| magic.priority() < best) || !magic.matches(bytes) {
                if rules_in_batch == RULES_PER_BATCH {
                    rules_in_batch = 0;
                    yield_to_executor().await;
                }
                continue;
            }
            match best_priority {
                Some(best) if magic.priority() > best => {
                    best_priority = Some(magic.priority());
                    seen.clear();
                    candidates.clear();
                }
                None => best_priority = Some(magic.priority()),
                _ => {}
            }
            if seen.insert(mime_index) {
                candidates.push(MimeCandidate {
                    mime_type: mime.name().to_owned(),
                    rank: candidates.len() as u32,
                });
            }
            if rules_in_batch == RULES_PER_BATCH {
                rules_in_batch = 0;
                yield_to_executor().await;
            }
        }
    }
    if let Some(reason) = scope.context().stop_reason() {
        return Err(MimeError::DetectorBackend {
            backend: "repository".to_owned(),
            reason: format!("operation stopped: {reason:?}"),
        });
    }
    Ok(candidates)
}

/// Apply the repository's synchronous filename/content policy to a bounded
/// content candidate list.
pub(in crate::detector) fn select_candidates(
    repository: &MimeRepository,
    content: Vec<MimeCandidate>,
    filename: Option<&str>,
    policy: MimeDetectionPolicy,
) -> Vec<MimeCandidate> {
    let Some(filename) = filename else {
        return content;
    };
    let from_filename = repository.detect_by_filename(filename);
    let from_content = content
        .iter()
        .filter_map(|candidate| repository.get(&candidate.mime_type))
        .collect::<Vec<_>>();
    crate::candidate_selection::select_candidate(&from_filename, &from_content, policy, |left, right| {
        if policy == MimeDetectionPolicy::Freedesktop {
            repository.is_a(left.name(), right.name())
        } else {
            left.name() == right.name()
        }
    })
    .map(|mime| {
        vec![MimeCandidate {
            mime_type: mime.name().to_owned(),
            rank: 0,
        }]
    })
    .unwrap_or_default()
}

async fn yield_to_executor() {
    // Yield to the current executor after each finite chunk. No timer or
    // blocking wait is involved.
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}
