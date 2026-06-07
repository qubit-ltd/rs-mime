// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Media stream classifier implementations.

pub(crate) mod ffprobe_command_media_stream_classifier;
pub(crate) mod ffprobe_command_media_stream_classifier_provider;
pub(crate) mod file_based_media_stream_classifier;
pub(crate) mod media_stream_classifier;
pub(crate) mod media_stream_classifier_availability;
pub(crate) mod media_stream_classifier_backend;
pub(crate) mod media_stream_classifier_helpers;
pub(crate) mod media_stream_classifier_provider;
pub(crate) mod media_stream_classifier_registry;
pub(crate) mod media_stream_classifier_spec;
mod media_stream_type;

pub use ffprobe_command_media_stream_classifier::FfprobeCommandMediaStreamClassifier;
pub use ffprobe_command_media_stream_classifier_provider::FfprobeCommandMediaStreamClassifierProvider;
pub use file_based_media_stream_classifier::FileBasedMediaStreamClassifier;
pub use media_stream_classifier::MediaStreamClassifier;
pub use media_stream_classifier_availability::MediaStreamClassifierAvailability;
pub use media_stream_classifier_backend::MediaStreamClassifierBackend;
pub use media_stream_classifier_provider::MediaStreamClassifierProvider;
pub use media_stream_classifier_registry::MediaStreamClassifierRegistry;
pub use media_stream_classifier_spec::MediaStreamClassifierSpec;
pub use media_stream_type::MediaStreamType;
