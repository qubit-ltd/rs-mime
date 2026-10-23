// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Top-level media stream classifier interface.

use std::fmt::Debug;
use std::io::Cursor;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use super::MediaStreamType;
use crate::MimeResult;

/// Classifies a media source by the audio and video streams it contains.
///
/// Implement this trait to provide file and reader classification. The default
/// content method adapts an in-memory byte slice to the reader method.
///
/// # Examples
///
/// ```
/// use std::io::Read;
/// use std::path::Path;
///
/// use qubit_mime::MediaStreamClassifier;
/// use qubit_mime::MediaStreamType;
/// use qubit_mime::MimeResult;
///
/// #[derive(Debug)]
/// struct AudioClassifier;
///
/// impl MediaStreamClassifier for AudioClassifier {
///     fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
///         Ok(MediaStreamType::AudioOnly)
///     }
///
///     fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
///         Ok(MediaStreamType::AudioOnly)
///     }
/// }
///
/// # fn main() -> MimeResult<()> {
/// let classifier = AudioClassifier;
/// assert_eq!(
///     classifier.classify_content(b"audio bytes")?,
///     MediaStreamType::AudioOnly,
/// );
/// # Ok(())
/// # }
/// ```
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
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the file cannot be
    /// read, or another [`MimeError`](crate::MimeError) when the classifier
    /// backend fails.
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType>;

    /// Classifies media bytes from a reader.
    ///
    /// # Parameters
    /// - `reader`: Media stream to classify. The stream is consumed as needed
    ///   by the classifier.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the stream cannot
    /// be read, or another [`MimeError`](crate::MimeError) when the
    /// classifier backend fails.
    fn classify_reader(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType>;

    /// Classifies an in-memory media payload.
    ///
    /// # Parameters
    /// - `content`: Media bytes to classify.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when a file-backed
    /// classifier cannot stage the content, or another
    /// [`MimeError`](crate::MimeError) when classification fails.
    #[inline]
    fn classify_content(&self, content: &[u8]) -> MimeResult<MediaStreamType> {
        let mut cursor = Cursor::new(content);
        self.classify_reader(&mut cursor)
    }
}

impl MediaStreamClassifier for Box<dyn MediaStreamClassifier> {
    /// Delegates content classification to the boxed classifier.
    #[inline]
    fn classify_content(&self, content: &[u8]) -> MimeResult<MediaStreamType> {
        self.as_ref().classify_content(content)
    }

    /// Delegates file classification to the boxed classifier.
    #[inline]
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.as_ref().classify_file(file)
    }

    /// Delegates stream classification to the boxed classifier.
    #[inline]
    fn classify_reader(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        self.as_ref().classify_reader(reader)
    }
}

impl MediaStreamClassifier for Arc<dyn MediaStreamClassifier> {
    /// Delegates content classification to the shared classifier.
    #[inline]
    fn classify_content(&self, content: &[u8]) -> MimeResult<MediaStreamType> {
        self.as_ref().classify_content(content)
    }

    /// Delegates file classification to the shared classifier.
    #[inline]
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.as_ref().classify_file(file)
    }

    /// Delegates stream classification to the shared classifier.
    #[inline]
    fn classify_reader(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        self.as_ref().classify_reader(reader)
    }
}
