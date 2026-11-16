// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Explicit asynchronous MIME detector interface.

use super::{
    mime_detection_report::MimeCandidates, mime_input::BorrowedMimeInput,
    mime_operation_options::MimeOperationOptions,
};
use crate::{ContentRequirement, MimeResult};
use qubit_operation::OperationScope;
use std::{fmt::Debug, future::Future, pin::Pin};

/// Boxed, sendable future returned by MIME domain backends.
pub type MimeFuture<'a, T> = Pin<Box<dyn Future<Output = MimeResult<T>> + Send + 'a>>;

/// Asynchronous MIME detector. Implementations must not fall back to a sync
/// hook implicitly.
pub trait AsyncMimeDetectorBackend: Debug + Send + Sync {
    /// Required inspection extent.
    fn content_requirement(&self) -> ContentRequirement;

    /// Inspects input within this operation's context.
    fn guess_content_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates>;
}
