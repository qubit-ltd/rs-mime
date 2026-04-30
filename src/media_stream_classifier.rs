/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Top-level media stream classifier interface.

use std::fmt::Debug;
use std::path::Path;

use crate::{FfprobeCommandMediaStreamClassifier, MediaStreamType, MimeError};

/// Classifies a media source by the audio and video streams it contains.
pub trait MediaStreamClassifier: Debug + Send + Sync {
    /// Classifies a local path.
    ///
    /// # Parameters
    /// - `path`: Local media path.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when the path cannot be read, or another
    /// [`MimeError`] when the classifier backend fails.
    fn classify_path(&self, path: &Path) -> Result<MediaStreamType, MimeError>;

    /// Classifies an in-memory media payload.
    ///
    /// # Parameters
    /// - `content`: Media bytes to classify.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when a file-backed classifier cannot stage the
    /// content.
    fn classify_content(&self, content: &[u8]) -> Result<MediaStreamType, MimeError>;
}

impl dyn MediaStreamClassifier {
    /// Gets the default media stream classifier when a backend is available.
    ///
    /// # Returns
    /// A FFprobe-backed classifier when `ffprobe` can be executed, otherwise
    /// `None`.
    pub fn default_classifier() -> Option<Box<dyn MediaStreamClassifier>> {
        default_media_stream_classifier()
    }
}

/// Gets the default media stream classifier.
///
/// # Returns
/// A FFprobe-backed classifier when available, otherwise `None`.
pub fn default_media_stream_classifier() -> Option<Box<dyn MediaStreamClassifier>> {
    default_media_stream_classifier_from_availability(
        FfprobeCommandMediaStreamClassifier::is_available(),
    )
}

/// Selects the default classifier from backend availability.
///
/// # Parameters
/// - `available`: Whether the FFprobe backend is available.
///
/// # Returns
/// A FFprobe-backed classifier when `available` is `true`, otherwise `None`.
fn default_media_stream_classifier_from_availability(
    available: bool,
) -> Option<Box<dyn MediaStreamClassifier>> {
    if available {
        Some(Box::new(FfprobeCommandMediaStreamClassifier::new()))
    } else {
        None
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for default classifier selection.

    use super::{
        MediaStreamClassifier, default_media_stream_classifier,
        default_media_stream_classifier_from_availability,
    };

    /// Exercises optional default classifier selection.
    ///
    /// # Returns
    /// Summary strings from default classifier lookups.
    pub(crate) fn exercise_classifier_defaults() -> Vec<String> {
        vec![
            default_media_stream_classifier().is_some().to_string(),
            default_media_stream_classifier_from_availability(true)
                .is_some()
                .to_string(),
            default_media_stream_classifier_from_availability(false)
                .is_none()
                .to_string(),
            <dyn MediaStreamClassifier>::default_classifier()
                .is_some()
                .to_string(),
        ]
    }
}
