/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Backend contract for MIME detector implementations.

use std::fmt::Debug;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use qubit_io::ReadSeek;

use crate::{
    DetectionSource,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorCore,
    MimeResult,
    StreamBasedMimeDetector,
};

use super::stream_based_mime_detector::read_prefix;

/// Core implementation contract for MIME detectors.
pub trait MimeDetectorBackend: Debug + Send + Sync {
    /// Gets the shared detector core.
    ///
    /// # Returns
    /// Shared detector configuration and merge/refinement behavior.
    fn core(&self) -> &MimeDetectorCore;

    /// Gets the maximum number of bytes needed for content inspection.
    ///
    /// # Returns
    /// Content prefix length to read from files and readers.
    fn max_test_bytes(&self) -> usize;

    /// Guesses MIME type names from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    ///
    /// # Returns
    /// Candidate MIME type names ordered by backend confidence.
    fn guess_from_filename(&self, filename: &str) -> Vec<String>;

    /// Guesses MIME type names from content bytes.
    ///
    /// # Parameters
    /// - `content`: Content bytes.
    ///
    /// # Returns
    /// Candidate MIME type names ordered by backend confidence.
    ///
    /// # Errors
    /// Returns an error when a backend cannot inspect the supplied content.
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>>;

    /// Guesses MIME type names from a seekable reader.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original position is restored.
    ///
    /// # Returns
    /// Candidate MIME type names and the content prefix used for refinement.
    ///
    /// # Errors
    /// Returns an error when reading, seeking, or backend inspection fails.
    fn guess_from_reader(&self, reader: &mut dyn ReadSeek) -> MimeResult<(Vec<String>, Vec<u8>)> {
        let content = read_prefix(reader, self.max_test_bytes(), self.core().max_buffer_size())?;
        let candidates = self.guess_from_content(&content)?;
        Ok((candidates, content))
    }

    /// Guesses MIME type names from a local file.
    ///
    /// # Parameters
    /// - `file`: Local file path.
    ///
    /// # Returns
    /// Candidate MIME type names and the content prefix used for refinement.
    ///
    /// # Errors
    /// Returns an error when opening, reading, seeking, or backend inspection fails.
    fn guess_from_file(&self, file: &Path) -> MimeResult<(Vec<String>, Vec<u8>)> {
        let mut reader = BufReader::new(File::open(file)?);
        self.guess_from_reader(&mut reader)
    }
}

impl<T> MimeDetector for T
where
    T: MimeDetectorBackend,
{
    /// Detects a MIME type from filename candidates.
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        self.guess_from_filename(filename).first().map(|mime_type| {
            self.core()
                .refine_detected_mime_type(mime_type, Some(filename), DetectionSource::None)
        })
    }

    /// Detects a MIME type from content candidates.
    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        self.guess_from_content(content)
            .ok()?
            .first()
            .map(|mime_type| {
                self.core().refine_detected_mime_type(
                    mime_type,
                    None,
                    DetectionSource::Content(content),
                )
            })
    }

    /// Detects a MIME type from content bytes and an optional filename.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        let from_filename = filename
            .map(|filename| self.guess_from_filename(filename))
            .unwrap_or_default();
        let from_content =
            if from_filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
                Vec::new()
            } else {
                self.guess_from_content(content).unwrap_or_default()
            };
        self.core().select_result(
            &from_filename,
            &from_content,
            filename,
            policy,
            DetectionSource::Content(content),
        )
    }

    /// Detects a MIME type from a seekable reader.
    fn detect_reader(
        &self,
        reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        let from_filename = filename
            .map(|filename| self.guess_from_filename(filename))
            .unwrap_or_default();
        let (from_content, content) =
            if from_filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
                (Vec::new(), Vec::new())
            } else {
                self.guess_from_reader(reader)?
            };
        Ok(self.core().select_result(
            &from_filename,
            &from_content,
            filename,
            policy,
            DetectionSource::Content(&content),
        ))
    }

    /// Detects a MIME type from a local file.
    fn detect_file(&self, file: &Path, policy: MimeDetectionPolicy) -> MimeResult<Option<String>> {
        let filename = file.to_string_lossy();
        let from_filename = self.guess_from_filename(&filename);
        let (from_content, _content) =
            if from_filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
                (Vec::new(), Vec::new())
            } else {
                self.guess_from_file(file)?
            };
        Ok(self.core().select_result(
            &from_filename,
            &from_content,
            Some(&filename),
            policy,
            DetectionSource::Path(file),
        ))
    }
}

impl<T> MimeDetectorBackend for T
where
    T: StreamBasedMimeDetector,
{
    /// Gets the shared detector core.
    fn core(&self) -> &MimeDetectorCore {
        StreamBasedMimeDetector::core(self)
    }

    /// Gets the maximum content prefix length needed by this detector.
    fn max_test_bytes(&self) -> usize {
        StreamBasedMimeDetector::max_test_bytes(self)
    }

    /// Guesses MIME type names from filename rules.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        StreamBasedMimeDetector::guess_from_filename(self, filename)
    }

    /// Guesses MIME type names from content bytes.
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        StreamBasedMimeDetector::guess_from_content_bytes(self, content)
    }

    /// Delegates reader inspection to the stream-based hook.
    fn guess_from_reader(&self, reader: &mut dyn ReadSeek) -> MimeResult<(Vec<String>, Vec<u8>)> {
        StreamBasedMimeDetector::guess_from_reader_stream(self, reader)
    }

    /// Delegates local-file inspection to the stream-based hook.
    fn guess_from_file(&self, file: &Path) -> MimeResult<(Vec<String>, Vec<u8>)> {
        StreamBasedMimeDetector::guess_from_file_stream(self, file)
    }
}
