// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Accounts provider-prefix bytes and settles the associated read reservation.

use std::ops::ControlFlow;

use qubit_fs::read::ReadIoObserver;
use qubit_fs::read::ReadIoPhase;
use qubit_operation::OperationContext;

use crate::detector::internal::mime_budget::ReadPermit;

/// Settles a bounded provider-prefix reservation with bytes delivered so far.
///
/// The guard commits its observed byte count when dropped, including when the
/// enclosing detection future is cancelled while a provider read is pending.
pub(super) struct PrefixReadAccounting {
    /// Provisional read reservation retained until settlement.
    permit: Option<ReadPermit>,
    /// Bytes reported by successful provider reads.
    bytes_read: u64,
    /// Context checked before each provider open or read.
    operation_context: OperationContext,
}

impl PrefixReadAccounting {
    /// Creates an accounting guard for one bounded prefix request.
    ///
    /// # Parameters
    /// - `permit`: Reservation that will be settled with the observed byte
    ///   count.
    pub(super) fn new(permit: ReadPermit, operation_context: OperationContext) -> Self {
        Self {
            permit: Some(permit),
            bytes_read: 0,
            operation_context,
        }
    }

    /// Commits the observed amount and disables cancellation settlement.
    ///
    /// # Returns
    /// The number of bytes committed to the read budget.
    pub(super) fn commit(mut self) -> u64 {
        self.permit
            .take()
            .expect("prefix read permit commits once")
            .commit(self.bytes_read)
    }
}

impl ReadIoObserver for PrefixReadAccounting {
    /// Stops before another provider call when the operation was stopped or
    /// expired.
    ///
    /// # Parameters
    /// - `_phase`: Provider I/O phase about to begin.
    fn before_io(&mut self, _phase: ReadIoPhase) -> ControlFlow<()> {
        if self.operation_context.stop_reason().is_some() || self.operation_context.is_expired() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    /// Records bytes reported by a successful provider read.
    ///
    /// # Parameters
    /// - `bytes`: Number of bytes reported by the provider.
    fn on_read_bytes(&mut self, bytes: usize) {
        let bytes = u64::try_from(bytes).expect("provider read byte count fits in u64");
        self.bytes_read = self
            .bytes_read
            .checked_add(bytes)
            .expect("bounded prefix byte count does not overflow");
    }
}

impl Drop for PrefixReadAccounting {
    /// Settles the reservation when it was not explicitly committed.
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            permit.commit(self.bytes_read);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::task::Waker;

    use qubit_fs::read::ReadIoObserver;
    use qubit_fs::read::ReadIoPhase;
    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationContext;
    use qubit_operation::StopReason;
    use qubit_operation::TimerRegistration;

    use super::PrefixReadAccounting;
    use crate::detector::internal::mime_budget::ReadTreeContext;

    struct TestClock {
        domain: ClockDomainId,
        ticks: AtomicU64,
    }

    impl ClockSource for TestClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.domain, self.ticks.load(Ordering::Acquire))
        }

        fn register_deadline(&self, _at: MonotonicTime, _wake: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoopRegistration)
        }
    }

    struct NoopRegistration;

    impl TimerRegistration for NoopRegistration {
        fn cancel(self: Box<Self>) {}
    }

    #[test]
    fn test_stop_after_successful_prefix_read_commits_bytes_and_blocks_next_io() {
        let tree = ReadTreeContext::new(8);
        let context = OperationContext::new(ContextOptions::default()).unwrap();
        let mut accounting = PrefixReadAccounting::new(tree.reserve(8).unwrap(), context.clone());

        assert!(accounting.before_io(ReadIoPhase::Open).is_continue());
        accounting.on_read_bytes(2);
        assert!(context.request_stop(StopReason::UserCancelled));
        assert!(accounting.before_io(ReadIoPhase::Read).is_break());
        drop(accounting);

        assert_eq!(2, tree.consumed());
        assert_eq!(6, tree.remaining());
    }

    #[test]
    fn test_deadline_after_successful_prefix_read_commits_bytes_and_blocks_next_io() {
        let domain = ClockDomainId::new_unique();
        let clock = Arc::new(TestClock {
            domain,
            ticks: AtomicU64::new(0),
        });
        let context = OperationContext::new(ContextOptions {
            clock: Some(clock.clone()),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 5))),
            ..Default::default()
        })
        .unwrap();
        let tree = ReadTreeContext::new(8);
        let mut accounting = PrefixReadAccounting::new(tree.reserve(8).unwrap(), context);

        assert!(accounting.before_io(ReadIoPhase::Open).is_continue());
        accounting.on_read_bytes(3);
        clock.ticks.store(5, Ordering::Release);
        assert!(accounting.before_io(ReadIoPhase::Read).is_break());
        drop(accounting);

        assert_eq!(3, tree.consumed());
        assert_eq!(5, tree.remaining());
    }
}
