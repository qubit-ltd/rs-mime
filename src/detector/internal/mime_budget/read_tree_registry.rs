// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Weak;

use qubit_operation::OperationId;

use super::ReadTreeContext;
use super::read_tree_registration::Registration;
use super::read_tree_registration::Roots;

/// Weakly indexes read-budget roots by operation ID.
#[derive(Debug, Default)]
pub(in crate::detector) struct ReadTreeRegistry {
    /// Root contexts retained only while an operation or descendant owns them.
    roots: Arc<Roots>,
}

impl ReadTreeRegistry {
    /// Creates a child context for an operation, creating its root if needed.
    ///
    /// # Parameters
    /// - `id`: Operation ID whose parent read tree is being extended.
    /// - `tree_limit`: Root-tree allowance used when no root is registered.
    /// - `scope_limit`: Allowance requested for the new child scope.
    ///
    /// # Returns
    /// A child context that shares the operation's root accounting.
    pub(in crate::detector) fn child_for_parent(
        &self,
        id: OperationId,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Arc<ReadTreeContext> {
        let parent = {
            let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(parent) = roots.get(&id).and_then(Weak::upgrade) {
                parent
            } else {
                roots.remove(&id);
                let context = ReadTreeContext::new(tree_limit);
                roots.insert(id, Arc::downgrade(&context));
                context.register(Registration {
                    roots: Arc::downgrade(&self.roots),
                    operation: id,
                });
                context
            }
        };
        parent.child(scope_limit)
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

    /// Looks up a live context registered for an operation.
    ///
    /// # Parameters
    /// - `id`: Operation ID to look up.
    ///
    /// # Returns
    /// The upgraded context when it is still owned, or `None` after it expires.
    pub(crate) fn for_operation(&self, id: OperationId) -> Option<Arc<ReadTreeContext>> {
        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .and_then(Weak::upgrade)
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

    use qubit_operation::OperationId;
    use qubit_operation::OperationOutcome;

    use super::ReadTreeContext;
    use super::ReadTreeRegistry;

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
    fn test_bound_operation_reuses_parent_tree_and_expires_weak_registration() {
        let registry = ReadTreeRegistry::default();
        let operation = operation_id();
        let root = ReadTreeContext::new(8);

        assert!(registry.for_operation(operation).is_none());
        registry.bind(operation, &root);
        assert!(Arc::ptr_eq(
            &root,
            &registry
                .for_operation(operation)
                .expect("bound root should be discoverable")
        ));

        let child = registry.child_for_parent(operation, 100, 5);
        assert!(Arc::ptr_eq(&root.tree, &child.tree));
        assert_eq!(child.reserve(5).unwrap().commit(5), 5);
        assert_eq!(root.consumed(), 5);

        drop(root);
        assert!(registry.for_operation(operation).is_some());
        drop(child);
        assert!(registry.for_operation(operation).is_none());
    }

    #[test]
    fn test_expired_operation_tree_is_replaced_with_new_limits() {
        let registry = ReadTreeRegistry::default();
        let operation = operation_id();

        let first = registry.child_for_parent(operation, 3, 3);
        assert_eq!(first.reserve(3).unwrap().commit(3), 3);
        drop(first);
        assert!(registry.for_operation(operation).is_none());

        let replacement = registry.child_for_parent(operation, 6, 6);
        assert_eq!(replacement.reserve(6).unwrap().commit(6), 6);
        assert_eq!(replacement.tree.consumed(), 6);
    }

    #[test]
    fn test_root_bindings_are_removed_after_context_drop() {
        let registry = ReadTreeRegistry::default();
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager
            .group(qubit_operation::GroupOptions { capacity: 32 })
            .unwrap();

        for _ in 0..32 {
            let mut operation = group
                .as_async()
                .try_submit(Default::default(), |scope| async move {
                    Ok::<_, ()>(scope.id())
                })
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
            &registry
                .for_operation(id)
                .expect("replacement should remain bound")
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
            std::thread::spawn(move || {
                let _guard = roots.lock().unwrap();
                panic!("poison read-tree roots lock");
            })
            .join()
            .is_err()
        );

        drop(context);
        assert!(
            registry
                .roots
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );
    }
}
