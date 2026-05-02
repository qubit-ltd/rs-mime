/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! MIME detector implementations.

pub(crate) mod abstract_mime_detector;
pub(crate) mod arc_mime_detector;
pub(crate) mod box_mime_detector;
#[cfg(coverage)]
pub(crate) mod coverage_support;
pub(crate) mod detection_source;
pub(crate) mod file_based_mime_detector;
pub(crate) mod file_command_mime_detector;
pub(crate) mod mime_detection_policy;
pub(crate) mod mime_detector;
pub(crate) mod mime_detector_backend;
pub(crate) mod repository_mime_detector;
pub(crate) mod stream_based_mime_detector;
pub(crate) mod string_list_mime_detector_backend;

pub use abstract_mime_detector::AbstractMimeDetector;
pub use arc_mime_detector::ArcMimeDetector;
pub use box_mime_detector::BoxMimeDetector;
pub use detection_source::DetectionSource;
pub use file_based_mime_detector::FileBasedMimeDetector;
pub use file_command_mime_detector::FileCommandMimeDetector;
pub use mime_detection_policy::MimeDetectionPolicy;
pub use mime_detector::MimeDetector;
pub use repository_mime_detector::RepositoryMimeDetector;
pub use stream_based_mime_detector::StreamBasedMimeDetector;
pub use string_list_mime_detector_backend::StringListMimeDetectorBackend;
