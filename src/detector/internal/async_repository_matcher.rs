// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded cooperative matcher for immutable MIME repository rules.

use std::future::poll_fn;
use std::task::Poll;

use qubit_operation::OperationScope;

use crate::MimeCandidate;
use crate::MimeDetectionPolicy;
use crate::MimeError;
use crate::MimeRepository;
use crate::MimeResult;
use crate::MimeType;
use crate::repository::magic_index::MagicMatchState;

/// Maximum number of repository rules evaluated between cooperative yields.
pub(super) const RULES_PER_BATCH: usize = 32;

/// Returns the unique repository filename result when the selected policy
/// permits skipping content inspection.
///
/// # Parameters
/// - `repository`: Immutable MIME filename rules.
/// - `filename`: Optional basename supplied by the input.
/// - `policy`: Candidate selection policy.
///
/// # Returns
/// The sole matching MIME type for `PreferFilename`, or `None` otherwise.
pub(in crate::detector) fn preferred_filename(
    repository: &MimeRepository,
    filename: Option<&str>,
    policy: MimeDetectionPolicy,
) -> Option<MimeType> {
    if policy != MimeDetectionPolicy::PreferFilename {
        return None;
    }
    let filename = filename?;
    let matches = repository.detect_by_filename(filename);
    (matches.len() == 1).then(|| matches[0].clone())
}

/// Returns the unique filename result for an already extracted native
/// basename when policy permits skipping content inspection.
pub(in crate::detector) fn preferred_basename(
    repository: &MimeRepository,
    basename: Option<&str>,
    policy: MimeDetectionPolicy,
) -> Option<MimeType> {
    if policy != MimeDetectionPolicy::PreferFilename {
        return None;
    }
    let matches = repository.detect_by_filename_basename(basename?);
    (matches.len() == 1).then(|| matches[0].clone())
}

/// Match content rules in bounded batches while preserving the synchronous
/// repository's highest-priority and insertion-order behavior.
///
/// # Parameters
/// - `scope`: Operation scope used to detect cancellation between batches.
/// - `repository`: Immutable MIME rules to evaluate.
/// - `bytes`: Content prefix tested against repository magic rules.
///
/// # Returns
/// Matching MIME candidates at the highest matching priority, in repository
/// order.
///
/// # Errors
/// Returns a detector backend error if the operation is stopped.
pub(in crate::detector) async fn match_content(
    scope: &OperationScope,
    repository: &MimeRepository,
    bytes: &[u8],
) -> MimeResult<Vec<MimeCandidate>> {
    let mut matches = MagicMatchState::default();
    let mut rules_in_batch = 0usize;
    for entry in repository.magic_rules() {
        if rules_in_batch == 0
            && let Some(reason) = scope.context().stop_reason()
        {
            return Err(MimeError::DetectorBackend {
                backend: "repository".to_owned(),
                reason: format!("operation stopped: {reason:?}"),
            });
        }
        rules_in_batch += 1;
        if matches.skips(entry.priority) {
            if rules_in_batch == RULES_PER_BATCH {
                rules_in_batch = 0;
                yield_to_executor().await;
            }
            continue;
        }
        let Some(magic) = repository.magic_rule(entry) else {
            continue;
        };
        let mut matched = false;
        for matcher in magic.matchers() {
            match matcher.matches_cooperatively(bytes, scope).await {
                Ok(true) => {
                    matched = true;
                    break;
                }
                Ok(false) => {}
                Err(reason) => {
                    return Err(MimeError::DetectorBackend {
                        backend: "repository".to_owned(),
                        reason: format!("operation stopped: {reason:?}"),
                    });
                }
            }
        }
        matches.record(entry.priority, entry.mime_index, matched);
        if rules_in_batch == RULES_PER_BATCH {
            rules_in_batch = 0;
            yield_to_executor().await;
        }
    }
    if let Some(reason) = scope.context().stop_reason() {
        return Err(MimeError::DetectorBackend {
            backend: "repository".to_owned(),
            reason: format!("operation stopped: {reason:?}"),
        });
    }
    Ok(matches
        .into_results()
        .into_iter()
        .enumerate()
        .filter_map(|(rank, mime_index)| {
            repository.all().get(mime_index).map(|mime| MimeCandidate {
                mime_type: mime.name().to_owned(),
                rank: rank as u32,
            })
        })
        .collect())
}

/// Apply the repository's synchronous filename/content policy to a bounded
/// content candidate list.
///
/// # Parameters
/// - `repository`: Repository used for filename lookup and MIME relationships.
/// - `content`: Candidates produced by content matching.
/// - `filename`: Optional filename used by the synchronous policy.
/// - `policy`: Rule for combining filename and content candidates.
///
/// # Returns
/// The selected candidate, or an empty list when no candidate is selected.
#[must_use]
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

