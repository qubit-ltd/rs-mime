/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Coverage helpers for default detector selection.

use std::sync::Arc;

use super::mime_detector::MimeDetector;
use super::mime_detector_backend::MimeDetectorBackend;
use crate::{ArcMimeDetector, BoxMimeDetector, MimeDetectionPolicy, RepositoryMimeDetector};

/// Exercises default detector paths and trait default methods.
///
/// # Returns
/// Summary strings from detector selections.
pub(crate) fn exercise_detector_defaults() -> Vec<String> {
    let default_detector = BoxMimeDetector::default();
    let configured_default =
        BoxMimeDetector::from_name("repository").expect("repository selector should resolve");
    let boxed_file_default =
        BoxMimeDetector::from_name("file").expect("file selector should resolve");
    let file_default = ArcMimeDetector::from_name("file").expect("file selector should resolve");
    let repository_default =
        ArcMimeDetector::from_name("repository").expect("repository selector should resolve");

    let boxed_wrapper = BoxMimeDetector::new(Box::new(RepositoryMimeDetector::default()));
    let boxed_as_detector = boxed_wrapper
        .as_detector()
        .detect_by_filename("file.pdf")
        .is_some()
        .to_string();
    let boxed_deref = std::ops::Deref::deref(&boxed_wrapper)
        .detect_by_content(b"%PDF-1.7\n")
        .is_some()
        .to_string();
    let boxed_content = boxed_wrapper
        .detect_by_content(b"%PDF-1.7\n")
        .is_some()
        .to_string();
    let boxed_detect = boxed_wrapper
        .detect(
            b"%PDF-1.7\n",
            Some("file.pdf"),
            MimeDetectionPolicy::VerifyContent,
        )
        .is_some()
        .to_string();
    let boxed_inner = boxed_wrapper
        .into_inner()
        .detect_by_filename("file.pdf")
        .is_some()
        .to_string();
    let boxed_trait: Box<dyn MimeDetector> = Box::new(RepositoryMimeDetector::default());
    let boxed_from_trait = BoxMimeDetector::from(boxed_trait);
    let boxed_into_trait: Box<dyn MimeDetector> = boxed_from_trait.into();
    let boxed_from_into = boxed_into_trait
        .detect_by_filename("file.pdf")
        .is_some()
        .to_string();

    let arc_wrapper = ArcMimeDetector::new(Arc::new(RepositoryMimeDetector::default()));
    let arc_as_detector = arc_wrapper
        .as_detector()
        .detect_by_filename("file.pdf")
        .is_some()
        .to_string();
    let arc_deref = std::ops::Deref::deref(&arc_wrapper)
        .detect_by_content(b"%PDF-1.7\n")
        .is_some()
        .to_string();
    let arc_content = arc_wrapper
        .detect_by_content(b"%PDF-1.7\n")
        .is_some()
        .to_string();
    let arc_detect = arc_wrapper
        .detect(
            b"%PDF-1.7\n",
            Some("file.pdf"),
            MimeDetectionPolicy::VerifyContent,
        )
        .is_some()
        .to_string();
    let arc_inner = arc_wrapper
        .into_inner()
        .detect_by_filename("file.pdf")
        .is_some()
        .to_string();
    let arc_trait: Arc<dyn MimeDetector> = Arc::new(RepositoryMimeDetector::default());
    let arc_from_trait = ArcMimeDetector::from(arc_trait);
    let arc_into_trait: Arc<dyn MimeDetector> = arc_from_trait.into();
    let arc_from_into = arc_into_trait
        .detect_by_filename("file.pdf")
        .is_some()
        .to_string();

    vec![
        default_detector
            .detect_by_filename("file.pdf")
            .is_some()
            .to_string(),
        configured_default
            .detect_by_filename("file.pdf")
            .is_some()
            .to_string(),
        boxed_file_default
            .detect_by_filename("file.pdf")
            .is_some()
            .to_string(),
        boxed_as_detector,
        boxed_deref,
        boxed_content,
        boxed_detect,
        boxed_inner,
        boxed_from_into,
        format!("{:?}", MimeDetectorBackend::select("", true)),
        format!("{:?}", MimeDetectorBackend::select("", false)),
        format!("{:?}", MimeDetectorBackend::select("repository", true)),
        file_default
            .detect_by_filename("file.pdf")
            .is_some()
            .to_string(),
        repository_default
            .detect_by_filename("file.pdf")
            .is_some()
            .to_string(),
        MimeDetectorBackend::from_name("unknown")
            .is_none()
            .to_string(),
        ArcMimeDetector::default()
            .detect_by_filename("image.png")
            .is_some()
            .to_string(),
        arc_as_detector,
        arc_deref,
        arc_content,
        arc_detect,
        arc_inner,
        arc_from_into,
    ]
}
