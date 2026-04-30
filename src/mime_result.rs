/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Result type used by MIME database parsing and detection.
//!
//! # Author
//!
//! Haixing Hu

use crate::MimeError;

/// A convenient alias for results returned by MIME-related functions.
pub type MimeResult<T> = Result<T, MimeError>;
