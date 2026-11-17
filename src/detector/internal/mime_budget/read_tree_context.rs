// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use super::ReadPermit;
use super::ReadScopeBudget;
use super::ReadTreeBudget;
use super::ScopeCounters;
use super::read_tree_registration::Registration;

/// Read accounting context inherited by MIME descendants.
#[derive(Debug)]
pub(crate) struct ReadTreeContext {
    /// Root-level allowance shared across related contexts.
    pub(super) tree: Arc<ReadTreeBudget>,
    /// Allowance and counters for this operation scope.
    pub(super) scope: Arc<ReadScopeBudget>,
    /// Keeps an external-parent anchor alive while a child operation is held.
    #[allow(dead_code)]
    parent: Option<Arc<ReadTreeContext>>,
    registrations: Mutex<Vec<Registration>>,
}

impl ReadTreeContext {
    /// Creates an independent root context with the given byte allowance.
    ///
    /// # Parameters
    /// - `limit`: Maximum bytes readable by the root and its descendants.
    ///
    /// # Returns
    /// A reference-counted root context.
    pub(crate) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            tree: ReadTreeBudget::new(limit),
            scope: Arc::new(ReadScopeBudget {
                limit,
                counters: Mutex::new(ScopeCounters::default()),
                parent: None,
            }),
            parent: None,
            registrations: Mutex::new(Vec::new()),
        })
    }

    /// Creates a child scope that shares the root and is bounded by its parent.
    ///
    /// # Parameters
    /// - `limit`: Requested child allowance, capped by the parent's scope
    ///   limit.
    ///
    /// # Returns
    /// A child context whose reads charge the ancestor scopes.
    pub(in crate::detector) fn child(self: &Arc<Self>, limit: u64) -> Arc<Self> {
        Arc::new(Self {
            tree: Arc::clone(&self.tree),
            scope: Arc::new(ReadScopeBudget {
                limit: limit.min(self.scope.limit),
                counters: Mutex::new(ScopeCounters::default()),
                parent: Some(Arc::clone(&self.scope)),
            }),
            parent: Some(Arc::clone(self)),
            registrations: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn register(&self, registration: Registration) {
        let mut registrations = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        if !registrations.iter().any(|item| {
            item.operation == registration.operation
                && Weak::ptr_eq(&item.roots, &registration.roots)
        }) {
            registrations.push(registration);
        }
    }

    /// Reserves read capacity across the root tree and every ancestor scope.
    ///
    /// # Parameters
    /// - `bytes`: Maximum number of bytes expected from the read.
    ///
    /// # Returns
    /// A permit when every applicable limit has capacity; otherwise `None`.
    pub(crate) fn reserve(&self, bytes: u64) -> Option<ReadPermit> {
        let mut tree = self.tree.counters.lock().unwrap_or_else(|e| e.into_inner());
        if bytes
            > self
                .tree
                .limit
                .saturating_sub(tree.consumed.saturating_add(tree.reserved))
        {
            return None;
        }
        tree.reserved += bytes;
        drop(tree);

        let mut scopes: Vec<Arc<ReadScopeBudget>> = Vec::new();
        let mut scope = Some(Arc::clone(&self.scope));
        while let Some(current) = scope {
            let mut counters = current.counters.lock().unwrap_or_else(|e| e.into_inner());
            if bytes
                > current
                    .limit
                    .saturating_sub(counters.consumed.saturating_add(counters.reserved))
            {
                drop(counters);
                for reserved in &scopes {
                    reserved.counters.lock().unwrap_or_else(|e| e.into_inner()).reserved -= bytes;
                }
                self.tree.counters.lock().unwrap_or_else(|e| e.into_inner()).reserved -= bytes;
                return None;
            }
            counters.reserved += bytes;
            drop(counters);
            scope = current.parent.clone();
            scopes.push(current);
        }
        Some(ReadPermit {
            tree: Arc::clone(&self.tree),
            scopes,
            reserved: bytes,
            committed: false,
        })
    }

    /// Returns the smallest remaining allowance across this scope's ancestors.
    pub(crate) fn remaining(&self) -> u64 {
        let tree = self.tree.counters.lock().unwrap_or_else(|e| e.into_inner());
        let mut remaining = self
            .tree
            .limit
            .saturating_sub(tree.consumed.saturating_add(tree.reserved));
        drop(tree);
        let mut scope = Some(Arc::clone(&self.scope));
        while let Some(current) = scope {
            let counters = current.counters.lock().unwrap_or_else(|e| e.into_inner());
            remaining = remaining.min(
                current
                    .limit
                    .saturating_sub(counters.consumed.saturating_add(counters.reserved)),
            );
            drop(counters);
            scope = current.parent.clone();
        }
        remaining
    }

    /// Returns bytes committed in this operation's inherited read tree.
    pub(crate) fn consumed(&self) -> u64 {
        self.scope.counters.lock().unwrap_or_else(|e| e.into_inner()).consumed
    }
}

impl Drop for ReadTreeContext {
    fn drop(&mut self) {
        let registrations =
            std::mem::take(&mut *self.registrations.lock().unwrap_or_else(|e| e.into_inner()));
        let context = std::ptr::from_ref(self);
        for registration in registrations {
            let Some(roots) = registration.roots.upgrade() else {
                continue;
            };
            let mut roots = roots.lock().unwrap_or_else(|e| e.into_inner());
            let matches = roots
                .get(&registration.operation)
                .is_some_and(|registered| std::ptr::eq(registered.as_ptr(), context));
            if matches {
                roots.remove(&registration.operation);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReadTreeContext;

    #[test]
    fn test_read_tree_reservation_and_commit_recover_poisoned_scope_counters() {
        let root = ReadTreeContext::new(10);
        let child = root.child(8);
        let tree_counters = std::sync::Arc::clone(&root.tree);
        assert!(
            std::thread::spawn(move || {
                let _guard = tree_counters.counters.lock().unwrap();
                panic!("poison shared read-tree counters");
            })
            .join()
            .is_err()
        );
        let root_scope = std::sync::Arc::clone(&root.scope);
        assert!(
            std::thread::spawn(move || {
                let _guard = root_scope.counters.lock().unwrap();
                panic!("poison root read-scope counters");
            })
            .join()
            .is_err()
        );
        let child_scope = std::sync::Arc::clone(&child.scope);
        assert!(
            std::thread::spawn(move || {
                let _guard = child_scope.counters.lock().unwrap();
                panic!("poison child read-scope counters");
            })
            .join()
            .is_err()
        );

        let permit = child.reserve(4).expect("reservation should recover all counters");
        assert_eq!(3, permit.commit(3));
        assert_eq!(3, root.consumed());
        assert_eq!(3, child.consumed());
        assert_eq!(7, root.remaining());
        assert_eq!(5, child.remaining());
    }

    #[test]
    fn test_child_read_limit_charges_ancestors_and_releases_unused_capacity() {
        let root = ReadTreeContext::new(10);
        let child = root.child(5);
        let permit = child.reserve(4).expect("child read should fit both limits");
        assert_eq!(root.remaining(), 6);
        assert_eq!(child.remaining(), 1);
        assert_eq!(permit.commit(3), 3);
        assert_eq!(root.consumed(), 3);
        assert_eq!(child.consumed(), 3);
        assert_eq!(root.remaining(), 7);
        assert_eq!(child.remaining(), 2);
        assert!(child.reserve(3).is_none());

        let provisional = child.reserve(2).expect("remaining child capacity should fit");
        assert_eq!(root.remaining(), 5);
        drop(provisional);
        assert_eq!(root.remaining(), 7);
        assert_eq!(child.remaining(), 2);
    }

    #[test]
    fn test_nested_reservation_failure_restores_every_ancestor() {
        let root = ReadTreeContext::new(20);
        let child = root.child(5);
        let grandchild = child.child(10);

        assert_eq!(child.reserve(4).unwrap().commit(4), 4);
        assert_eq!(root.remaining(), 16);
        assert_eq!(child.remaining(), 1);
        assert_eq!(grandchild.remaining(), 1);

        assert!(grandchild.reserve(2).is_none());
        assert_eq!(root.consumed(), 4);
        assert_eq!(child.consumed(), 4);
        assert_eq!(grandchild.consumed(), 0);
        assert_eq!(root.remaining(), 16);
        assert_eq!(child.remaining(), 1);
        assert_eq!(grandchild.remaining(), 1);

        assert_eq!(grandchild.reserve(1).unwrap().commit(1), 1);
        assert_eq!(root.consumed(), 5);
        assert_eq!(child.consumed(), 5);
        assert_eq!(grandchild.consumed(), 1);
    }

    #[test]
    fn test_zero_byte_read_does_not_consume_any_scope_capacity() {
        let root = ReadTreeContext::new(4);
        let child = root.child(2);
        assert_eq!(child.reserve(0).unwrap().commit(0), 0);

        assert_eq!(root.consumed(), 0);
        assert_eq!(child.consumed(), 0);
        assert_eq!(root.remaining(), 4);
        assert_eq!(child.remaining(), 2);
    }

    #[test]
    fn test_inflight_sibling_reservations_cannot_overcommit_tree_limit() {
        let root = ReadTreeContext::new(5);
        let first = root.child(5);
        let sibling = root.child(5);
        let in_flight = first.reserve(4).expect("first reservation fits the root");

        assert_eq!(root.remaining(), 1);
        assert!(sibling.reserve(2).is_none());
        assert_eq!(root.remaining(), 1, "failed sibling reservation must roll back");

        drop(in_flight);
        assert_eq!(root.remaining(), 5);
        assert_eq!(sibling.reserve(5).unwrap().commit(5), 5);
        assert_eq!(root.consumed(), 5);
        assert_eq!(root.remaining(), 0);
    }
}