/// Gives the current executor an opportunity to poll another task.
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

#[cfg(test)]
mod cooperative_matcher_tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    use super::match_content;
    use crate::MimeRepository;

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    #[test]
    fn test_cooperative_matching_yields_and_observes_stop_inside_wide_rule() {
        let xml = format!(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\"><mime-type type=\"application/x-wide\"><magic priority=\"50\">{}</magic></mime-type></mime-info>",
            (0..4)
                .map(|index| format!("<match type=\"string\" value=\"NEVER{index}\" offset=\"0:65535\"/>"))
                .collect::<String>(),
        );
        let repository = Arc::new(MimeRepository::from_xml(&xml).unwrap());
        let input = Arc::new(vec![b'X'; 65_540]);
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let probe_polled = Arc::new(AtomicBool::new(false));
        let mut operation = block_on(group.try_submit_async(Default::default(), {
            let probe_polled = Arc::clone(&probe_polled);
            move |scope| async move {
                let mut matching = Box::pin(match_content(&scope, &repository, &input));
                let mut stop_requested = false;
                let result = std::future::poll_fn(|cx| {
                    let result = matching.as_mut().poll(cx);
                    if result.is_pending() && !stop_requested {
                        stop_requested = true;
                        probe_polled.store(true, Ordering::SeqCst);
                        let _ = scope.context().request_stop(StopReason::UserCancelled);
                    }
                    result
                })
                .await;
                Ok::<_, std::convert::Infallible>(result.is_err())
            }
        }))
        .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        assert!(
            probe_polled.load(Ordering::SeqCst),
            "the matcher should yield within a single wide rule"
        );
        assert!(matches!(outcome, OperationOutcome::Stopped(report) if report.reason == StopReason::UserCancelled));
    }

    #[test]
    fn test_async_magic_ranking_matches_sync_order_and_deduplication() {
        let repository = Arc::new(
            MimeRepository::from_xml(
                r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
                  <mime-type type="application/x-low"><magic priority="50"><match type="string" value="HIT" offset="0"/></magic></mime-type>
                  <mime-type type="application/x-high-a"><magic priority="80"><match type="string" value="HIT" offset="0"/></magic></mime-type>
                  <mime-type type="application/x-high-b"><magic priority="80"><match type="string" value="HIT" offset="0"/><match type="string" value="HIT" offset="0"/></magic></mime-type>
                </mime-info>"#,
            )
            .unwrap(),
        );
        let bytes = Arc::new(b"HIT".to_vec());
        let synchronous = repository
            .detect_by_content(&bytes)
            .into_iter()
            .map(|mime| mime.name().to_owned())
            .collect::<Vec<_>>();
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let mut operation = block_on(group.try_submit_async(Default::default(), {
            let repository = Arc::clone(&repository);
            move |scope| async move { match_content(&scope, &repository, &bytes).await }
        }))
        .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        let asynchronous = match outcome {
            OperationOutcome::Succeeded(candidates) => candidates
                .into_iter()
                .map(|candidate| candidate.mime_type)
                .collect::<Vec<_>>(),
            other => panic!("async matcher should complete: {other:?}"),
        };
        assert_eq!(asynchronous, synchronous);
        assert_eq!(asynchronous, ["application/x-high-a", "application/x-high-b"]);
    }

    #[test]
    fn test_async_matching_preserves_maximum_nested_matcher_depth() {
        let mut xml = String::from(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\"><mime-type type=\"application/x-deep\"><magic priority=\"50\">",
        );
        for _ in 0..128 {
            xml.push_str("<match type=\"string\" value=\"X\" offset=\"0\">");
        }
        for _ in 0..128 {
            xml.push_str("</match>");
        }
        xml.push_str("</magic></mime-type></mime-info>");
        let repository = Arc::new(MimeRepository::from_xml(&xml).unwrap());
        assert_eq!(
            vec!["application/x-deep"],
            repository
                .detect_by_content(b"X")
                .into_iter()
                .map(|mime| mime.name())
                .collect::<Vec<_>>()
        );
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            match_content(&scope, &repository, b"X").await
        }))
        .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        let candidates = match outcome {
            OperationOutcome::Succeeded(candidates) => candidates,
            other => panic!("maximum-depth async matcher should complete: {other:?}"),
        };
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].mime_type, "application/x-deep");
        assert_eq!(candidates[0].rank, 0);
    }
}
