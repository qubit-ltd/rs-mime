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

/// A canonical asynchronous provider candidate, or a known sync-only provider.
#[derive(Clone)]
pub(crate) enum AsyncProviderCandidate<T> {
    /// A snapshotted factory that can construct the requested backend.
    Async { id: String, provider: T },
    /// A registered provider that does not expose an async factory.
    SyncOnly { id: String },
}

/// Resolves selectors against async and sync catalogs without running factories.
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
                right_priority
                    .cmp(left_priority)
                    .then_with(|| left_id.cmp(right_id))
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

    if !unknown.is_empty()
        && !matches!(selection.target(), ProviderSelectionTargetRef::Chain { .. })
    {
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

/// Returns whether a factory error allows candidate fallback.
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

fn unknown_selection_error(service: &str, selectors: &[String]) -> MimeError {
    MimeError::ProviderSelection(format!(
        "unknown {service} selector(s): {}",
        selectors.join(", ")
    ))
}
