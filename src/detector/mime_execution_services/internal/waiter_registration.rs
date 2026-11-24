// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::task::Waker;

/// One future's current notification target.
pub(in crate::detector::mime_execution_services) struct WaiterRegistration {
    /// Identity owned by the observing future.
    pub(in crate::detector::mime_execution_services) identity: Arc<()>,
    /// Current task waker for that future.
    pub(in crate::detector::mime_execution_services) waker: Waker,
}
