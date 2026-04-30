/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # Qubit MIME
//!
//! MIME type detection based on filename glob rules and content magic rules.
//!
//! # Author
//!
//! Haixing Hu

pub mod classifier;
pub mod detector;
pub mod repository;

mod common_mime_types;
mod constants;
mod media_stream_classifier;
mod mime_config;
mod mime_detector;

pub use classifier::{
    AbstractMediaStreamClassifier, FfprobeCommandMediaStreamClassifier,
    FileBasedMediaStreamClassifier, MediaStreamType,
};
pub use common_mime_types::*;
pub use constants::*;
pub use detector::{
    AbstractMimeDetector, DetectionSource, FileBasedMimeDetector, FileCommandMimeDetector,
    RepositoryMimeDetector, StreamBasedMimeDetector, StringListMimeDetectorBackend,
};
pub use media_stream_classifier::{MediaStreamClassifier, default_media_stream_classifier};
pub use mime_config::MimeConfig;
pub use mime_detector::{MimeDetectionPolicy, MimeDetector, default_mime_detector};
pub use repository::{
    MagicValueType, MimeError, MimeGlob, MimeMagic, MimeMagicMatcher, MimeRepository, MimeType,
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
        result.extend(crate::repository::mime_error::coverage_support::exercise_error_builders());
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
        result.extend(crate::mime_detector::coverage_support::exercise_detector_defaults());
        result.extend(
            crate::media_stream_classifier::coverage_support::exercise_classifier_defaults(),
        );
        result.extend(
            crate::detector::abstract_mime_detector::coverage_support::exercise_abstract_edges(),
        );
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
            crate::classifier::abstract_media_stream_classifier::coverage_support::exercise_abstract_classifier_edges(),
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
