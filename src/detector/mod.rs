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

pub(crate) mod arc_mime_detector;
pub(crate) mod box_mime_detector;
// qubit-style: allow coverage-cfg
#[cfg(coverage)]
pub(crate) mod coverage_support;
pub(crate) mod detection_source;
pub(crate) mod file_based_mime_detector;
pub(crate) mod file_command_mime_detector;
pub(crate) mod mime_detection_policy;
pub(crate) mod mime_detector;
pub(crate) mod mime_detector_backend;
pub(crate) mod mime_detector_core;
pub(crate) mod mime_detector_kind;
pub(crate) mod repository_mime_detector;
pub(crate) mod stream_based_mime_detector;

pub use arc_mime_detector::ArcMimeDetector;
pub use box_mime_detector::BoxMimeDetector;
pub use detection_source::DetectionSource;
pub use file_based_mime_detector::FileBasedMimeDetector;
pub use file_command_mime_detector::FileCommandMimeDetector;
pub use mime_detection_policy::MimeDetectionPolicy;
pub use mime_detector::MimeDetector;
pub use mime_detector_backend::MimeDetectorBackend;
pub use mime_detector_core::MimeDetectorCore;
pub use repository_mime_detector::RepositoryMimeDetector;
