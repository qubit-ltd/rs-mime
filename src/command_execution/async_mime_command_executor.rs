// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Asynchronous command boundary driven through operation's public adapter.

use qubit_operation::OperationScope;
use qubit_operation::ResourceRef;

use super::ManagedMimeCommand;
use super::MimeCommandOutput;
use crate::MimeFuture;
use crate::managed_temp_staging::StagedMimeInput;

/// Async command adapter. The command remains an operation child, so typed
/// outcome and worker input release are observed independently.
pub(crate) trait AsyncMimeCommandExecutor: Send + Sync {
    /// Executes a staged command as a managed child of the caller's operation.
    ///
    /// # Parameters
    /// - `scope`: Parent scope that supplies operation and cancellation
    ///   context.
    /// - `command`: Runner, command, and runtime group for this child.
    /// - `input`: Staged input retained until the command worker returns.
    ///
    /// # Returns
    /// The complete command output or a typed execution error.
    ///
    /// # Errors
    /// Returns a typed error when operation admission, command execution,
    /// cancellation, or settlement fails.
    fn run_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        command: ManagedMimeCommand,
        input: ResourceRef<StagedMimeInput>,
    ) -> MimeFuture<'a, MimeCommandOutput>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use crate::command_execution::internal::PollSignal;

    struct CountWake(AtomicUsize);
    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_poll_signal_wakes_waiters_without_lost_notifications() {
        for _ in 0..512 {
            let signal = Arc::new(PollSignal::new());
            let gate = Arc::new(Barrier::new(2));
            let to_publish = Arc::clone(&signal);
            let publisher_gate = Arc::clone(&gate);
            let publisher = std::thread::spawn(move || {
                publisher_gate.wait();
                to_publish.publish(7_u8);
            });
            let wake = Arc::new(CountWake(AtomicUsize::new(0)));
            let waker = Waker::from(Arc::clone(&wake));
            let mut context = Context::from_waker(&waker);
            gate.wait();
            let first = signal.poll_clone(&mut context);
            publisher.join().unwrap();
            match first {
                Poll::Ready(7) => {}
                Poll::Pending => {
                    assert!(
                        wake.0.load(Ordering::SeqCst) > 0,
                        "publisher must wake a registered poller"
                    );
                    assert!(matches!(signal.poll_clone(&mut context), Poll::Ready(7)));
                }
                other => panic!("unexpected signal result: {other:?}"),
            }
        }
    }

    #[test]
    fn test_poll_signal_outcome_can_be_taken_once() {
        let signal = Arc::new(PollSignal::new());
        let wake_probe = Arc::new(CountWake(AtomicUsize::new(0)));
        let wake = Waker::from(Arc::clone(&wake_probe));
        let mut context = Context::from_waker(&wake);
        assert!(matches!(signal.poll_take(&mut context), Poll::Pending));
        let publisher_signal = Arc::clone(&signal);
        let publisher = std::thread::spawn(move || publisher_signal.publish("outcome"));
        publisher.join().unwrap();
        assert!(wake_probe.0.load(Ordering::SeqCst) > 0);
        assert!(matches!(signal.poll_take(&mut context), Poll::Ready("outcome")));
        assert!(matches!(signal.poll_take(&mut context), Poll::Pending));
    }
}
