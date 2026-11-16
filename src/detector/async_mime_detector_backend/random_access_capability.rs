// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Random-access provider capability declared by an async backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomAccessCapability {
    /// Backend cannot inspect provider content at arbitrary offsets.
    Unsupported,
    /// Backend can sample arbitrary offsets on the same opened logical object.
    SamplesSameObject,
}
