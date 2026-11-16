// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use super::Amount;
use super::BudgetPermit;
use super::Counters;
use super::Limits;
use super::PermitInner;

/// A finite budget shared by cloned execution services.
#[derive(Debug)]
pub(in crate::detector) struct MimeBudget {
    pub(super) limits: Limits,
    pub(super) counters: Mutex<Counters>,
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
