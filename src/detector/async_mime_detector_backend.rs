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
/// The `'a` bound limits how long a pending future may be retained; backend
/// methods use it to tie in-flight work to their borrowed inputs.
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
///
/// # Examples
///
/// A caller supplies the operation context and borrowed input from its current
/// detection flow. Awaiting the backend future performs the inspection and
/// propagates any MIME error.
///
/// ```
/// use qubit_mime::{
///     AsyncMimeDetectorBackend, BorrowedMimeInput, MimeCandidates,
///     MimeOperationOptions, MimeResult,
/// };
/// use qubit_operation::OperationScope;
///
/// async fn inspect(
///     backend: &dyn AsyncMimeDetectorBackend,
///     scope: &OperationScope,
///     input: BorrowedMimeInput<'_>,
///     options: &MimeOperationOptions,
/// ) -> MimeResult<MimeCandidates> {
///     backend.guess_content_in(scope, input, options).await
/// }
/// ```
pub trait AsyncMimeDetectorBackend: Debug + Send + Sync {
    /// Returns the minimum input extent this backend needs to inspect before it
    /// can produce a useful MIME candidate.
    ///
    /// # Returns
    /// The minimum extent callers should make available before requesting a
    /// candidate from this backend.
    fn content_requirement(&self) -> ContentRequirement;

    /// Returns whether this backend can sample arbitrary positions from the
    /// same opened input object.
    ///
    /// The detector uses this capability to choose a compatible read strategy;
    /// implementations must report it explicitly rather than relying on a
    /// synchronous fallback.
    ///
    /// # Returns
    /// The backend's declared random-access capability for one opened input.
    fn random_access_capability(&self) -> RandomAccessCapability;

    /// Returns whether provider inputs must be delivered through a metered
    /// same-object read session instead of an unbounded filesystem handle.
    ///
    /// Complete reads and same-object sampling always use metered sessions,
    /// regardless of this compatibility hook's return value. Prefix backends
    /// normally receive bounded bytes; a backend that prefers a metered
    /// sequential session may override this method to return `true`.
    ///
    /// # Returns
    /// `true` when provider input should be opened once and exposed only
    /// through [`crate::MeteredReadSession`].
    fn uses_managed_provider_reads(&self) -> bool {
        false
    }

    /// Inspects input within this operation's context.
    ///
    /// # Parameters
    /// - `scope`: Operation context that controls cancellation and accounting.
    /// - `input`: Borrowed MIME input to inspect.
    /// - `options`: Per-operation limits and selection preferences.
    ///
    /// # Returns
    /// A future that must be awaited or otherwise polled to run the inspection;
    /// it resolves to the backend's candidates.
    ///
    /// # Errors
    /// The future resolves to a MIME error if the backend cannot inspect the
    /// input or the operation is cancelled or exceeds a configured limit.
    fn guess_content_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates>;

    /// Inspects provider content with its native basename retained as a
    /// distinct filename source.
    ///
    /// The default preserves existing backend behavior by forwarding the
    /// original input unchanged. Repository backends can override this hook
    /// to avoid treating separator characters in a provider basename as path
    /// separators. For ordinary byte inputs, callers continue to use
    /// guess_content_in directly.
    ///
    /// # Parameters
    /// - scope: Operation context controlling cancellation and accounting.
    /// - input: Borrowed provider content, usually bounded bytes or a managed
    ///   provider session.
    /// - basename: Optional basename taken from the typed provider path.
    /// - options: Per-operation limits and selection preferences.
    ///
    /// # Returns
    /// A future resolving to the backend's MIME candidates.
    ///
    /// # Errors
    /// Resolves to an error if the backend cannot inspect the provider input.
    #[doc(hidden)]
    fn guess_provider_content_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        _basename: Option<&'a str>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        self.guess_content_in(scope, input, options)
    }
}
