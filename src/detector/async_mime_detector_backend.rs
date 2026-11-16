// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Explicit asynchronous MIME detector interface.

use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

use qubit_operation::OperationScope;

use super::mime_detection_report::MimeCandidates;
use super::mime_input::BorrowedMimeInput;
use super::mime_operation_options::MimeOperationOptions;
use crate::ContentRequirement;
use crate::MimeResult;

mod random_access_capability;

pub use self::random_access_capability::RandomAccessCapability;

/// Boxed, sendable future returned by MIME domain backends.
pub type MimeFuture<'a, T> = Pin<Box<dyn Future<Output = MimeResult<T>> + Send + 'a>>;

/// Asynchronous MIME detector. Implementations must not fall back to a sync
/// hook implicitly.
pub trait AsyncMimeDetectorBackend: Debug + Send + Sync {
    /// Required inspection extent.
    fn content_requirement(&self) -> ContentRequirement;

    /// Declares whether provider inputs support random sampling of the same
    /// opened object. Implementations must state this explicitly.
    fn random_access_capability(&self) -> RandomAccessCapability;

    /// Inspects input within this operation's context.
    fn guess_content_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates>;
}
