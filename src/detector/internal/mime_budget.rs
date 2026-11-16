//! Shared bounded MIME-owned memory and staging counters.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

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
}
