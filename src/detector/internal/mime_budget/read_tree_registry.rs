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

use pending_binding::PendingBinding;
use pending_read_tree_root::PendingReadTreeRoot;
use pending_read_tree_root::PendingRoots;
use pending_read_tree_root_state::PendingReadTreeRootState;
use qubit_operation::OperationId;
use qubit_operation::OperationScope;
use qubit_operation::ResourceCapacityError;
use qubit_operation::ResourceManageErrorKind;

use super::ReadTreeContext;
use super::parent_read_budget_anchor::ParentReadBudgetAnchor;
use super::registration::Registration;
use super::registration::Roots;

#[path = "read_tree_registry/pending_binding.rs"]
mod pending_binding;
#[path = "read_tree_registry/pending_read_tree_binding.rs"]
mod pending_read_tree_binding;
#[path = "read_tree_registry/pending_read_tree_root.rs"]
mod pending_read_tree_root;
#[path = "read_tree_registry/pending_read_tree_root_state.rs"]
mod pending_read_tree_root_state;

pub(crate) use pending_read_tree_binding::PendingReadTreeBinding;

/// Weakly indexes read-budget roots by operation ID.
#[derive(Debug, Default)]
pub(in crate::detector) struct ReadTreeRegistry {
    /// Root contexts retained only while an operation or descendant owns them.
    roots: Arc<Roots>,
    /// Ancestor associations retained weakly without consuming parent slots.
    ancestors: Arc<Roots>,
    /// Serializes first registration with the external parent resource scope.
    registrations: Arc<Mutex<()>>,
    /// Pending first bindings shared by sibling submissions before one body
    /// commits the root.
    pending_roots: Arc<PendingRoots>,
}

impl ReadTreeRegistry {
    /// Creates a child context using the immediate parent resource API.
    ///
    /// This compatibility path remains for internal consumers that already
    /// run after admission. New external-parent submissions should use
    /// [`reserve_child_for_parent`](Self::reserve_child_for_parent) and commit
    /// from their accepted body.
    ///
    /// # Parameters
    /// - `parent`: Active operation scope that owns the resource anchor.
    /// - `tree_limit`: Maximum bytes shared by the registered read-tree root.
    /// - `scope_limit`: Maximum bytes available to the returned child context.
    ///
    /// # Returns
    /// A child context that shares an existing parent tree or registers a new
    /// tree anchored to the parent scope.
    ///
    /// # Errors
    /// Returns the resource-scope error when the parent cannot accept the
    /// resource anchor.
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

    /// Borrows a child context when the parent already has a committed or
    /// pending read tree, without reserving another parent resource.
    ///
    /// # Parameters
    /// - `parent`: Operation whose existing read tree should be reused.
    /// - `scope_limit`: Maximum bytes available to the borrowed child context.
    ///
    /// # Returns
    /// A bounded child context when a committed or pending tree exists;
    /// otherwise, `None`.
    pub(in crate::detector) fn existing_child_for_parent(
        &self,
        parent: &OperationScope,
        scope_limit: u64,
    ) -> Option<Arc<ReadTreeContext>> {
        let _registration = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        let operation = parent.id();
        {
            let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(root) = roots.get(&operation).and_then(Weak::upgrade) {
                return Some(root.child(scope_limit));
            }
            roots.remove(&operation);
        }

        let pending_root = {
            let mut pending_roots = self.pending_roots.lock().unwrap_or_else(|e| e.into_inner());
            let pending_root = pending_roots.get(&operation).and_then(Weak::upgrade);
            if pending_root.is_none() {
                pending_roots.remove(&operation);
            }
            pending_root
        };
        if let Some(pending_root) = pending_root {
            let pending_state = pending_root.state.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(&*pending_state, PendingReadTreeRootState::Reserved(_)) {
                return Some(pending_root.root.child(scope_limit));
            }
        }
        let mut ancestors = self.ancestors.lock().unwrap_or_else(|e| e.into_inner());
        let root = ancestors.get(&operation).and_then(Weak::upgrade);
        if root.is_none() {
            ancestors.remove(&operation);
        }
        root.map(|root| root.child(scope_limit))
    }

