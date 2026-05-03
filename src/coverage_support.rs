/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
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
    result
        .extend(crate::repository::mime_magic_matcher::coverage_support::exercise_matcher_edges());
    result
        .extend(crate::repository::mime_repository::coverage_support::exercise_repository_edges());
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
        crate::detector::file_based_mime_detector::coverage_support::exercise_file_based_edges(),
    );
    result.extend(
        crate::detector::file_command_mime_detector::coverage_support::exercise_file_command_edges(
        ),
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
