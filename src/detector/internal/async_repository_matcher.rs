// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded cooperative matcher for immutable MIME repository rules.

use qubit_operation::OperationScope;

use crate::MimeCandidate;
use crate::MimeDetectionPolicy;
use crate::MimeError;
use crate::MimeRepository;
use crate::MimeResult;
use crate::MimeType;
use crate::repository::internal::magic_match_state::MagicMatchState;
use crate::repository::mime_magic_matcher::MatcherWorkBudget;
use crate::repository::mime_magic_matcher::yield_after_work;

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
pub(in crate::detector) fn preferred_filename<'a>(
    repository: &'a MimeRepository,
    filename: Option<&str>,
    policy: MimeDetectionPolicy,
) -> Option<&'a MimeType> {
    if policy != MimeDetectionPolicy::PreferFilename {
        return None;
    }
    let filename = filename?;
    let matches = repository.detect_by_filename(filename);
    (matches.len() == 1).then(|| matches[0])
}

/// Returns the unique filename result for an already extracted native
/// basename when policy permits skipping content inspection.
///
/// # Parameters
/// - `repository`: Immutable MIME filename rules.
/// - `basename`: Optional native basename supplied by the input.
/// - `policy`: Candidate selection policy.
///
/// # Returns
/// The sole matching MIME type for `PreferFilename`, or `None` otherwise.
pub(in crate::detector) fn preferred_basename<'a>(
    repository: &'a MimeRepository,
    basename: Option<&str>,
    policy: MimeDetectionPolicy,
) -> Option<&'a MimeType> {
    if policy != MimeDetectionPolicy::PreferFilename {
        return None;
    }
    let matches = repository.detect_by_filename_basename(basename?);
    (matches.len() == 1).then(|| matches[0])
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
    let mut work = MatcherWorkBudget::default();
    match_content_with_budget(scope, repository, bytes, &mut work).await
}

