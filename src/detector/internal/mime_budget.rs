//! Shared bounded MIME-owned memory and staging counters.

#[path = "mime_budget/counters.rs"]
mod counters;
use counters::Counters;
#[path = "mime_budget/mime_budget.rs"]
mod mime_budget;
pub(in crate::detector) use mime_budget::MimeBudget;
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
