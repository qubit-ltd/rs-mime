// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Backend capabilities required by MIME operations.

/// MIME capability that an operation may require from its backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeCapability {
    /// Random sampling of offsets within the same opened logical object.
    RandomAccessSampling,
    /// Refinement can inspect the exact staged object without reopening a
    /// mutable provider path.
    SameObjectRefinement,
}
