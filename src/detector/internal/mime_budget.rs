//! Shared bounded MIME-owned memory and staging counters.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use qubit_operation::OperationId;

#[derive(Debug, Default)]
struct Counters {
    owned: usize,
    staging: u64,
    allocations: HashMap<usize, (Weak<[u8]>, Weak<PermitInner>)>,
}

/// A finite budget shared by cloned execution services.
#[derive(Debug)]
pub(in crate::detector) struct MimeBudget {
    limits: Limits,
    counters: Mutex<Counters>,
}

#[derive(Debug, Default)]
struct ReadCounters {
    consumed: u64,
    reserved: u64,
}

#[derive(Debug, Default)]
struct ScopeCounters {
    consumed: u64,
    reserved: u64,
}

#[derive(Debug)]
struct ReadScopeBudget {
    limit: u64,
    counters: Mutex<ScopeCounters>,
    parent: Option<Arc<ReadScopeBudget>>,
}

/// Cumulative read allowance shared by one root request and its descendants.
#[derive(Debug)]
pub(in crate::detector) struct ReadTreeBudget {
    limit: u64,
    counters: Mutex<ReadCounters>,
}

/// Read accounting context inherited by MIME descendants.
#[derive(Debug)]
pub(in crate::detector) struct ReadTreeContext {
    tree: Arc<ReadTreeBudget>,
    scope: Arc<ReadScopeBudget>,
    /// Keeps an external-parent anchor alive while a child operation is held.
    #[allow(dead_code)]
    parent: Option<Arc<ReadTreeContext>>,
}

impl ReadTreeContext {
    pub(in crate::detector) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            tree: ReadTreeBudget::new(limit),
            scope: Arc::new(ReadScopeBudget { limit, counters: Mutex::new(ScopeCounters::default()), parent: None }),
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

    pub(in crate::detector) fn reserve(&self, bytes: u64) -> Option<ReadPermit> {
        let mut tree = self.tree.counters.lock().unwrap_or_else(|e| e.into_inner());
        if bytes > self.tree.limit.saturating_sub(tree.consumed.saturating_add(tree.reserved)) {
            return None;
        }
        tree.reserved += bytes;
        drop(tree);

        let mut scopes: Vec<Arc<ReadScopeBudget>> = Vec::new();
        let mut scope = Some(Arc::clone(&self.scope));
        while let Some(current) = scope {
            let mut counters = current.counters.lock().unwrap_or_else(|e| e.into_inner());
            if bytes > current.limit.saturating_sub(counters.consumed.saturating_add(counters.reserved)) {
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
        Some(ReadPermit { tree: Arc::clone(&self.tree), scopes, reserved: bytes, committed: false })
    }

    pub(in crate::detector) fn remaining(&self) -> u64 {
        let tree = self.tree.counters.lock().unwrap_or_else(|e| e.into_inner());
        let mut remaining = self.tree.limit.saturating_sub(tree.consumed.saturating_add(tree.reserved));
        drop(tree);
        let mut scope = Some(Arc::clone(&self.scope));
        while let Some(current) = scope {
            let counters = current.counters.lock().unwrap_or_else(|e| e.into_inner());
            remaining = remaining.min(current.limit.saturating_sub(counters.consumed.saturating_add(counters.reserved)));
            drop(counters);
            scope = current.parent.clone();
        }
        remaining
    }
}

impl ReadTreeBudget {
    pub(in crate::detector) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self { limit, counters: Mutex::new(ReadCounters::default()) })
    }

    /// Reserves requested read capacity before provider I/O begins.
    #[allow(dead_code)]
    pub(in crate::detector) fn consumed(&self) -> u64 {
        self.counters.lock().unwrap_or_else(|e| e.into_inner()).consumed
    }
}

pub(in crate::detector) struct ReadPermit {
    tree: Arc<ReadTreeBudget>,
    scopes: Vec<Arc<ReadScopeBudget>>,
    reserved: u64,
    committed: bool,
}

impl ReadPermit {
    /// Commits the bytes actually returned and releases unused capacity.
    pub(in crate::detector) fn commit(mut self, actual: u64) -> u64 {
        assert!(actual <= self.reserved, "provider returned more bytes than reserved");
        let mut counters = self.tree.counters.lock().unwrap_or_else(|e| e.into_inner());
        counters.reserved -= self.reserved;
        counters.consumed += actual;
        for scope in &self.scopes {
            let mut counters = scope.counters.lock().unwrap_or_else(|e| e.into_inner());
            counters.reserved -= self.reserved;
            counters.consumed += actual;
        }
        self.committed = true;
        actual
    }
}

impl Drop for ReadPermit {
    fn drop(&mut self) {
        if !self.committed {
            self.tree.counters.lock().unwrap_or_else(|e| e.into_inner()).reserved -= self.reserved;
            for scope in &self.scopes {
                scope.counters.lock().unwrap_or_else(|e| e.into_inner()).reserved -= self.reserved;
            }
        }
    }
}

#[derive(Debug, Default)]
pub(in crate::detector) struct ReadTreeRegistry {
    roots: Mutex<HashMap<OperationId, Weak<ReadTreeContext>>>,
}

