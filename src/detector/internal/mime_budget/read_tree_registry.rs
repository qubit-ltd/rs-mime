// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg

#[cfg(coverage)]
use std::panic::AssertUnwindSafe;
#[cfg(coverage)]
use std::panic::catch_unwind;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use qubit_operation::OperationId;
use qubit_operation::OperationScope;
use qubit_operation::ResourceManageErrorKind;

use super::ReadTreeContext;
use super::parent_read_budget_anchor::ParentReadBudgetAnchor;
use super::registration::Registration;
use super::registration::Roots;

/// Weakly indexes read-budget roots by operation ID.
#[derive(Debug, Default)]
pub(in crate::detector) struct ReadTreeRegistry {
    /// Root contexts retained only while an operation or descendant owns them.
    roots: Arc<Roots>,
    /// Serializes first registration with the external parent resource scope.
    registrations: Mutex<()>,
}

impl ReadTreeRegistry {
    /// Creates a child context for an operation, creating its root if needed.
    ///
    /// # Parameters
    /// - `parent`: External parent operation whose resource scope owns the
    ///   tree.
    /// - `tree_limit`: Root-tree allowance used when no root is registered.
    /// - `scope_limit`: Allowance requested for the new child scope.
    ///
    /// # Returns
    /// A child context that shares the operation's root accounting.
    ///
    /// # Errors
    /// Returns the resource-scope failure when the parent cannot retain its
    /// cumulative read-tree anchor.
    pub(in crate::detector) fn child_for_parent(
        &self,
        parent: &OperationScope,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Result<Arc<ReadTreeContext>, ResourceManageErrorKind> {
        let _registration = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        let operation = parent.id();
        {
            let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(root) = roots.get(&operation).and_then(Weak::upgrade) {
                return Ok(root.child(scope_limit));
            }
            roots.remove(&operation);
        }

        let root = ReadTreeContext::new(tree_limit);
        let anchor = ParentReadBudgetAnchor::new(Arc::clone(&root));
        let resource = parent.manage(anchor).map_err(|error| error.kind())?;
        drop(resource);

        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(operation, Arc::downgrade(&root));
        root.register(Registration {
            roots: Arc::downgrade(&self.roots),
            operation,
        });
        Ok(root.child(scope_limit))
    }

    /// Looks up a live context registered for an operation.
    ///
    /// # Parameters
    /// - `id`: Operation ID to look up.
    ///
    /// # Returns
    /// The upgraded context when it is still owned, or `None` after it expires.
    #[inline]
    pub(crate) fn for_operation(&self, id: OperationId) -> Option<Arc<ReadTreeContext>> {
        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .and_then(Weak::upgrade)
    }

    /// Poisons the registration mutex so coverage can exercise recovery.
    #[cfg(coverage)]
    pub(crate) fn poison_registrations_for_coverage(&self) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = self.registrations.lock().unwrap();
            panic!("poison read-tree registration lock for coverage");
        }));
    }

    /// Associates an operation ID with an existing read-tree context.
    ///
    /// # Parameters
    /// - `id`: Operation ID to associate.
    /// - `tree`: Context whose root should be reused by child operations.
    pub(in crate::detector) fn bind(&self, id: OperationId, tree: &Arc<ReadTreeContext>) {
        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Arc::downgrade(tree));
        tree.register(Registration {
            roots: Arc::downgrade(&self.roots),
            operation: id,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::thread;

    use qubit_operation::GroupOptions;
    use qubit_operation::OperationId;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::OperationScope;

    use super::ReadTreeContext;
    use super::ReadTreeRegistry;

    struct ThreadWake(thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Runs a future to completion, parking the test thread until it is woken.
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match Pin::as_mut(&mut future).poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => thread::park(),
            }
        }
    }

    /// Starts a completed operation and returns its identity for registry
    /// tests.
    fn operation_id() -> OperationId {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let mut operation = group
            .as_async()
            .try_submit(Default::default(), |scope| async move { Ok::<_, ()>(scope.id()) })
            .unwrap();

        match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(id) => id,
            other => panic!("operation should return its own identity: {other:?}"),
        }
    }

    /// Runs a test closure in a managed parent operation and waits for
    /// settlement.
    fn with_parent_scope<T: Send + 'static>(body: impl FnOnce(&OperationScope) -> T + Send + 'static) -> T {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let mut operation = group
            .as_async()
            .try_submit(
                Default::default(),
                move |scope| async move { Ok::<_, ()>(body(&scope)) },
            )
            .unwrap();
        let result = match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(result) => result,
            _ => panic!("parent operation should succeed"),
        };
        let _settlement = block_on(operation.wait_settlement());
        result
    }

    #[test]
    fn test_bound_operation_reuses_parent_tree_and_expires_weak_registration() {
        let registry = Arc::new(ReadTreeRegistry::default());
        let registry_body = Arc::clone(&registry);
        let operation = with_parent_scope(move |scope| {
            let operation = scope.id();
            let root = ReadTreeContext::new(8);

            assert!(registry_body.for_operation(operation).is_none());
            registry_body.bind(operation, &root);
            assert!(Arc::ptr_eq(
                &root,
                &registry_body
                    .for_operation(operation)
                    .expect("bound root should be discoverable")
            ));

            let child = registry_body.child_for_parent(scope, 100, 5).unwrap();
            assert!(Arc::ptr_eq(&root.tree, &child.tree));
            assert_eq!(child.reserve(5).unwrap().commit(5), 5);
            assert_eq!(root.consumed(), 5);

            drop(root);
            assert!(registry_body.for_operation(operation).is_some());
            drop(child);
            assert!(registry_body.for_operation(operation).is_none());
            operation
        });
        assert!(registry.for_operation(operation).is_none());
    }

    #[test]
    fn test_external_parent_tree_and_limit_survive_sequential_children() {
        let registry = Arc::new(ReadTreeRegistry::default());
        let registry_body = Arc::clone(&registry);
        let operation = with_parent_scope(move |scope| {
            let first = registry_body.child_for_parent(scope, 3, 3).unwrap();
            assert_eq!(first.reserve(3).unwrap().commit(3), 3);
            let tree = Arc::clone(&first.tree);
            drop(first);
            assert!(registry_body.for_operation(scope.id()).is_some());

            let second = registry_body.child_for_parent(scope, 6, 6).unwrap();
            assert!(Arc::ptr_eq(&tree, &second.tree));
            assert_eq!(second.tree.consumed(), 3);
            assert!(second.reserve(1).is_none());
            scope.id()
        });
        assert!(registry.for_operation(operation).is_none());
    }

    #[test]
    fn test_child_replaces_expired_weak_root_registration() {
        let registry = Arc::new(ReadTreeRegistry::default());
        let registry_body = Arc::clone(&registry);
        let operation = with_parent_scope(move |scope| {
            let old_root = ReadTreeContext::new(2);
            registry_body
                .roots
                .lock()
                .unwrap()
                .insert(scope.id(), Arc::downgrade(&old_root));
            drop(old_root);

            let child = registry_body.child_for_parent(scope, 8, 4).unwrap();
            assert_eq!(4, child.remaining());
            let registered = registry_body.for_operation(scope.id()).unwrap();
            assert!(Arc::ptr_eq(&child.tree, &registered.tree));
            scope.id()
        });
        assert!(registry.for_operation(operation).is_none());
    }

    #[test]
    fn test_root_bindings_are_removed_after_context_drop() {
        let registry = ReadTreeRegistry::default();
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 32 }).unwrap();

        for _ in 0..32 {
            let mut operation = group
                .as_async()
                .try_submit(Default::default(), |scope| async move { Ok::<_, ()>(scope.id()) })
                .unwrap();
            let id = match block_on(operation.wait_outcome()).unwrap() {
                OperationOutcome::Succeeded(id) => id,
                other => panic!("operation should return its own identity: {other:?}"),
            };
            let root = ReadTreeContext::new(8);
            registry.bind(id, &root);
            assert!(registry.for_operation(id).is_some());
            drop(root);
            assert!(registry.for_operation(id).is_none());
            assert!(registry.roots.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn test_dropping_replaced_context_keeps_new_binding() {
        let registry = ReadTreeRegistry::default();
        let id = operation_id();
        let old = ReadTreeContext::new(8);
        let replacement = ReadTreeContext::new(16);

        registry.bind(id, &old);
        registry.bind(id, &replacement);
        drop(old);

        assert!(Arc::ptr_eq(
            &replacement,
            &registry.for_operation(id).expect("replacement should remain bound")
        ));
        assert_eq!(1, registry.roots.lock().unwrap().len());
        drop(replacement);
        assert!(registry.roots.lock().unwrap().is_empty());
    }

    #[test]
    fn test_context_unregisters_multiple_ids_and_survives_registry_drop() {
        let id_a = operation_id();
        let id_b = operation_id();
        let context = ReadTreeContext::new(8);
        let registry = ReadTreeRegistry::default();

        registry.bind(id_a, &context);
        registry.bind(id_b, &context);
        assert_eq!(2, registry.roots.lock().unwrap().len());
        drop(context);
        assert!(registry.roots.lock().unwrap().is_empty());

        let context = ReadTreeContext::new(8);
        let registry = ReadTreeRegistry::default();
        registry.bind(id_a, &context);
        drop(registry);
        drop(context);
    }

    #[test]
    fn test_context_drop_recovers_poisoned_roots_lock() {
        let registry = ReadTreeRegistry::default();
        let id = operation_id();
        let context = ReadTreeContext::new(8);
        registry.bind(id, &context);
        let roots = Arc::clone(&registry.roots);
        assert!(
            thread::spawn(move || {
                let _guard = roots.lock().unwrap();
                panic!("poison read-tree roots lock");
            })
            .join()
            .is_err()
        );

        drop(context);
        assert!(registry.roots.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
    }
}
