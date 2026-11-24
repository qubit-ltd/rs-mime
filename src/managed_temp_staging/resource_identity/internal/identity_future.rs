// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Cancellable future that waits for cleanup ownership registration.

use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use qubit_operation::ResourceId;

#[cfg(test)]
use super::super::PublishEvent;
use super::super::ResourceIdentity;

/// Waits for the ID assigned when recovery takes ownership of a staged file.
pub(in crate::managed_temp_staging) struct IdentityFuture<'a> {
    /// Identity state shared with the recovery registration path.
    pub(in crate::managed_temp_staging::resource_identity) identity: &'a ResourceIdentity,
    /// Token preventing this future from removing a newer waiter's waker.
    pub(in crate::managed_temp_staging::resource_identity) token: u64,
}

impl Future for IdentityFuture<'_> {
    /// Resource ID assigned when recovery takes ownership of the staged file.
    type Output = ResourceId;

    /// Resolves the registered ID or stores the current task's waker.
    ///
    /// # Parameters
    /// - `self`: identity wait whose cancellation owns its waiter token.
    /// - `context`: executor context providing the waker for a pending wait.
    ///
    /// # Returns
    /// `Ready` with the ID after publication, or `Pending` until then.
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let candidate = context.waker().clone();
        let (result, old_waiter, hook) = {
            let mut state = self.identity.state.lock().unwrap_or_else(|poison| poison.into_inner());
            if let Some(id) = state.value {
                (Poll::Ready(id), None, None)
            } else {
                let old = state.waiter.replace((self.token, candidate)).map(|(_, waker)| waker);
                #[cfg(test)]
                let hook = state.hook.clone();
                #[cfg(not(test))]
                let hook: Option<()> = None;
                (Poll::Pending, old, hook)
            }
        };
        drop(old_waiter);
        #[cfg(test)]
        if result.is_pending()
            && let Some(hook) = hook
        {
            hook(PublishEvent::CleanupPending);
        }
        #[cfg(not(test))]
        let _ = hook;
        result
    }
}

impl Drop for IdentityFuture<'_> {
    /// Removes this future's waker if it is still the registered waiter.
    ///
    /// # Parameters
    /// - `self`: cancelled or completed wait being dropped.
    fn drop(&mut self) {
        let waiter = {
            let mut state = self.identity.state.lock().unwrap_or_else(|poison| poison.into_inner());
            if state.waiter.as_ref().is_some_and(|(token, _)| *token == self.token) {
                state.waiter.take().map(|(_, waker)| waker)
            } else {
                None
            }
        };
        drop(waiter);
    }
}
