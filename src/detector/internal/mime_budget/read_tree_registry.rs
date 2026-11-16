// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use qubit_operation::OperationId;

use super::ReadTreeContext;

/// Weakly indexes read-budget roots by operation ID.
#[derive(Debug, Default)]
pub(in crate::detector) struct ReadTreeRegistry {
    /// Root contexts retained only while an operation or descendant owns them.
    roots: Mutex<HashMap<OperationId, Weak<ReadTreeContext>>>,
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
        let mut roots = self.roots.lock().unwrap_or_else(|e| e.into_inner());
        roots.retain(|_, tree| tree.strong_count() > 0);
        let parent = roots.get(&id).and_then(Weak::upgrade).unwrap_or_else(|| {
            let context = ReadTreeContext::new(tree_limit);
            roots.insert(id, Arc::downgrade(&context));
            context
        });
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
