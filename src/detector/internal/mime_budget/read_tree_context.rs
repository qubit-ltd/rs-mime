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
use super::RequestStagingUsage;
use super::ScopeCounters;
use super::registration::Registration;

/// Read accounting context inherited by MIME descendants.
#[derive(Debug)]
pub(crate) struct ReadTreeContext {
    /// Request-local accounting for concurrently held staging permits.
    #[allow(dead_code)] // Used by request report assembly in the following task.
    pub(super) staging_usage: Arc<RequestStagingUsage>,
    /// Root-level allowance shared across related contexts.
    pub(super) tree: Arc<ReadTreeBudget>,
    /// Allowance and counters for this operation scope.
    pub(super) scope: Arc<ReadScopeBudget>,
    /// Keeps an external-parent anchor alive while a child operation is held.
    #[allow(dead_code)]
    parent: Option<Arc<ReadTreeContext>>,
    /// Operation IDs that must be unregistered when this context is dropped.
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
            staging_usage: Arc::new(RequestStagingUsage::default()),
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
            staging_usage: Arc::new(RequestStagingUsage::default()),
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

    /// Records one weak registry entry for final-drop cleanup.
    ///
    /// # Parameters
    /// - `registration`: Weak roots map and operation ID to remove on drop.
    pub(super) fn register(&self, registration: Registration) {
        let mut registrations = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        if !registrations
            .iter()
            .any(|item| item.operation == registration.operation && Weak::ptr_eq(&item.roots, &registration.roots))
        {
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

/// Removes only registry entries that still point to this context.
impl Drop for ReadTreeContext {
    /// Unregisters this context from each weak operation index.
    fn drop(&mut self) {
        let registrations = std::mem::take(&mut *self.registrations.lock().unwrap_or_else(|e| e.into_inner()));
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
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::OperationId;
    use qubit_operation::OperationOutcome;

    use super::super::registration::Registration;
    use super::ReadTreeContext;

    struct ThreadWake(std::thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match Pin::as_mut(&mut future).poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    fn operation_id() -> OperationId {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
        let mut operation = group
            .as_async()
            .try_submit(Default::default(), |scope| async move { Ok::<_, ()>(scope.id()) })
            .unwrap();
        match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(id) => id,
            other => panic!("operation should return its own identity: {other:?}"),
        }
    }

    #[test]
    fn request_staging_contexts_do_not_share_usage_with_parent_or_siblings() {
        let root = ReadTreeContext::new(10);
        let first = root.child(10);
        let second = root.child(10);

        assert!(!Arc::ptr_eq(&root.staging_usage, &first.staging_usage));
        assert!(!Arc::ptr_eq(&first.staging_usage, &second.staging_usage));
    }

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

    #[test]
    fn test_drop_handles_registry_expiring_before_read_context() {
        let operation = operation_id();
        let roots = Arc::new(Mutex::new(HashMap::new()));
        let context = ReadTreeContext::new(8);
        context.register(Registration {
            roots: Arc::downgrade(&roots),
            operation,
        });

        drop(roots);
        drop(context);
    }

    #[test]
    fn test_register_deduplicates_cleanup_entries() {
        let operation = operation_id();
        let roots = Arc::new(Mutex::new(HashMap::new()));
        let context = ReadTreeContext::new(8);
        let registration = || Registration {
            roots: Arc::downgrade(&roots),
            operation,
        };

        context.register(registration());
        context.register(registration());
        assert_eq!(
            1,
            context
                .registrations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len()
        );
    }

    #[test]
    fn test_drop_preserves_replacement_registry_entry() {
        let operation = operation_id();
        let roots = Arc::new(Mutex::new(HashMap::new()));
        let old = ReadTreeContext::new(8);
        let replacement = ReadTreeContext::new(16);
        roots.lock().unwrap().insert(operation, Arc::downgrade(&replacement));
        old.register(Registration {
            roots: Arc::downgrade(&roots),
            operation,
        });

        drop(old);

        assert!(Arc::ptr_eq(
            &replacement,
            &roots.lock().unwrap().get(&operation).unwrap().upgrade().unwrap()
        ));
    }

    #[test]
    fn test_register_recovers_poisoned_cleanup_lock() {
        let operation = operation_id();
        let roots = Arc::new(Mutex::new(HashMap::new()));
        let context = ReadTreeContext::new(8);
        let poisoned = Arc::clone(&context);
        assert!(
            std::thread::spawn(move || {
                let _guard = poisoned.registrations.lock().unwrap();
                panic!("poison read-tree registrations lock");
            })
            .join()
            .is_err()
        );

        context.register(Registration {
            roots: Arc::downgrade(&roots),
            operation,
        });
        assert_eq!(
            1,
            context
                .registrations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len()
        );
    }
}
