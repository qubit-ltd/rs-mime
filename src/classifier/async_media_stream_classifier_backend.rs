// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Explicit asynchronous media stream classifier interface.

use std::fmt::Debug;

use qubit_operation::OperationScope;

use crate::MediaStreamType;
use crate::MimeFuture;
use crate::MimeOperationOptions;
use crate::detector::BorrowedMimeInput;

/// Asynchronous media stream classifier. Implementations must not call a
/// synchronous classifier implicitly.
pub trait AsyncMediaStreamClassifierBackend: Debug + Send + Sync {
    /// Classifies stream content within this operation's context.
    fn classify_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MediaStreamType>;
}
