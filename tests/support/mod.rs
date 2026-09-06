// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared integration test support.

mod direct_backend_detector;
#[cfg(unix)]
mod path_env_guard;
mod prefix_file_system_spi;
mod static_entry_point_mime_detector;
mod static_media_stream_classifier;
mod static_mime_detector;
mod test_media_stream_classifier_provider;
mod test_mime_detector_provider;
mod test_provider_behavior;

pub(crate) use direct_backend_detector::DirectBackendDetector;
#[cfg(unix)]
pub(crate) use path_env_guard::PathEnvGuard;
pub(crate) use prefix_file_system_spi::PrefixFileSystemSpi;
pub(crate) use static_entry_point_mime_detector::StaticEntryPointMimeDetector;
pub(crate) use static_media_stream_classifier::StaticMediaStreamClassifier;
pub(crate) use static_mime_detector::StaticMimeDetector;
pub(crate) use test_media_stream_classifier_provider::TestMediaStreamClassifierProvider;
pub(crate) use test_mime_detector_provider::TestMimeDetectorProvider;
pub(crate) use test_provider_behavior::TestProviderBehavior;
