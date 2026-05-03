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
    ArcMediaStreamClassifier,
    BoxMediaStreamClassifier,
    FfprobeCommandMediaStreamClassifier,
    FileBasedMediaStreamClassifier,
    MediaStreamClassifier,
    MediaStreamClassifierBackend,
    MediaStreamType,
};
pub use common_mime_types::*;
pub use constants::*;
pub use detector::{
    ArcMimeDetector,
    BoxMimeDetector,
    DetectionSource,
    FileBasedMimeDetector,
    FileCommandMimeDetector,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorBackend,
    MimeDetectorCore,
    RepositoryMimeDetector,
};
pub use mime_config::MimeConfig;
pub use mime_error::MimeError;
pub use mime_result::MimeResult;
pub use repository::{
    MagicValueType,
    MimeGlob,
    MimeMagic,
    MimeMagicMatcher,
    MimeRepository,
    MimeType,
    MimeTypeBuilder,
};
