// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Synchronized state used to publish a staged resource's recovery ID.

use std::task::Waker;

use qubit_operation::ResourceId;

#[cfg(test)]
use crate::managed_temp_staging::resource_identity::PublishHook;

/// Holds the ID and the single cleanup waiter before publication completes.
#[derive(Default)]
pub(in crate::managed_temp_staging::resource_identity) struct State {
    /// The recovery ID once cleanup ownership has been registered.
    pub(in crate::managed_temp_staging::resource_identity) value: Option<ResourceId>,
    /// Monotonic token source used to distinguish replaced waiters.
    pub(in crate::managed_temp_staging::resource_identity) next_waiter: u64,
    /// The current waiter token and waker, if cleanup is waiting for an ID.
    pub(in crate::managed_temp_staging::resource_identity) waiter: Option<(u64, Waker)>,
    /// Test observer for the cleanup pending and identity publication events.
    #[cfg(test)]
    pub(in crate::managed_temp_staging::resource_identity) hook: Option<PublishHook>,
}
