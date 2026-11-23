// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Shared selection rules for asynchronous MIME provider catalogs.

use std::collections::HashMap;
use std::collections::HashSet;

use qubit_spi::FallbackPolicy;
use qubit_spi::MissingProviderPolicy;
use qubit_spi::ProviderSelection;
use qubit_spi::ProviderSelectionTargetRef;

use crate::MimeError;

mod internal;
pub(crate) use internal::async_provider_candidate::AsyncProviderCandidate;

/// Resolves a selection against async and sync catalogs without running factories.
///
/// # Type Parameters
/// - `T`: Cloneable async factory entry retained in the resolved candidate snapshot.
///
/// # Parameters
/// - `selection`: Validated provider selection, including missing-selector and fallback policies.
/// - `async_catalog`: Selector-to-canonical-ID, priority, and async factory snapshots.
/// - `sync_provider_id`: Lookup that returns a canonical ID for a known sync provider.
/// - `auto_builtin_ids`: Built-in IDs in the registry's stable Auto order.
/// - `auto_priority_ids`: Other registered candidates with their descriptor priorities.
/// - `service`: Human-readable provider category used in diagnostics.
///
/// # Returns
/// An ordered snapshot of async factories and sync-only candidates eligible for this selection.
///
/// # Errors
/// Returns a selection diagnostic when required selectors are unknown or no candidates match.
pub(crate) fn resolve<T: Clone>(
    selection: &ProviderSelection,
    async_catalog: &HashMap<String, (String, i32, T)>,
    sync_provider_id: impl Fn(&str) -> Option<String>,
    auto_builtin_ids: Vec<String>,
    mut auto_priority_ids: Vec<(String, i32)>,
    service: &str,
) -> Result<Vec<AsyncProviderCandidate<T>>, MimeError> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut unknown = Vec::new();

    match selection.target() {
        ProviderSelectionTargetRef::Auto => {
            for id in auto_builtin_ids {
                push_candidate(&mut candidates, &mut seen, &id, async_catalog);
            }

            auto_priority_ids.extend(
                async_catalog
                    .values()
                    .filter(|(id, _, _)| !seen.contains(id))
                    .map(|(id, priority, _)| (id.clone(), *priority))
                    .collect::<Vec<_>>(),
            );
            auto_priority_ids.sort_by(|(left_id, left_priority), (right_id, right_priority)| {
                right_priority.cmp(left_priority).then_with(|| left_id.cmp(right_id))
            });
            for (id, _) in auto_priority_ids {
                push_candidate(&mut candidates, &mut seen, &id, async_catalog);
            }
        }
        ProviderSelectionTargetRef::Named(selector) => {
            let selector = selector.as_str();
            if let Some((id, _, provider)) = async_catalog.get(selector) {
                push_async(&mut candidates, &mut seen, id, provider.clone());
            } else if let Some(id) = sync_provider_id(selector) {
                push_candidate(&mut candidates, &mut seen, &id, async_catalog);
            } else {
                unknown.push(selector.to_owned());
            }
        }
        ProviderSelectionTargetRef::Chain {
            selectors,
            missing_policy,
        } => {
            for selector in selectors {
                let selector = selector.as_str();
                if let Some((id, _, provider)) = async_catalog.get(selector) {
                    push_async(&mut candidates, &mut seen, id, provider.clone());
                } else if let Some(id) = sync_provider_id(selector) {
                    push_candidate(&mut candidates, &mut seen, &id, async_catalog);
                } else {
                    unknown.push(selector.to_owned());
                }
            }
            if missing_policy == MissingProviderPolicy::Reject && !unknown.is_empty() {
                return Err(unknown_selection_error(service, &unknown));
            }
        }
        _ => {
            return Err(MimeError::ProviderSelection(format!(
                "unsupported {service} selection target"
            )));
        }
    }

    if !unknown.is_empty() && !matches!(selection.target(), ProviderSelectionTargetRef::Chain { .. }) {
        return Err(unknown_selection_error(service, &unknown));
    }
    if candidates.is_empty() {
        let detail = if unknown.is_empty() {
            "no registered selectors matched".to_owned()
        } else {
            format!("unknown selector(s): {}", unknown.join(", "))
        };
        return Err(MimeError::ProviderSelection(format!(
            "no {service} candidates match the selection ({detail})"
        )));
    }
    Ok(candidates)
}

/// Returns whether the fallback policy permits continuing after a factory error.
///
/// # Parameters
/// - `policy`: Selection policy that controls candidate fallback.
/// - `error`: Factory error to classify for absence-based fallback.
///
/// # Returns
/// `true` when the selection permits another candidate to be tried.
pub(crate) fn should_fallback(policy: FallbackPolicy, error: &MimeError) -> bool {
    match policy {
        FallbackPolicy::Never => false,
        FallbackPolicy::OnAbsence => matches!(
            error,
            MimeError::AsyncUnsupported
                | MimeError::UnsupportedCapability { .. }
                | MimeError::DetectorUnavailable { .. }
                | MimeError::ClassifierUnavailable { .. }
        ),
        FallbackPolicy::OnAnyError => true,
        _ => false,
    }
}

/// Adds a registered selector's canonical provider once to the candidate list.
///
/// # Type Parameters
/// - `T`: Cloneable async factory entry stored in the catalog.
///
/// # Parameters
/// - `candidates`: Candidate snapshot being built.
/// - `seen`: Canonical IDs already included in the snapshot.
/// - `id`: Selector or canonical provider ID to resolve.
/// - `async_catalog`: Async factories indexed by canonical IDs and aliases.
fn push_candidate<T: Clone>(
    candidates: &mut Vec<AsyncProviderCandidate<T>>,
    seen: &mut HashSet<String>,
    id: &str,
    async_catalog: &HashMap<String, (String, i32, T)>,
) {
    if let Some((canonical_id, _, provider)) = async_catalog.get(id) {
        push_async(candidates, seen, canonical_id, provider.clone());
    } else if seen.insert(id.to_owned()) {
        candidates.push(AsyncProviderCandidate::SyncOnly { id: id.to_owned() });
    }
}

/// Adds an async provider factory once by its canonical ID.
///
/// # Type Parameters
/// - `T`: Async factory entry stored in the catalog.
///
/// # Parameters
/// - `candidates`: Candidate snapshot being built.
/// - `seen`: Canonical IDs already included in the snapshot.
/// - `id`: Canonical provider ID to add.
/// - `provider`: Snapshotted async factory entry.
fn push_async<T: Clone>(
    candidates: &mut Vec<AsyncProviderCandidate<T>>,
    seen: &mut HashSet<String>,
    id: &str,
    provider: T,
) {
    if seen.insert(id.to_owned()) {
        candidates.push(AsyncProviderCandidate::Async {
            id: id.to_owned(),
            provider,
        });
    }
}

/// Creates a selector-resolution diagnostic that retains each unknown selector.
///
/// # Parameters
/// - `service`: Human-readable provider category.
/// - `selectors`: Unknown selectors in input order.
///
/// # Returns
/// A MIME error whose display text names the provider category and selectors.
fn unknown_selection_error(service: &str, selectors: &[String]) -> MimeError {
    MimeError::ProviderSelection(format!("unknown {service} selector(s): {}", selectors.join(", ")))
}
