// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared bounded MIME-owned memory and staging counters.

#[path = "mime_budget/counters.rs"]
mod counters;
use counters::Counters;
#[path = "mime_budget/mime_budget.rs"]
mod budget;
pub(in crate::detector) use budget::MimeBudget;
#[path = "mime_budget/staging_budget.rs"]
mod staging_budget;
pub(crate) use staging_budget::StagingBudget;
#[path = "mime_budget/staging_permit.rs"]
mod staging_permit;
pub(crate) use staging_permit::StagingPermit;
#[path = "mime_budget/owned_memory_permit.rs"]
mod owned_memory_permit;
pub(crate) use owned_memory_permit::OwnedMemoryPermit;
#[path = "mime_budget/read_counters.rs"]
mod read_counters;
use read_counters::ReadCounters;
#[path = "mime_budget/scope_counters.rs"]
mod scope_counters;
use scope_counters::ScopeCounters;
#[path = "mime_budget/read_scope_budget.rs"]
mod read_scope_budget;
use read_scope_budget::ReadScopeBudget;
#[path = "mime_budget/read_tree_budget.rs"]
mod read_tree_budget;
pub(in crate::detector) use read_tree_budget::ReadTreeBudget;
#[path = "mime_budget/read_tree_context.rs"]
mod read_tree_context;
pub(crate) use read_tree_context::ReadTreeContext;
#[path = "mime_budget/read_permit.rs"]
mod read_permit;
pub(crate) use read_permit::ReadPermit;
#[path = "mime_budget/read_tree_registry.rs"]
mod read_tree_registry;
pub(in crate::detector) use read_tree_registry::ReadTreeRegistry;
#[path = "mime_budget/limits.rs"]
mod limits;
pub(in crate::detector) use limits::Limits;
#[path = "mime_budget/amount.rs"]
mod amount;
use amount::Amount;
#[path = "mime_budget/permit_inner.rs"]
mod permit_inner;
use permit_inner::PermitInner;
#[path = "mime_budget/budget_permit.rs"]
mod budget_permit;
pub(in crate::detector) use budget_permit::BudgetPermit;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::Limits;
    use super::MimeBudget;
    use super::ReadTreeContext;

    #[test]
    fn test_refinement_shares_owned_memory_and_staging_budgets() {
        let budget = MimeBudget::new(Limits { owned: 16, staging: 8 });
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
    fn test_mime_budget_overflow_preserves_counters_and_exact_limits_release() {
        let budget = MimeBudget::new(Limits {
            owned: usize::MAX,
            staging: u64::MAX,
        });
        let owned = budget.reserve_owned(usize::MAX).unwrap();
        assert_eq!(budget.owned_bytes(), usize::MAX);
        assert!(budget.reserve_owned(1).is_none());
        assert_eq!(budget.owned_bytes(), usize::MAX);
        drop(owned);
        assert_eq!(budget.owned_bytes(), 0);

        let staging = budget.reserve_staging(u64::MAX).unwrap();
        assert_eq!(budget.staging_bytes(), u64::MAX);
        assert!(budget.reserve_staging(1).is_none());
        assert_eq!(budget.staging_bytes(), u64::MAX);
        drop(staging);
        assert_eq!(budget.staging_bytes(), 0);
    }

    #[test]
    fn test_mime_budget_alias_permits_share_reservation_until_last_drop() {
        let budget = MimeBudget::new(Limits { owned: 8, staging: 0 });
        let allocation: Arc<[u8]> = Arc::from([7u8; 8]);
        let alias = Arc::clone(&allocation);
        let permit = budget.reserve_allocation(&allocation).unwrap();
        let alias_permit = budget.reserve_allocation(&alias).unwrap();

        assert_eq!(budget.owned_bytes(), 8);
        drop(permit);
        assert_eq!(budget.owned_bytes(), 8);
        assert!(budget.reserve_owned(1).is_none());
        drop(alias_permit);
        assert_eq!(budget.owned_bytes(), 0);
        assert!(budget.reserve_owned(8).is_some());
    }

    #[test]
    fn test_read_tree_counts_committed_bytes_and_releases_rolled_back_reservations() {
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

    #[test]
    fn test_read_permit_enforces_exact_boundaries_and_releases_unused_capacity() {
        let root = ReadTreeContext::new(u64::MAX);
        let child = root.child(5);
        let permit = child.reserve(5).unwrap();
        assert_eq!(child.remaining(), 0);
        assert!(child.reserve(1).is_none());

        assert_eq!(permit.commit(2), 2);
        assert_eq!(root.tree.consumed(), 2);
        assert_eq!(child.consumed(), 2);
        assert_eq!(child.remaining(), 3);
        assert_eq!(child.reserve(3).unwrap().commit(3), 3);
        assert_eq!(child.remaining(), 0);
        assert_eq!(root.tree.consumed(), 5);
    }

    #[test]
    fn test_read_reservation_rollback_restores_tree_and_ancestor_capacity() {
        let root = ReadTreeContext::new(6);
        let child = root.child(4);
        {
            let pending = child.reserve(4).unwrap();
            assert_eq!(child.remaining(), 0);
            assert_eq!(root.tree.consumed(), 0);
            drop(pending);
        }
        assert_eq!(child.remaining(), 4);
        assert_eq!(child.reserve(4).unwrap().commit(4), 4);
        assert_eq!(child.remaining(), 0);
        assert_eq!(root.tree.consumed(), 4);
        assert_eq!(root.remaining(), 2);
    }
}
