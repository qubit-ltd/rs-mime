// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Creation behavior selected for one test provider.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TestProviderBehavior {
    /// Creates a service that reports the supplied MIME type.
    Success(&'static str),
    /// Rejects the requested capability.
    Unsupported,
    /// Reports a missing runtime dependency.
    Unavailable,
    /// Reports an unexpected provider initialization failure.
    InitializationFailed,
}
