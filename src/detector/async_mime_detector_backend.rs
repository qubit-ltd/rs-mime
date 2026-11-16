// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

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
///
/// # Type Parameters
/// - `T`: Owned value produced by the backend.
pub type MimeFuture<'a, T> = Pin<Box<dyn Future<Output = MimeResult<T>> + Send + 'a>>;

/// Asynchronous MIME detector. Implementations must not fall back to a sync
/// hook implicitly.
///
/// The returned future is tied to the backend, operation scope, input, and
/// options borrows. It cannot be retained as a `'static` task:
///
/// ```compile_fail
/// use qubit_mime::{
///     AsyncMimeDetectorBackend, BorrowedMimeInput, MimeCandidates, MimeFuture,
///     MimeOperationOptions,
/// };
/// use qubit_operation::OperationScope;
///
/// fn escape_to_static(
///     backend: &impl AsyncMimeDetectorBackend,
///     scope: &OperationScope,
///     input: BorrowedMimeInput<'_>,
///     options: &MimeOperationOptions,
/// ) -> MimeFuture<'static, MimeCandidates> {
///     backend.guess_content_in(scope, input, options)
/// }
/// ```
pub trait AsyncMimeDetectorBackend: Debug + Send + Sync {
    /// Required inspection extent.
    fn content_requirement(&self) -> ContentRequirement;

    /// Declares whether provider inputs support random sampling of the same
    /// opened object. Implementations must state this explicitly.
    fn random_access_capability(&self) -> RandomAccessCapability;

    /// Inspects input within this operation's context.
    ///
    /// # Parameters
    /// - `scope`: Operation context that controls cancellation and accounting.
    /// - `input`: Borrowed MIME input to inspect.
    /// - `options`: Per-operation limits and selection preferences.
    ///
    /// # Returns
    /// A future that resolves to the backend's candidates or a MIME error.
    fn guess_content_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates>;
}
