/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Coverage helpers for default classifier selection.

use std::path::Path;
use std::sync::Arc;

use crate::{ArcMediaStreamClassifier, BoxMediaStreamClassifier, MediaStreamClassifier};

use super::FfprobeCommandMediaStreamClassifier;
use super::media_stream_classifier_backend::MediaStreamClassifierBackend;

/// Exercises optional default classifier selection.
///
/// # Returns
/// Summary strings from default classifier lookups.
pub(crate) fn exercise_classifier_defaults() -> Vec<String> {
    let boxed_wrapper =
        BoxMediaStreamClassifier::new(Box::new(FfprobeCommandMediaStreamClassifier::new()));
    let boxed_as_classifier = boxed_wrapper
        .as_ref()
        .classify_content(b"media")
        .is_ok()
        .to_string();
    let boxed_deref = std::ops::Deref::deref(&boxed_wrapper)
        .classify_content(b"media")
        .is_ok()
        .to_string();
    let boxed_content = boxed_wrapper.classify_content(b"media").is_ok().to_string();
    let boxed_file = boxed_wrapper
        .classify_file(Path::new("Cargo.toml"))
        .is_ok()
        .to_string();
    let boxed_inner = boxed_wrapper
        .into_inner()
        .classify_content(b"media")
        .is_ok()
        .to_string();
    let boxed_trait: Box<dyn MediaStreamClassifier> =
        Box::new(FfprobeCommandMediaStreamClassifier::new());
    let boxed_from_trait = BoxMediaStreamClassifier::from(boxed_trait);
    let boxed_into_trait: Box<dyn MediaStreamClassifier> = boxed_from_trait.into();
    let boxed_from_into = boxed_into_trait
        .classify_content(b"media")
        .is_ok()
        .to_string();

    let arc_wrapper =
        ArcMediaStreamClassifier::new(Arc::new(FfprobeCommandMediaStreamClassifier::new()));
    let arc_as_classifier = arc_wrapper
        .as_ref()
        .classify_content(b"media")
        .is_ok()
        .to_string();
    let arc_deref = std::ops::Deref::deref(&arc_wrapper)
        .classify_content(b"media")
        .is_ok()
        .to_string();
    let arc_content = arc_wrapper.classify_content(b"media").is_ok().to_string();
    let arc_file = arc_wrapper
        .classify_file(Path::new("Cargo.toml"))
        .is_ok()
        .to_string();
    let arc_inner = arc_wrapper
        .into_inner()
        .classify_content(b"media")
        .is_ok()
        .to_string();
    let arc_trait: Arc<dyn MediaStreamClassifier> =
        Arc::new(FfprobeCommandMediaStreamClassifier::new());
    let arc_from_trait = ArcMediaStreamClassifier::from(arc_trait);
    let arc_into_trait: Arc<dyn MediaStreamClassifier> = arc_from_trait.into();
    let arc_from_into = arc_into_trait
        .classify_content(b"media")
        .is_ok()
        .to_string();

    vec![
        BoxMediaStreamClassifier::default()
            .classify_content(b"media")
            .is_ok()
            .to_string(),
        BoxMediaStreamClassifier::from_name("ffprobe")
            .is_some()
            .to_string(),
        BoxMediaStreamClassifier::from_name("unknown")
            .is_none()
            .to_string(),
        boxed_as_classifier,
        boxed_deref,
        boxed_content,
        boxed_file,
        boxed_inner,
        boxed_from_into,
        format!("{:?}", MediaStreamClassifierBackend::select("")),
        format!("{:?}", MediaStreamClassifierBackend::select("unknown")),
        format!("{:?}", MediaStreamClassifierBackend::select("ffprobe")),
        ArcMediaStreamClassifier::default()
            .classify_content(b"media")
            .is_ok()
            .to_string(),
        ArcMediaStreamClassifier::from_name("ffprobe")
            .is_some()
            .to_string(),
        ArcMediaStreamClassifier::from_name("unknown")
            .is_none()
            .to_string(),
        arc_as_classifier,
        arc_deref,
        arc_content,
        arc_file,
        arc_inner,
        arc_from_into,
    ]
}
