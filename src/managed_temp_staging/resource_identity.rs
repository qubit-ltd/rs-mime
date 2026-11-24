// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! One-time resource identity publication shared with asynchronous cleanup.

use std::sync::Mutex;

use qubit_operation::ResourceId;

// Implements the cancellable waiter and its synchronized publication state.
mod internal;
#[cfg(test)]
mod publish_event;

use internal::IdentityFuture;
use internal::State;
#[cfg(test)]
pub(crate) use publish_event::PublishEvent;
#[cfg(test)]
pub(crate) use publish_event::PublishHook;

/// Shares a registered resource ID with cleanup that may start before commit.
#[derive(Default)]
pub(super) struct ResourceIdentity {
    /// Stores the published ID and the waker waiting for its publication.
    state: Mutex<State>,
}

impl ResourceIdentity {
    /// Installs a test observer for cleanup and ID publication events.
    ///
    /// # Parameters
    /// - `hook`: callback invoked at test synchronization points.
    #[cfg(test)]
    pub(crate) fn set_hook(&self, hook: PublishHook) {
        self.state.lock().unwrap_or_else(|poison| poison.into_inner()).hook = Some(hook);
    }

    /// Creates an identity that already contains a registered resource ID.
    ///
    /// # Parameters
    /// - `id`: resource ID already assigned by recovery registration.
    ///
    /// # Returns
    /// An identity that immediately resolves cleanup waiters to `id`.
    #[cfg(test)]
    pub(super) fn published(id: ResourceId) -> Self {
        Self {
            state: Mutex::new(State {
                value: Some(id),
                ..State::default()
            }),
        }
    }

    /// Publishes the ID once and wakes cleanup waiting for registration.
    ///
    /// # Panics
    /// Panics if an ID has already been published for this staged resource.
    ///
    /// # Parameters
    /// - `id`: resource ID assigned by recovery registration.
    pub(super) fn publish(&self, id: ResourceId) {
        #[cfg(test)]
        let hook = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .hook
            .clone();
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(PublishEvent::Publishing(id));
        }
        let waiter = {
            let mut state = self.state.lock().unwrap_or_else(|poison| poison.into_inner());
            assert!(state.value.is_none(), "resource identity published more than once");
            state.value = Some(id);
            state.waiter.take().map(|(_, waiter)| waiter)
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    /// Returns a future that waits until the recovery system assigns an ID.
    pub(super) fn wait(&self) -> IdentityFuture<'_> {
        let token = {
            let mut state = self.state.lock().unwrap_or_else(|poison| poison.into_inner());
            state.next_waiter = state.next_waiter.wrapping_add(1);
            state.next_waiter
        };
        IdentityFuture { identity: self, token }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::Context;
    use std::task::Waker;

    use super::ResourceIdentity;

    #[test]
    fn test_cancelled_identity_wait_removes_its_waker() {
        let identity = ResourceIdentity::default();
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(identity.wait());
        assert!(future.as_mut().poll(&mut context).is_pending());
        assert!(identity.state.lock().unwrap().waiter.is_some());
        drop(future);
        assert!(identity.state.lock().unwrap().waiter.is_none());
    }
}
