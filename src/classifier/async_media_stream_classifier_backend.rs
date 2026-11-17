// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
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
///
/// # Examples
///
/// ```
/// use qubit_mime::{
///     AsyncMediaStreamClassifierBackend, BorrowedMimeInput, MediaStreamType,
///     MimeOperationOptions, MimeResult,
/// };
/// use qubit_operation::OperationScope;
///
/// async fn classify<'a>(
///     backend: &'a dyn AsyncMediaStreamClassifierBackend,
///     scope: &'a OperationScope,
///     input: BorrowedMimeInput<'a>,
///     options: &'a MimeOperationOptions,
/// ) -> MimeResult<MediaStreamType> {
///     backend.classify_in(scope, input, options).await
/// }
/// ```
///
/// The surrounding MIME operation supplies the operation scope, borrowed
/// input, and detection options.
pub trait AsyncMediaStreamClassifierBackend: Debug + Send + Sync {
    /// Classifies stream content within this operation's context.
    ///
    /// # Parameters
    /// - `scope`: Operation scope used to observe cancellation and deadlines.
    /// - `input`: Borrowed MIME input, valid only for the returned future's
    ///   lifetime.
    /// - `options`: Detection options that constrain this classification.
    ///
    /// # Returns
    /// A future that resolves to the classified media stream type or an error.
    fn classify_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MediaStreamType>;
}