/// Evaluates repository rules while sharing one cooperative work budget.
///
/// # Parameters
/// - `scope`: Operation scope used to detect cancellation between batches.
/// - `repository`: Immutable MIME rules to evaluate.
/// - `bytes`: Content prefix tested against repository magic rules.
/// - `work`: Work counter shared with nested matcher evaluation.
///
/// # Returns
/// Matching MIME candidates at the highest matching priority, in repository
/// order.
///
/// # Errors
/// Returns a detector backend error if the operation is stopped.
async fn match_content_with_budget(
    scope: &OperationScope,
    repository: &MimeRepository,
    bytes: &[u8],
    work: &mut MatcherWorkBudget,
) -> MimeResult<Vec<MimeCandidate>> {
    let mut matches = MagicMatchState::default();
    for entry in repository.magic_rules() {
        if work.charge()
            && let Err(reason) = yield_after_work(scope).await
        {
            return Err(stopped(reason));
        }
        if matches.skips(entry.priority) {
            continue;
        }
        let Some(magic) = repository.magic_rule(entry) else {
            continue;
        };
        let mut matched = false;
        for matcher in magic.matchers() {
            match matcher.matches_cooperatively(bytes, scope, work).await {
                Ok(true) => {
                    matched = true;
                    break;
                }
                Ok(false) => {}
                Err(reason) => {
                    return Err(stopped(reason));
                }
            }
        }
        matches.record(entry.priority, entry.mime_index, matched);
    }
    if let Some(reason) = scope.context().stop_reason() {
        return Err(stopped(reason));
    }
    work.finish();
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

/// Converts an operation stop reason into the repository backend error shape.
///
/// # Parameters
/// - `reason`: Stop cause returned by the operation context.
///
/// # Returns
/// A detector error retaining the stop cause for the caller.
fn stopped(reason: qubit_operation::StopReason) -> MimeError {
    MimeError::DetectorBackend {
        backend: "repository".to_owned(),
        reason: format!("operation stopped: {reason:?}"),
    }
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

#[cfg(test)]
mod cooperative_matcher_tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::time::Instant;

    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    use super::match_content;
    use super::match_content_with_budget;
    use super::preferred_basename;
    use super::select_candidates;
    use crate::MimeCandidate;
    use crate::MimeDetectionPolicy;
    use crate::MimeRepository;
    use crate::MimeType;
    use crate::repository::mime_magic_matcher::MATCH_WORK_PER_BATCH;
    use crate::repository::mime_magic_matcher::MatcherWorkBudget;

    #[test]
    fn test_candidate_selection_filters_unmapped_backend_results() {
        let repository = MimeRepository::from_xml(
            r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
                <mime-type type="application/x-known"/>
            </mime-info>"#,
        )
        .unwrap();
        let backend_candidates = vec![MimeCandidate {
            mime_type: "application/x-not-registered".to_owned(),
            rank: 7,
        }];

        assert_eq!(
            backend_candidates,
            select_candidates(
                &repository,
                backend_candidates.clone(),
                None,
                MimeDetectionPolicy::Freedesktop,
            )
        );
        assert!(
            select_candidates(
                &repository,
                backend_candidates,
                Some("unknown.bin"),
                MimeDetectionPolicy::Freedesktop,
            )
            .is_empty()
        );
    }

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
    #[ignore = "manual debug-profile sampler for MIME-047 matcher work"]
    fn test_reports_work_for_manual_matcher_samples() {
        const ITERATIONS: usize = 200;
        let early_fail = "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\"><mime-type type=\"application/x-early\"><magic priority=\"50\"><match type=\"string\" value=\"NEVER\" offset=\"0:255\"/></magic></mime-type></mime-info>".to_owned();
        let late_mismatch = format!(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\"><mime-type type=\"application/x-late\"><magic priority=\"50\"><match type=\"string\" value=\"{}Z\" offset=\"0:31\"/></magic></mime-type></mime-info>",
            "A".repeat(127),
        );
        let wide_shallow = format!(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\"><mime-type type=\"application/x-wide\"><magic priority=\"50\"><match type=\"string\" value=\"P\" offset=\"0\">{}</match></magic></mime-type></mime-info>",
            (0..512)
                .map(|index| format!("<match type=\"string\" value=\"N{index}\" offset=\"0\"/>"))
                .collect::<String>(),
        );
        let many_roots = format!(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\"><mime-type type=\"application/x-roots\"><magic priority=\"50\">{}</magic></mime-type></mime-info>",
            (0..128)
                .map(|index| format!("<match type=\"string\" value=\"N{index}\" offset=\"0:31\"/>"))
                .collect::<String>(),
        );
        let normal_rules = format!(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">{}</mime-info>",
            (0..64)
                .map(|index| format!("<mime-type type=\"application/x-normal-{index:03}\"><magic priority=\"50\"><match type=\"string\" value=\"SIGNAL-{index:03}\" offset=\"0\"/></magic></mime-type>"))
                .collect::<String>(),
        );
        let scenarios = [
            ("early_fail", early_fail, vec![b'X'; 256]),
            ("late_mismatch", late_mismatch, vec![b'A'; 160]),
            ("wide_shallow", wide_shallow, vec![b'P']),
            ("many_subthreshold_roots", many_roots, vec![b'X'; 32]),
            ("normal_rules_64", normal_rules, b"SIGNAL-063".to_vec()),
        ];

        for (name, xml, bytes) in scenarios {
            let repository = Arc::new(MimeRepository::from_xml(&xml).unwrap());
            let input = Arc::new(bytes);
            for round in 1..=3 {
                let sync_started = Instant::now();
                for _ in 0..ITERATIONS {
                    std::hint::black_box(repository.detect_by_content(&input));
                }
                let sync_ns_per_query = sync_started.elapsed().as_nanos() / ITERATIONS as u128;

                let manager = OperationManager::new(Default::default()).unwrap();
                let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
                let mut operation = block_on(group.try_submit_async(Default::default(), {
                    let repository = Arc::clone(&repository);
                    let input = Arc::clone(&input);
                    move |scope| async move {
                        let started = Instant::now();
                        let mut maximum_poll_ns = 0u128;
                        let mut total_work = 0usize;
                        let mut maximum_poll_work = 0usize;
                        let mut yields = 0usize;
                        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
                        let mut context = Context::from_waker(&waker);
                        for _ in 0..ITERATIONS {
                            let mut work = MatcherWorkBudget::default();
                            let mut matching =
                                Box::pin(match_content_with_budget(&scope, &repository, &input, &mut work));
                            loop {
                                let poll_started = Instant::now();
                                let result = matching.as_mut().poll(&mut context);
                                maximum_poll_ns = maximum_poll_ns.max(poll_started.elapsed().as_nanos());
                                match result {
                                    Poll::Ready(result) => {
                                        assert!(result.is_ok(), "baseline workload should complete");
                                        break;
                                    }
                                    Poll::Pending => {}
                                }
                            }
                            drop(matching);
                            total_work += work.total_work();
                            maximum_poll_work = maximum_poll_work.max(work.maximum_work_in_poll());
                            yields += work.yields();
                        }
                        Ok::<_, std::convert::Infallible>((
                            started.elapsed().as_nanos() / ITERATIONS as u128,
                            maximum_poll_ns,
                            total_work / ITERATIONS,
                            maximum_poll_work,
                            yields,
                        ))
                    }
                }))
                .unwrap();
                let outcome = block_on(operation.wait_outcome()).unwrap();
                let (async_ns_per_query, maximum_poll_ns, work_per_query, maximum_poll_work, yields) = match outcome {
                    OperationOutcome::Succeeded(stats) => stats,
                    other => panic!("matcher sample did not succeed: {other:?}"),
                };
                println!(
                    "matcher {name} round={round}: sync_ns/query={sync_ns_per_query}, async_ns/query={async_ns_per_query}, work/query={work_per_query}, max_poll_work={maximum_poll_work}, max_poll_ns={maximum_poll_ns}, yields={yields}"
                );
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
    fn test_cooperative_matching_yields_across_wide_shallow_branch() {
        let xml = format!(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\"><mime-type type=\"application/x-wide-shallow\"><magic priority=\"50\"><match type=\"string\" value=\"P\" offset=\"0\">{}</match></magic></mime-type></mime-info>",
            (0..10_000)
                .map(|index| format!("<match type=\"string\" value=\"N{index}\" offset=\"0\"/>"))
                .collect::<String>(),
        );
        let repository = Arc::new(MimeRepository::from_xml(&xml).unwrap());
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let mut stop_response_samples_ns = Vec::new();
        for _ in 0..5 {
            let probe_polled = Arc::new(AtomicBool::new(false));
            let stop_requested_at = Arc::new(Mutex::new(None::<Instant>));
            let mut operation = block_on(group.try_submit_async(Default::default(), {
                let probe_polled = Arc::clone(&probe_polled);
                let stop_requested_at = Arc::clone(&stop_requested_at);
                let repository = Arc::clone(&repository);
                move |scope| async move {
                    let mut matching = Box::pin(match_content(&scope, &repository, b"P"));
                    let mut stop_requested = false;
                    let result = std::future::poll_fn(|cx| {
                        let result = matching.as_mut().poll(cx);
                        if result.is_pending() && !stop_requested {
                            stop_requested = true;
                            probe_polled.store(true, Ordering::SeqCst);
                            *stop_requested_at.lock().unwrap() = Some(Instant::now());
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
                "the matcher should yield while traversing a wide shallow branch"
            );
            assert!(matches!(outcome, OperationOutcome::Stopped(report) if report.reason == StopReason::UserCancelled));
            let requested_at = stop_requested_at
                .lock()
                .unwrap()
                .as_ref()
                .copied()
                .expect("the test must observe a pending poll before requesting stop");
            stop_response_samples_ns.push(requested_at.elapsed().as_nanos());
        }
        println!("wide_shallow_stop_response_ns={stop_response_samples_ns:?}");
    }

    #[test]
    fn test_cooperative_work_budget_caps_cross_root_and_byte_comparisons() {
        let xml = format!(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">{}</mime-info>",
            (0..1_000)
                .map(|index| format!("<mime-type type=\"application/x-root-{index}\"><magic priority=\"50\"><match type=\"string\" value=\"ABCZ\" offset=\"0:1\"/></magic></mime-type>"))
                .collect::<String>(),
        );
        let repository = Arc::new(MimeRepository::from_xml(&xml).unwrap());
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let mut work = MatcherWorkBudget::default();
            let result = match_content_with_budget(&scope, &repository, b"ABCX", &mut work).await;
            assert!(result.is_ok());
            Ok::<_, std::convert::Infallible>((work.total_work(), work.maximum_work_in_poll(), work.yields()))
        }))
        .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        let (total_work, maximum_work_in_poll, yields) = match outcome {
            OperationOutcome::Succeeded(stats) => stats,
            other => panic!("work counter operation did not succeed: {other:?}"),
        };
        assert!(total_work > MATCH_WORK_PER_BATCH);
        assert_eq!(maximum_work_in_poll, MATCH_WORK_PER_BATCH);
        assert!(yields > 0);
    }

    #[test]
    fn test_preferred_basename_borrows_unique_match() {
        let repository = MimeRepository::from_xml(
            r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
              <mime-type type="application/x-non-ascii"><glob pattern="报告🚀.mimecase"/></mime-type>
            </mime-info>"#,
        )
        .unwrap();
        let expected = repository.detect_by_filename_basename("报告🚀.mimecase")[0];
        let actual: Option<&MimeType> = preferred_basename(
            &repository,
            Some("报告🚀.mimecase"),
            MimeDetectionPolicy::PreferFilename,
        );

        assert!(actual.is_some_and(|actual| std::ptr::eq(actual, expected)));
        assert!(
            preferred_basename(
                &repository,
                Some("unknown.mimecase"),
                MimeDetectionPolicy::PreferFilename,
            )
            .is_none()
        );
    }

    #[test]
    fn test_async_magic_ranking_matches_sync_order_and_deduplication() {
        let repository = Arc::new(
            MimeRepository::from_xml(
                r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
                  <mime-type type="application/x-low"><magic priority="50"><match type="string" value="HIT" offset="0"/></magic></mime-type>
                  <mime-type type="application/x-high-a"><magic priority="80"><match type="string" value="HIT" offset="0"/></magic><magic priority="80"><match type="string" value="HIT" offset="0"/></magic></mime-type>
                  <mime-type type="application/x-high-b"><magic priority="80"><match type="string" value="HIT" offset="0"/></magic></mime-type>
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
