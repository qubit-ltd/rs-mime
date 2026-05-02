/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Media stream classifier implementations.

pub(crate) mod abstract_media_stream_classifier;
pub(crate) mod arc_media_stream_classifier;
pub(crate) mod box_media_stream_classifier;
#[cfg(coverage)]
pub(crate) mod coverage_support;
pub(crate) mod ffprobe_command_media_stream_classifier;
pub(crate) mod file_based_media_stream_classifier;
pub(crate) mod media_stream_classifier;
pub(crate) mod media_stream_classifier_backend;
mod media_stream_type;

pub use abstract_media_stream_classifier::AbstractMediaStreamClassifier;
pub use arc_media_stream_classifier::ArcMediaStreamClassifier;
pub use box_media_stream_classifier::BoxMediaStreamClassifier;
pub use ffprobe_command_media_stream_classifier::FfprobeCommandMediaStreamClassifier;
pub use file_based_media_stream_classifier::FileBasedMediaStreamClassifier;
pub use media_stream_classifier::MediaStreamClassifier;
pub use media_stream_type::MediaStreamType;
