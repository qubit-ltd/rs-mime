// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Test-only events emitted around cleanup identity publication.

use std::sync::Arc;

use qubit_operation::ResourceId;

/// Identifies a test synchronization point in resource cleanup.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PublishEvent {
    /// Cleanup began waiting before recovery assigned the resource ID.
    CleanupPending,
    /// Recovery assigned the ID and is publishing it to cleanup.
    Publishing(ResourceId),
}

/// Observer invoked at a resource identity test synchronization point.
pub(crate) type PublishHook = Arc<dyn Fn(PublishEvent) + Send + Sync>;
