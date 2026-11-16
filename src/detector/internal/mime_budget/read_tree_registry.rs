// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use qubit_operation::OperationId;

use super::ReadTreeContext;

#[derive(Debug, Default)]
pub(in crate::detector) struct ReadTreeRegistry {
    roots: Mutex<HashMap<OperationId, Weak<ReadTreeContext>>>,
}

impl ReadTreeRegistry {
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

    pub(in crate::detector) fn bind(&self, id: OperationId, tree: &Arc<ReadTreeContext>) {
        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Arc::downgrade(tree));
    }

    pub(crate) fn for_operation(&self, id: OperationId) -> Option<Arc<ReadTreeContext>> {
        self.roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .and_then(Weak::upgrade)
    }
}
