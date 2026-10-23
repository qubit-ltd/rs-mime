// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Core media stream classifier backend contract.

use std::fmt::Debug;
use std::io::Read;
use std::path::Path;

use super::MediaStreamClassifier;
use super::MediaStreamType;
use super::media_stream_classifier_helpers::validate_readable_file;
use crate::MimeResult;

/// Core implementation contract for classifiers that can handle local files and
/// streams.
///
/// Implement this trait to provide classification behavior for a custom media
/// backend. The blanket [`MediaStreamClassifier`] implementation validates
/// local files before calling the corresponding backend hook.
///
/// # Examples
///
/// ```
/// use std::io::Read;
/// use std::path::Path;
///
/// use qubit_mime::MediaStreamClassifier;
/// use qubit_mime::MediaStreamClassifierBackend;
/// use qubit_mime::MediaStreamType;
/// use qubit_mime::MimeResult;
///
/// #[derive(Debug)]
/// struct AudioBackend;
///
/// impl MediaStreamClassifierBackend for AudioBackend {
///     fn classify_by_local_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
///         Ok(MediaStreamType::AudioOnly)
///     }
///
///     fn classify_by_content(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
///         Ok(MediaStreamType::AudioOnly)
///     }
/// }
///
/// let backend = AudioBackend;
/// assert_eq!(
///     backend.classify_content(b"audio data")?,
///     MediaStreamType::AudioOnly,
/// );
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
pub trait MediaStreamClassifierBackend: Debug + Send + Sync {
    /// Classifies a local file after the blanket classifier implementation has
    /// validated that it is readable.
    ///
    /// # Parameters
    /// - `file`: Readable local media file whose contents the backend inspects.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) or another
    /// [`MimeError`](crate::MimeError) when backend classification fails.
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType>;

    /// Classifies media bytes read from a stream.
    ///
    /// # Parameters
    /// - `reader`: Media stream to consume as needed during classification.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) or another
    /// [`MimeError`](crate::MimeError) when backend classification fails.
    fn classify_by_content(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType>;
}

impl<T> MediaStreamClassifier for T
where
    T: MediaStreamClassifierBackend,
{
    /// Validates the local file and delegates to the backend hook.
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        validate_readable_file(file)?;
        self.classify_by_local_file(file)
    }

    /// Delegates stream classification to the backend hook.
    #[inline]
    fn classify_reader(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        self.classify_by_content(reader)
    }
}
