/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
#![allow(dead_code)]
//! Coverage helpers for default detector selection.

use super::mime_detector::MimeDetector;
use super::mime_detector_backend::MimeDetectorBackend;
use crate::{ArcMimeDetector, BoxMimeDetector};

/// Exercises default detector paths and trait default methods.
///
/// # Returns
/// Summary strings from detector selections.
pub(crate) fn exercise_detector_defaults() -> Vec<String> {
    let default_detector = BoxMimeDetector::default();
    let configured_default =
        BoxMimeDetector::from_name("repository").expect("repository selector should resolve");
    let file_default =
        ArcMimeDetector::from_name("file").expect("file selector should resolve");
    let repository_default =
        ArcMimeDetector::from_name("repository").expect("repository selector should resolve");
    vec![
        default_detector
            .detect_by_filename("file.pdf")
            .is_some()
            .to_string(),
        configured_default
            .detect_by_filename("file.pdf")
            .is_some()
            .to_string(),
        format!("{:?}", MimeDetectorBackend::select("", true)),
        format!("{:?}", MimeDetectorBackend::select("", false)),
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
    ]
}

