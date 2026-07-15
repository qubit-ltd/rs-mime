// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! MIME detector implementations.

pub(crate) mod detection_source;
pub(crate) mod file_based_mime_detector;
pub(crate) mod file_command_mime_detector;
pub(crate) mod file_command_mime_detector_provider;
pub(crate) mod mime_detection_policy;
pub(crate) mod mime_detector;
pub(crate) mod mime_detector_backend;
pub(crate) mod mime_detector_core;
pub(crate) mod mime_detector_provider;
pub(crate) mod mime_detector_registry;
pub(crate) mod mime_detector_registry_builder;
pub(crate) mod mime_detector_spec;
pub(crate) mod repository_mime_detector;
pub(crate) mod repository_mime_detector_provider;
pub(crate) mod stream_based_mime_detector;

pub use detection_source::DetectionSource;
pub use file_based_mime_detector::FileBasedMimeDetector;
pub use file_command_mime_detector::FileCommandMimeDetector;
pub use file_command_mime_detector_provider::{
    FileCommandMimeDetectorProvider,
    file_command_mime_detector_descriptor,
};
pub use mime_detection_policy::MimeDetectionPolicy;
pub use mime_detector::MimeDetector;
pub use mime_detector_backend::MimeDetectorBackend;
pub use mime_detector_core::MimeDetectorCore;
pub use mime_detector_provider::MimeDetectorProvider;
pub use mime_detector_registry::MimeDetectorRegistry;
pub use mime_detector_registry_builder::MimeDetectorRegistryBuilder;
pub use mime_detector_spec::MimeDetectorSpec;
pub use repository_mime_detector::RepositoryMimeDetector;
pub use repository_mime_detector_provider::{
    RepositoryMimeDetectorProvider,
    repository_mime_detector_descriptor,
};
pub use stream_based_mime_detector::StreamBasedMimeDetector;