impl ReadTreeRegistry {
    pub(in crate::detector) fn child_for_parent(&self, id: OperationId, tree_limit: u64, scope_limit: u64) -> Arc<ReadTreeContext> {
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
        self.roots.lock().unwrap_or_else(|e| e.into_inner()).insert(id, Arc::downgrade(tree));
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::detector) struct Limits {
    pub owned: usize,
    pub staging: u64,
}

impl MimeBudget {
    pub(in crate::detector) fn new(limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            counters: Mutex::new(Counters::default()),
        })
    }

    pub(in crate::detector) fn reserve_owned(
        self: &Arc<Self>,
        bytes: usize,
    ) -> Option<BudgetPermit> {
        self.reserve(Amount::Owned(bytes))
    }

    /// Counts an Arc allocation once while any request retains that allocation.
    pub(in crate::detector) fn reserve_allocation(
        self: &Arc<Self>,
        allocation: &Arc<[u8]>,
    ) -> Option<BudgetPermit> {
        let identity = Arc::as_ptr(allocation) as *const u8 as usize;
        let mut counters = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((registered, permit)) = counters.allocations.get(&identity) {
            if Weak::ptr_eq(registered, &Arc::downgrade(allocation)) {
                if let Some(permit) = permit.upgrade() {
                    return Some(BudgetPermit { _inner: permit });
                }
            }
        }
        counters.allocations.remove(&identity);
        let next = counters.owned.checked_add(allocation.len())?;
        if next > self.limits.owned {
            return None;
        }
        counters.owned = next;
        let inner = Arc::new(PermitInner {
            budget: Arc::clone(self),
            amount: Amount::Owned(allocation.len()),
        });
        counters.allocations.insert(
            identity,
            (Arc::downgrade(allocation), Arc::downgrade(&inner)),
        );
        Some(BudgetPermit { _inner: inner })
    }

    #[allow(dead_code)]
    pub(in crate::detector) fn reserve_staging(
        self: &Arc<Self>,
        bytes: u64,
    ) -> Option<BudgetPermit> {
        self.reserve(Amount::Staging(bytes))
    }

    fn reserve(self: &Arc<Self>, amount: Amount) -> Option<BudgetPermit> {
        let mut counters = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        match amount {
            Amount::Owned(bytes) => {
                let next = counters.owned.checked_add(bytes)?;
                if next > self.limits.owned {
                    return None;
                }
                counters.owned = next;
            }
            Amount::Staging(bytes) => {
                let next = counters.staging.checked_add(bytes)?;
                if next > self.limits.staging {
                    return None;
                }
                counters.staging = next;
            }
        }
        Some(BudgetPermit {
            _inner: Arc::new(PermitInner {
                budget: Arc::clone(self),
                amount,
            }),
        })
    }

    #[allow(dead_code)]
    pub(in crate::detector) fn owned_bytes(&self) -> usize {
        self.counters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .owned
    }

    #[allow(dead_code)]
    pub(in crate::detector) fn staging_bytes(&self) -> u64 {
        self.counters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .staging
    }
}

enum Amount {
    Owned(usize),
    Staging(u64),
}
struct PermitInner {
    budget: Arc<MimeBudget>,
    amount: Amount,
}

/// Cloneable reservation whose count returns only after its final owner drops.
pub(in crate::detector) struct BudgetPermit {
    _inner: Arc<PermitInner>,
}

impl Drop for PermitInner {
    fn drop(&mut self) {
        let mut counters = self
            .budget
            .counters
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match self.amount {
            Amount::Owned(bytes) => counters.owned -= bytes,
            Amount::Staging(bytes) => counters.staging -= bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_runtime_budget_shared_across_refinement() {
        let budget = MimeBudget::new(Limits {
            owned: 16,
            staging: 8,
        });
        let first: Arc<[u8]> = Arc::from([1u8; 8]);
        let alias = Arc::clone(&first);
        let permit_a = budget.reserve_allocation(&first).unwrap();
        let permit_b = budget.reserve_allocation(&alias).unwrap();
        assert_eq!(budget.owned_bytes(), 8);
        let copy: Arc<[u8]> = Arc::from(first.as_ref());
        let permit_copy = budget.reserve_allocation(&copy).unwrap();
        assert_eq!(budget.owned_bytes(), 16);
        drop((permit_a, permit_b));
        assert_eq!(budget.owned_bytes(), 8);
        drop(permit_copy);
        assert_eq!(budget.owned_bytes(), 0);
        assert!(budget.reserve_staging(9).is_none());
        assert_eq!(budget.staging_bytes(), 0);
    }

    #[test]
    fn operation_runtime_read_tree_counts_actual_bytes_and_rolls_back_reservations() {
        let root = ReadTreeContext::new(6);
        let tighter_child = root.child(3);
        let sibling = root.child(6);
        let first = tighter_child.reserve(2).unwrap();
        assert!(tighter_child.reserve(2).is_none());
        first.commit(2);
        assert_eq!(root.tree.consumed(), 2);
        let sibling_read = sibling.reserve(3).unwrap();
        sibling_read.commit(3);
        assert_eq!(root.tree.consumed(), 5);
        assert!(sibling.reserve(2).is_none());
        {
            let temporary = sibling.reserve(1).unwrap();
            assert!(sibling.reserve(1).is_none());
            drop(temporary);
        }
        assert!(sibling.reserve(1).is_some());

        let independent = ReadTreeContext::new(4);
        assert_eq!(independent.reserve(4).unwrap().commit(4), 4);
        assert_eq!(root.tree.consumed(), 5);
    }
}
