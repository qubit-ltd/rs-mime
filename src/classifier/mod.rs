/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Media stream classifier implementations.

pub(crate) mod abstract_media_stream_classifier;
pub(crate) mod ffprobe_command_media_stream_classifier;
pub(crate) mod file_based_media_stream_classifier;
mod media_stream_type;

pub use abstract_media_stream_classifier::AbstractMediaStreamClassifier;
pub use ffprobe_command_media_stream_classifier::FfprobeCommandMediaStreamClassifier;
pub use file_based_media_stream_classifier::FileBasedMediaStreamClassifier;
pub use media_stream_type::MediaStreamType;
