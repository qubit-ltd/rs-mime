// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Private state and future implementation for resource identity publication.

// Implements the cancellable future returned while cleanup awaits an ID.
mod identity_future;
// Stores the assigned ID, pending waiter, and test-only observer.
mod state;

pub(super) use identity_future::IdentityFuture;
pub(super) use state::State;