    /// Associates a child with the parent's tree without reserving a parent
    /// resource slot. A later provider read promotes this association.
    ///
    /// # Parameters
    /// - `parent`: Parent operation whose read tree should own the child.
    /// - `tree_limit`: Maximum resources allowed in the associated tree.
    /// - `scope_limit`: Maximum resources allowed for this child scope.
    ///
    /// # Returns
    /// The child context associated with the parent's read tree.
    pub(in crate::detector) fn associate_child_for_parent(
        &self,
        parent: &OperationScope,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Arc<ReadTreeContext> {
        let _registration = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        let operation = parent.id();
        if let Some(root) = self
            .roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&operation)
            .and_then(Weak::upgrade)
        {
            return root.child(scope_limit);
        }
        if let Some(root) = self
            .pending_roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&operation)
            .and_then(Weak::upgrade)
        {
            let state = root.state.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(&*state, PendingReadTreeRootState::Reserved(_)) {
                let mut ancestors = self.ancestors.lock().unwrap_or_else(|e| e.into_inner());
                ancestors.insert(operation, Arc::downgrade(&root.root));
                root.root.register(Registration {
                    roots: Arc::downgrade(&self.ancestors),
                    operation,
                });
                return root.root.child(scope_limit);
            }
        }
        let mut ancestors = self.ancestors.lock().unwrap_or_else(|e| e.into_inner());
        let root = ancestors.get(&operation).and_then(Weak::upgrade).unwrap_or_else(|| {
            ancestors.remove(&operation);
            ReadTreeContext::new(tree_limit)
        });
        ancestors.insert(operation, Arc::downgrade(&root));
        root.register(Registration {
            roots: Arc::downgrade(&self.ancestors),
            operation,
        });
        root.child(scope_limit)
    }

    /// Reserves a parent binding whose anchor will be committed by an
    /// admitted child's body.
    ///
    /// # Parameters
    /// - `parent`: Active parent scope for which to reserve the resource slot.
    /// - `tree_limit`: Maximum bytes shared by the new read-tree root.
    /// - `scope_limit`: Maximum bytes available to the returned child context.
    ///
    /// # Returns
    /// A pending binding that must be committed after child admission.
    ///
    /// # Errors
    /// Returns the parent resource-capacity or lifecycle rejection.
    pub(in crate::detector) fn reserve_child_for_parent(
        &self,
        parent: &OperationScope,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Result<PendingReadTreeBinding, ResourceManageErrorKind> {
        let _registration = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        let operation = parent.id();
        {
            let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(root) = roots.get(&operation).and_then(Weak::upgrade) {
                return Ok(PendingReadTreeBinding {
                    binding: PendingBinding::Existing(root),
                    scope_limit,
                });
            }
            roots.remove(&operation);
        }

        {
            let mut pending_roots = self.pending_roots.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(pending_root) = pending_roots.get(&operation).and_then(Weak::upgrade) {
                return Ok(PendingReadTreeBinding {
                    binding: PendingBinding::First {
                        roots: Arc::clone(&self.roots),
                        pending_roots: Arc::clone(&self.pending_roots),
                        registrations: Arc::clone(&self.registrations),
                        operation,
                        pending_root,
                    },
                    scope_limit,
                });
            }
            pending_roots.remove(&operation);
        }

        let associated_root = {
            let mut ancestors = self.ancestors.lock().unwrap_or_else(|e| e.into_inner());
            let root = ancestors.get(&operation).and_then(Weak::upgrade);
            if root.is_none() {
                ancestors.remove(&operation);
            }
            root
        };

        let reservation = parent.reserve_resource().map_err(|error| match error {
            ResourceCapacityError::ScopeEnded => ResourceManageErrorKind::ScopeEnded,
            ResourceCapacityError::Stopped => ResourceManageErrorKind::Stopped,
            ResourceCapacityError::CapacityExceeded => ResourceManageErrorKind::CapacityExceeded,
        })?;
        let pending_root = Arc::new(PendingReadTreeRoot {
            root: associated_root.unwrap_or_else(|| ReadTreeContext::new(tree_limit)),
            state: Mutex::new(PendingReadTreeRootState::Reserved(Some(reservation))),
            pending_roots: Arc::downgrade(&self.pending_roots),
            operation,
        });
        self.pending_roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(operation, Arc::downgrade(&pending_root));
        Ok(PendingReadTreeBinding {
            binding: PendingBinding::First {
                roots: Arc::clone(&self.roots),
                pending_roots: Arc::clone(&self.pending_roots),
                registrations: Arc::clone(&self.registrations),
                operation,
                pending_root,
            },
            scope_limit,
        })
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
    use std::sync::Mutex;
    use std::sync::Weak;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::thread;

    use qubit_operation::GroupOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationId;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::OperationScope;
    use qubit_operation::ResourceManageErrorKind;
    use qubit_operation::StopReason;

    use super::PendingBinding;
    use super::ReadTreeContext;
    use super::ReadTreeRegistry;
    use super::pending_read_tree_root::PendingReadTreeRoot;
    use super::pending_read_tree_root::PendingRoots;
    use super::pending_read_tree_root::unregister_pending;
    use super::pending_read_tree_root_state::PendingReadTreeRootState;

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

    fn with_two_parent_scopes<T: Send + 'static>(
        body: impl FnOnce(&OperationManager, &OperationScope, &OperationScope) -> T + Send + 'static,
    ) -> T {
        let mut manager_options = ManagerOptions::default();
        manager_options.limits.active_operations = 4;
        manager_options.limits.queued_operations = 8;
        manager_options.async_executor.worker_count = 3;
        let manager = OperationManager::new(manager_options).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let (first_scope_tx, first_scope_rx) = std::sync::mpsc::channel();
        let (second_scope_tx, second_scope_rx) = std::sync::mpsc::channel();
        let (release_first, release_first_rx) = std::sync::mpsc::channel();
        let (release_second, release_second_rx) = std::sync::mpsc::channel();
        let mut first = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                first_scope_tx.send(scope).unwrap();
                let _ = release_first_rx.recv();
                Ok::<_, ()>(())
            })
            .unwrap();
        let mut second = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                second_scope_tx.send(scope).unwrap();
                let _ = release_second_rx.recv();
                Ok::<_, ()>(())
            })
            .unwrap();
        let first_scope = first_scope_rx.recv().unwrap();
        let second_scope = second_scope_rx.recv().unwrap();
        let result = body(&manager, &first_scope, &second_scope);
        release_first.send(()).unwrap();
        release_second.send(()).unwrap();
        assert!(matches!(
            block_on(first.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(matches!(
            block_on(second.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        block_on(first.wait_settlement());
        block_on(second.wait_settlement());
        result
    }

    fn bind_child(
        registry: &ReadTreeRegistry,
        parent: &OperationScope,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Arc<ReadTreeContext> {
        registry
            .reserve_child_for_parent(parent, tree_limit, scope_limit)
            .unwrap()
            .commit()
            .unwrap()
    }

    fn poison_mutex<T: Send>(mutex: &Mutex<T>) {
        thread::scope(|scope| {
            let poisoner = scope.spawn(|| {
                let _guard = mutex.lock().unwrap();
                panic!("poison read-tree recovery mutex");
            });
            assert!(poisoner.join().is_err());
        });
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

            let child = bind_child(&registry_body, scope, 100, 5);
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
            let first = bind_child(&registry_body, scope, 3, 3);
            assert_eq!(first.reserve(3).unwrap().commit(3), 3);
            let tree = Arc::clone(&first.tree);
            drop(first);
            assert!(registry_body.for_operation(scope.id()).is_some());

            let second = bind_child(&registry_body, scope, 6, 6);
            assert!(Arc::ptr_eq(&tree, &second.tree));
            assert_eq!(second.tree.consumed(), 3);
            assert!(second.reserve(1).is_none());
            scope.id()
        });
        assert!(registry.for_operation(operation).is_none());
    }

    #[test]
    fn test_uncommitted_parent_binding_leaves_no_root_and_releases_capacity() {
        let registry = Arc::new(ReadTreeRegistry::default());
        let registry_body = Arc::clone(&registry);
        let operation = with_parent_scope(move |scope| {
            let operation = scope.id();
            let pending = registry_body.reserve_child_for_parent(scope, 8, 4).unwrap();

            assert!(registry_body.for_operation(operation).is_none());
            drop(pending);
            assert!(registry_body.for_operation(operation).is_none());

            let capacity = scope.reserve_resource().expect("dropped ticket releases its slot");
            drop(capacity);
            operation
        });
        assert!(registry.for_operation(operation).is_none());
    }

    #[test]
    fn test_dropping_batch_bindings_for_distinct_parents_clears_pending_index() {
        let registry = Arc::new(ReadTreeRegistry::default());
        let registry_body = Arc::clone(&registry);
        let (distinct_ids, pending_count, pending_empty_after_drop, committed_empty, slots_returned) =
            with_two_parent_scopes(move |_, first, second| {
                let distinct_ids = first.id() != second.id();
                let first_binding = registry_body.reserve_child_for_parent(first, 8, 4).unwrap();
                let second_binding = registry_body.reserve_child_for_parent(second, 8, 4).unwrap();
                let pending_count = registry_body.pending_roots.lock().unwrap().len();
                let committed_empty = registry_body.roots.lock().unwrap().is_empty();

                drop((first_binding, second_binding));

                let pending_empty = registry_body.pending_roots.lock().unwrap().is_empty();
                let committed_empty_after_drop = registry_body.roots.lock().unwrap().is_empty();
                let first_slot_returned = first.reserve_resource().is_ok();
                let second_slot_returned = second.reserve_resource().is_ok();
                (
                    distinct_ids,
                    pending_count,
                    pending_empty,
                    committed_empty && committed_empty_after_drop,
                    first_slot_returned && second_slot_returned,
                )
            });
        assert!(distinct_ids);
        assert_eq!(2, pending_count);
        assert!(pending_empty_after_drop);
        assert!(committed_empty);
        assert!(slots_returned);
    }

    #[test]
    fn test_queued_cancellation_drops_pending_binding_and_returns_parent_slot() {
        let registry = Arc::new(ReadTreeRegistry::default());
        let registry_body = Arc::clone(&registry);
        let (pending_cleared, roots_empty, slot_returned, queued_stopped) =
            with_two_parent_scopes(move |manager, first, _| {
                let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
                let (entered_tx, entered_rx) = std::sync::mpsc::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let mut blocker = group
                    .as_async()
                    .try_submit(Default::default(), move |_| async move {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok::<_, ()>(())
                    })
                    .unwrap();
                entered_rx.recv().unwrap();

                let pending = registry_body.reserve_child_for_parent(first, 8, 4).unwrap();
                let mut queued = group
                    .as_async()
                    .try_submit(Default::default(), move |_| async move {
                        drop(pending);
                        Ok::<_, ()>(())
                    })
                    .unwrap();
                let was_queued = queued.observer().snapshot().execution == qubit_operation::ExecutionPhase::Queued;
                let _ = queued.request_stop(StopReason::UserCancelled);
                release_tx.send(()).unwrap();
                let queued_stopped = matches!(block_on(queued.wait_outcome()).unwrap(), OperationOutcome::Stopped(_));
                let settled = block_on(queued.wait_settlement()).all_released();
                let pending_cleared = registry_body.pending_roots.lock().unwrap().is_empty();
                let roots_empty = registry_body.roots.lock().unwrap().is_empty();
                let slot_returned = first.reserve_resource().is_ok();
                assert!(matches!(
                    block_on(blocker.wait_outcome()).unwrap(),
                    OperationOutcome::Succeeded(())
                ));
                assert!(block_on(blocker.wait_settlement()).all_released());
                (
                    pending_cleared && settled,
                    roots_empty,
                    slot_returned,
                    was_queued && queued_stopped,
                )
            });

        assert!(pending_cleared);
        assert!(roots_empty);
        assert!(slot_returned);
        assert!(queued_stopped);
    }

    #[test]
    fn test_pending_root_removal_preserves_later_replacement() {
        let operation = operation_id();
        let pending_roots: PendingRoots = Mutex::new(std::collections::HashMap::new());
        let pending_roots_weak = Arc::new(pending_roots);
        let old = Arc::new(PendingReadTreeRoot {
            root: ReadTreeContext::new(8),
            state: Mutex::new(PendingReadTreeRootState::Detached),
            pending_roots: Arc::downgrade(&pending_roots_weak),
            operation,
        });
        let replacement = Arc::new(PendingReadTreeRoot {
            root: ReadTreeContext::new(16),
            state: Mutex::new(PendingReadTreeRootState::Detached),
            pending_roots: Arc::downgrade(&pending_roots_weak),
            operation,
        });
        pending_roots_weak
            .lock()
            .unwrap()
            .insert(operation, Arc::downgrade(&replacement));

        unregister_pending(&pending_roots_weak, operation, &old);

        let registered = pending_roots_weak
            .lock()
            .unwrap()
            .get(&operation)
            .and_then(Weak::upgrade)
            .expect("replacement pending root remains indexed");
        assert!(Arc::ptr_eq(&registered, &replacement));
    }

    #[test]
    fn test_provider_binding_promotes_unanchored_ancestor_association() {
        let registry = Arc::new(ReadTreeRegistry::default());
        let registry_body = Arc::clone(&registry);
        with_parent_scope(move |scope| {
            let mut occupied = Vec::new();
            while let Ok(reservation) = scope.reserve_resource() {
                occupied.push(reservation);
            }
            assert!(!occupied.is_empty(), "the parent should expose resource capacity");

            let intermediate = registry_body.associate_child_for_parent(scope, 8, 8);
            assert!(matches!(
                registry_body.reserve_child_for_parent(scope, 16, 16),
                Err(ResourceManageErrorKind::CapacityExceeded)
            ));
            drop(occupied);
            let direct_provider = registry_body
                .reserve_child_for_parent(scope, 16, 16)
                .expect("provider read can reserve the ancestor resource slot")
                .commit()
                .expect("provider read promotes the ancestor association");
            assert!(Arc::ptr_eq(&intermediate.tree, &direct_provider.tree));

            assert_eq!(6, intermediate.reserve(6).unwrap().commit(6));
            assert_eq!(2, direct_provider.reserve(2).unwrap().commit(2));
            assert!(direct_provider.reserve(1).is_none());
        });
    }

    #[test]
    fn test_concurrent_parent_binding_tickets_reuse_first_committed_root() {
        let registry = Arc::new(ReadTreeRegistry::default());
        let registry_body = Arc::clone(&registry);
        let operation = with_parent_scope(move |scope| {
            let operation = scope.id();
            let first = registry_body.reserve_child_for_parent(scope, 8, 4).unwrap();
            let second = registry_body.reserve_child_for_parent(scope, 16, 6).unwrap();
            assert!(registry_body.for_operation(operation).is_none());

            let first = first.commit().expect("first accepted child commits its root");
            let second = second.commit().expect("later accepted child reuses the root");
            assert!(Arc::ptr_eq(&first.tree, &second.tree));
            assert_eq!(first.remaining(), 4);
            assert_eq!(second.remaining(), 6);
            operation
        });
        assert!(registry.for_operation(operation).is_none());
    }

    #[test]
    fn test_existing_parent_tree_is_borrowed_without_another_resource_slot() {
        let mut manager_options = ManagerOptions::default();
        manager_options.scope_limits.resources = 1;
        let manager = OperationManager::new(manager_options).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let registry = Arc::new(ReadTreeRegistry::default());
        let body_registry = Arc::clone(&registry);
        let mut parent = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                let first = bind_child(&body_registry, &scope, 8, 8);
                assert_eq!(3, first.reserve(3).unwrap().commit(3));

                let borrowed = body_registry
                    .existing_child_for_parent(&scope, 5)
                    .expect("the parent's registered read tree should be borrowed");
                assert!(Arc::ptr_eq(&first.tree, &borrowed.tree));
                assert_eq!(3, borrowed.tree.consumed());
                assert_eq!(5, borrowed.remaining());
                assert_eq!(5, borrowed.reserve(5).unwrap().commit(5));
                assert_eq!(8, borrowed.tree.consumed());

                assert!(matches!(
                    scope.reserve_resource(),
                    Err(qubit_operation::ResourceCapacityError::CapacityExceeded)
                ));
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            block_on(parent.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let _settlement = block_on(parent.wait_settlement());
    }

    #[test]
    fn test_existing_child_borrows_pending_root_and_siblings_recover_poisoned_locks() {
        let mut manager_options = ManagerOptions::default();
        manager_options.scope_limits.resources = 1;
        let manager = OperationManager::new(manager_options).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let registry = Arc::new(ReadTreeRegistry::default());
        let body_registry = Arc::clone(&registry);
        let mut parent = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                let first = body_registry
                    .reserve_child_for_parent(&scope, 8, 8)
                    .expect("the first child reserves the parent tree");
                let pending_root = match &first.binding {
                    PendingBinding::First { pending_root, .. } => Arc::clone(pending_root),
                    PendingBinding::Existing(_) => {
                        panic!("the first child must own the pending root")
                    }
                };

                poison_mutex(&body_registry.registrations);
                poison_mutex(&body_registry.roots);
                poison_mutex(&body_registry.pending_roots);
                poison_mutex(&pending_root.state);

                let borrowed = body_registry
                    .existing_child_for_parent(&scope, 6)
                    .expect("a reserved tree is borrowable before its first commit");
                let second = body_registry
                    .reserve_child_for_parent(&scope, 16, 4)
                    .expect("an accepted sibling reuses the first pending root");

                assert_eq!(3, borrowed.reserve(3).unwrap().commit(3));
                let first = first.commit().expect("the first sibling commits the root");
                let second = second.commit().expect("the later sibling reuses the root");
                assert!(Arc::ptr_eq(&borrowed.tree, &first.tree));
                assert!(Arc::ptr_eq(&first.tree, &second.tree));
                assert_eq!(3, second.tree.consumed());
                assert!(matches!(
                    scope.reserve_resource(),
                    Err(qubit_operation::ResourceCapacityError::CapacityExceeded)
                ));
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            block_on(parent.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let _settlement = block_on(parent.wait_settlement());
        drop(registry);
    }

    #[test]
    fn test_child_for_parent_maps_stopped_scope_without_registering_a_tree() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let registry = Arc::new(ReadTreeRegistry::default());
        let body_registry = Arc::clone(&registry);
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let mut parent = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                assert_eq!(
                    qubit_operation::StopRequestStatus::Accepted,
                    scope.request_stop(StopReason::UserCancelled)
                );
                let result = body_registry.child_for_parent(&scope, 8, 4);
                result_tx.send((scope.id(), result.err())).unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();

        let (operation, error) = result_rx.recv().unwrap();
        assert_eq!(Some(ResourceManageErrorKind::Stopped), error);
        assert!(registry.for_operation(operation).is_none());
        let _outcome = block_on(parent.wait_outcome()).unwrap();
        let _settlement = block_on(parent.wait_settlement());
    }

    #[test]
    fn test_accepted_sibling_bindings_share_tree_after_parent_body_returns() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let registry = Arc::new(ReadTreeRegistry::default());
        let body_registry = Arc::clone(&registry);
        let (pending_tx, pending_rx) = std::sync::mpsc::channel();
        let mut parent = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                let first = body_registry.reserve_child_for_parent(&scope, 8, 4).unwrap();
                let second = body_registry.reserve_child_for_parent(&scope, 8, 4).unwrap();
                pending_tx.send((first, second)).unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
        let (first, second) = pending_rx.recv().unwrap();
        assert!(matches!(
            block_on(parent.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));

        poison_mutex(&registry.pending_roots);
        let first = first
            .commit()
            .expect("accepted child retains its reserved parent tree after body return");
        let second = second
            .commit()
            .expect("accepted sibling reuses the same tree after parent body return");
        assert!(Arc::ptr_eq(&first.tree, &second.tree));
        assert_eq!(first.reserve(4).unwrap().commit(4), 4);
        assert_eq!(second.reserve(4).unwrap().commit(4), 4);
        assert_eq!(8, first.tree.consumed());

        drop((first, second));
        let _settlement = block_on(parent.wait_settlement());
    }

    #[test]
    fn test_accepted_binding_commit_returns_parent_stop_reason() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let registry = Arc::new(ReadTreeRegistry::default());
        let body_registry = Arc::clone(&registry);
        let (reason_tx, reason_rx) = std::sync::mpsc::channel();
        let mut parent = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let pending = body_registry.reserve_child_for_parent(&scope, 8, 4).unwrap();
            assert_eq!(
                qubit_operation::StopRequestStatus::Accepted,
                scope.request_stop(StopReason::UserCancelled)
            );
            poison_mutex(&body_registry.pending_roots);
            let kind = pending.commit().expect_err("a stopped accepted parent rejects commit");
            assert!(body_registry.for_operation(scope.id()).is_none());
            reason_tx.send(kind).unwrap();
            Ok::<_, ()>(())
        }))
        .unwrap();

        assert_eq!(ResourceManageErrorKind::Stopped, reason_rx.recv().unwrap());
        let _outcome = block_on(parent.wait_outcome()).unwrap();
        let _settlement = block_on(parent.wait_settlement());
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

            let child = bind_child(&registry_body, scope, 8, 4);
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
