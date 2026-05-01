/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Top-level media stream classifier interface.
//!
//! # Author
//!
//! Haixing Hu

use std::fmt::Debug;
use std::path::Path;

use crate::MimeResult;

use super::MediaStreamType;

/// Classifies a media source by the audio and video streams it contains.
pub trait MediaStreamClassifier: Debug + Send + Sync {
    /// Classifies a local file.
    ///
    /// # Parameters
    /// - `file`: Local media file.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the file cannot be read, or another
    /// [`MimeError`](crate::MimeError) when the classifier backend fails.
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType>;

    /// Classifies an in-memory media payload.
    ///
    /// # Parameters
    /// - `content`: Media bytes to classify.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when a file-backed classifier cannot stage the
    /// content.
    fn classify_content(&self, content: &[u8]) -> MimeResult<MediaStreamType>;
}
