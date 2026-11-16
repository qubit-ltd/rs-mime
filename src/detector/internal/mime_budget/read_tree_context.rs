// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

use super::ReadPermit;
use super::ReadScopeBudget;
use super::ReadTreeBudget;
use super::ScopeCounters;

/// Read accounting context inherited by MIME descendants.
#[derive(Debug)]
pub(crate) struct ReadTreeContext {
    pub(super) tree: Arc<ReadTreeBudget>,
    pub(super) scope: Arc<ReadScopeBudget>,
    /// Keeps an external-parent anchor alive while a child operation is held.
    #[allow(dead_code)]
    parent: Option<Arc<ReadTreeContext>>,
}

impl ReadTreeContext {
    pub(crate) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            tree: ReadTreeBudget::new(limit),
            scope: Arc::new(ReadScopeBudget {
                limit,
                counters: Mutex::new(ScopeCounters::default()),
                parent: None,
            }),
            parent: None,
        })
    }

    pub(in crate::detector) fn child(self: &Arc<Self>, limit: u64) -> Arc<Self> {
        Arc::new(Self {
            tree: Arc::clone(&self.tree),
            scope: Arc::new(ReadScopeBudget {
                limit: limit.min(self.scope.limit),
                counters: Mutex::new(ScopeCounters::default()),
                parent: Some(Arc::clone(&self.scope)),
            }),
            parent: Some(Arc::clone(self)),
        })
    }

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
                    reserved
                        .counters
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .reserved -= bytes;
                }
                self.tree
                    .counters
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .reserved -= bytes;
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
        self.scope
            .counters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .consumed
    }
}
