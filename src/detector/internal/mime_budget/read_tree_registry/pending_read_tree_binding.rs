// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Weak;

use qubit_operation::ResourceManageErrorKind;

use crate::detector::internal::mime_budget::ReadTreeContext;
use crate::detector::internal::mime_budget::parent_read_budget_anchor::ParentReadBudgetAnchor;
use crate::detector::internal::mime_budget::read_tree_registry::pending_binding::PendingBinding;
use crate::detector::internal::mime_budget::read_tree_registry::pending_read_tree_root::unregister_pending;
use crate::detector::internal::mime_budget::read_tree_registry::pending_read_tree_root_state::PendingReadTreeRootState;
use crate::detector::internal::mime_budget::registration::Registration;

/// Holds a parent read-tree binding until child admission is known.
///
/// Accepted siblings keep sharing a pending root if the parent body ends before
/// their bodies start. Dropping the final uncommitted sibling ticket releases
/// the shared reservation.
pub(crate) struct PendingReadTreeBinding {
    /// Existing or first-root binding state to commit after admission.
    pub(super) binding: PendingBinding,
    /// This ticket's immediate MIME parent scope, which may differ between
    /// tickets sharing the same canonical anchor reservation.
    pub(super) scope_parent: Arc<ReadTreeContext>,
    /// Maximum bytes exposed by the child context produced at commit.
    pub(super) scope_limit: u64,
    /// Cumulative limit selected by this first actual provider request.
    pub(super) tree_limit: u64,
}

impl PendingReadTreeBinding {
    /// Commits the parent anchor and returns a child context after admission.
    ///
    /// # Returns
    /// The bounded child context after its parent tree is committed or reused.
    ///
    /// # Errors
    /// Returns a non-terminal resource-scope failure such as an accepted
    /// parent stop. If the parent scope has ended after child admission, the
    /// accepted child receives the detached shared tree without a parent
    /// anchor.
    pub(crate) fn commit(self) -> Result<Arc<ReadTreeContext>, ResourceManageErrorKind> {
        match self.binding {
            PendingBinding::Existing(root) => {
                debug_assert!(Arc::ptr_eq(&root.tree, &self.scope_parent.tree));
                Ok(self.scope_parent.child(self.scope_limit))
            }
            PendingBinding::First {
                roots,
                ancestors,
                pending_roots,
                registrations,
                operation,
                pending_root,
            } => {
                let _registration = registrations.lock().unwrap_or_else(|e| e.into_inner());
                let mut roots_map = roots.lock().unwrap_or_else(|e| e.into_inner());
                let existing = roots_map.get(&operation).and_then(Weak::upgrade);
                if let Some(existing) = existing.as_ref().filter(|root| root.is_anchored()) {
                    debug_assert!(Arc::ptr_eq(&existing.tree, &self.scope_parent.tree));
                    let mut pending_state = pending_root.state.lock().unwrap_or_else(|e| e.into_inner());
                    match &mut *pending_state {
                        PendingReadTreeRootState::Reserved(reservation) => {
                            drop(reservation.take());
                        }
                        PendingReadTreeRootState::Detached | PendingReadTreeRootState::Failed(_) => {}
                    }
                    unregister_pending(&pending_roots, operation, &pending_root);
                    return Ok(self.scope_parent.child(self.scope_limit));
                }
                if existing.is_none() {
                    roots_map.remove(&operation);
                }

                let mut pending_state = pending_root.state.lock().unwrap_or_else(|e| e.into_inner());
                let reservation = match &mut *pending_state {
                    PendingReadTreeRootState::Reserved(reservation) => reservation.take(),
                    PendingReadTreeRootState::Detached => {
                        return Ok(self.scope_parent.child(self.scope_limit));
                    }
                    PendingReadTreeRootState::Failed(kind) => return Err(*kind),
                };
                let Some(reservation) = reservation else {
                    return Err(ResourceManageErrorKind::ScopeEnded);
                };

                let root = Arc::clone(&pending_root.root);
                let _binding = root.tree.binding.lock().unwrap_or_else(|e| e.into_inner());
                let anchor = ParentReadBudgetAnchor::new(Arc::clone(&root));
                match reservation.commit(anchor) {
                    Ok(resource) => {
                        drop(resource);
                        root.select_tree_limit_before_reads(self.tree_limit);
                        root.mark_anchored();
                    }
                    Err(error) => {
                        let (kind, anchor, reservation) = error.into_parts();
                        drop(anchor);
                        if kind == ResourceManageErrorKind::ScopeEnded {
                            // Admission already succeeded, so accepted siblings
                            // must keep sharing this in-memory tree even when
                            // the parent body ended before this child started.
                            // A stopped parent remains an error; ScopeEnded is
                            // safe to detach because it cannot admit new children.
                            drop(reservation);
                            root.select_tree_limit_before_reads(self.tree_limit);
                            *pending_state = PendingReadTreeRootState::Detached;
                            unregister_pending(&pending_roots, operation, &pending_root);
                            return Ok(self.scope_parent.child(self.scope_limit));
                        }
                        drop(reservation);
                        *pending_state = PendingReadTreeRootState::Failed(kind);
                        unregister_pending(&pending_roots, operation, &pending_root);
                        return Err(kind);
                    }
                }

                roots_map.insert(operation, Arc::downgrade(&root));
                ancestors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(operation, Arc::downgrade(&root));
                unregister_pending(&pending_roots, operation, &pending_root);
                root.register(Registration {
                    roots: Arc::downgrade(&roots),
                    operation,
                });
                root.register(Registration {
                    roots: Arc::downgrade(&ancestors),
                    operation,
                });
                Ok(self.scope_parent.child(self.scope_limit))
            }
        }
    }
}
