/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # Qubit MIME
//!
//! MIME type detection based on filename glob rules and content magic rules.
//!

pub mod classifier;
pub mod detector;
pub mod repository;

mod common_mime_types;
mod constants;
mod mime_config;
mod mime_error;
mod mime_result;

pub use classifier::{
    ArcMediaStreamClassifier, BoxMediaStreamClassifier, FfprobeCommandMediaStreamClassifier,
    FileBasedMediaStreamClassifier, MediaStreamClassifier, MediaStreamClassifierBackend,
    MediaStreamType,
};
pub use common_mime_types::*;
pub use constants::*;
pub use detector::{
    ArcMimeDetector, BoxMimeDetector, DetectionSource, FileBasedMimeDetector,
    FileCommandMimeDetector, MimeDetectionPolicy, MimeDetector, MimeDetectorBackend,
    MimeDetectorCore, RepositoryMimeDetector,
};
pub use mime_config::MimeConfig;
pub use mime_error::MimeError;
pub use mime_result::MimeResult;
pub use repository::{
    MagicValueType, MimeGlob, MimeMagic, MimeMagicMatcher, MimeRepository, MimeType,
    MimeTypeBuilder,
};

#[cfg(coverage)]
#[doc(hidden)]
pub mod coverage_support {
    //! Coverage-only hooks for branches that require synthetic failures.

    /// Exercises coverage-only branches across modules.
    ///
    /// # Returns
    /// Summary strings from each exercised branch group.
    pub fn exercise_all() -> Vec<String> {
        let mut result = Vec::new();
        result.extend(
            crate::repository::magic_value_type::coverage_support::exercise_magic_value_types(),
        );
        result.extend(crate::mime_error::coverage_support::exercise_error_builders());
        result.extend(crate::repository::mime_glob::coverage_support::exercise_glob_edges());
        result.extend(crate::repository::mime_magic::coverage_support::exercise_magic_edges());
        result.extend(
            crate::repository::mime_magic_matcher::coverage_support::exercise_matcher_edges(),
        );
        result.extend(
            crate::repository::mime_repository::coverage_support::exercise_repository_edges(),
        );
        result.extend(crate::repository::mime_type::coverage_support::exercise_mime_type_edges());
        result.extend(
            crate::detector::repository_mime_detector::coverage_support::exercise_detector_edges(),
        );
        result.extend(
            crate::detector::repository_mime_detector::coverage_support::exercise_reader_errors(),
        );
        result.extend(crate::mime_config::coverage_support::exercise_config_edges());
        result.extend(crate::detector::coverage_support::exercise_detector_defaults());
        result.extend(crate::classifier::coverage_support::exercise_classifier_defaults());
        result.extend(crate::detector::mime_detector_core::coverage_support::exercise_core_edges());
        result.extend(
            crate::detector::file_based_mime_detector::coverage_support::exercise_file_based_edges(
            ),
        );
        result.extend(
            crate::detector::file_command_mime_detector::coverage_support::exercise_file_command_edges(),
        );
        result.extend(
            crate::detector::stream_based_mime_detector::coverage_support::exercise_stream_edges(),
        );
        result.extend(
            crate::classifier::media_stream_classifier_helpers::coverage_support::exercise_media_stream_classifier_helper_edges(),
        );
        result.extend(
            crate::classifier::file_based_media_stream_classifier::coverage_support::exercise_file_based_classifier_edges(),
        );
        result.extend(
            crate::classifier::ffprobe_command_media_stream_classifier::coverage_support::exercise_ffprobe_edges(),
        );
        result
    }
}
