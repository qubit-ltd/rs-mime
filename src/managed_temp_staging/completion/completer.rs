// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Sender half of a completion channel.

use std::sync::Arc;
use std::sync::Mutex;

use super::Completion;

/// Publishes one result to a waiting completion future.
///
/// # Type Parameters
/// - `T`: Result value shared with the receiver.
#[must_use]
pub(in crate::managed_temp_staging) struct Completer<T>(
    #[doc = "Shared result and receiver-waker state."] pub(in crate::managed_temp_staging) Arc<Mutex<Completion<T>>>,
);

impl<T> Completer<T> {
    /// Stores the result and wakes a receiver that is already waiting.
    ///
    /// # Parameters
    /// - `value`: Result to publish to the receiver.
    ///
    /// # Returns
    /// Nothing; the waiting receiver is woken when needed.
    pub(in crate::managed_temp_staging) fn complete(self, value: T) {
        let waker = {
            let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
            state.value = Some(value);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;

    use super::super::completion_pair;

    #[test]
    fn test_completion_sender_recovers_a_poisoned_shared_state() {
        let (sender, receiver) = completion_pair::<u8>();
        let state = Arc::clone(&sender.0);
        let poisoner = std::thread::spawn(move || {
            let _state = state.lock().unwrap();
            panic!("poison completion state before publishing");
        });
        assert!(poisoner.join().is_err());

        sender.complete(27);
        let mut receiver = Box::pin(receiver);
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(matches!(receiver.as_mut().poll(&mut context), Poll::Ready(27)));
    }
}
