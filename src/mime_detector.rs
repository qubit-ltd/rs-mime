/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Top-level MIME detector interface.

use crate::{ENV_MIME_DETECTOR_DEFAULT, FileCommandMimeDetector, RepositoryMimeDetector};

/// Policy for resolving combined MIME detection from filename and content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeDetectionPolicy {
    /// Prefer a definitive filename result without checking content magic.
    PreferFilename,

    /// Check content magic even when filename detection has a definitive result.
    VerifyContent,
}

impl MimeDetectionPolicy {
    /// Tells whether content magic should be checked for a definitive filename match.
    pub(crate) fn should_verify_content(self) -> bool {
        matches!(self, Self::VerifyContent)
    }
}

/// Detects MIME types from filenames and content.
pub trait MimeDetector {
    /// Detects a MIME type from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    ///
    /// # Returns
    /// First matching MIME type name, or `None`.
    fn detect_by_filename(&self, filename: &str) -> Option<String>;

    /// Detects a MIME type from content bytes.
    ///
    /// # Parameters
    /// - `content`: Content bytes to inspect.
    ///
    /// # Returns
    /// First matching MIME type name, or `None`.
    fn detect_by_content(&self, content: &[u8]) -> Option<String>;

    /// Detects a MIME type from content bytes and an optional filename.
    ///
    /// # Parameters
    /// - `content`: Content bytes to inspect.
    /// - `filename`: Optional file path or basename.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String>;
}

impl Default for Box<dyn MimeDetector> {
    fn default() -> Self {
        default_mime_detector()
    }
}

/// Gets the default MIME detector.
///
/// # Returns
/// A `file` command detector when configured or available; otherwise a
/// repository-backed detector.
pub fn default_mime_detector() -> Box<dyn MimeDetector> {
    let configured = std::env::var(ENV_MIME_DETECTOR_DEFAULT).unwrap_or_default();
    default_mime_detector_from_inputs(&configured, FileCommandMimeDetector::is_available())
}

/// Selects a detector from configuration and backend availability.
///
/// # Parameters
/// - `configured`: Configured detector selector.
/// - `file_command_available`: Whether the `file` backend is available.
///
/// # Returns
/// Selected detector.
fn default_mime_detector_from_inputs(
    configured: &str,
    file_command_available: bool,
) -> Box<dyn MimeDetector> {
    if let Some(detector) = detector_from_name(configured) {
        detector
    } else if file_command_available {
        Box::new(FileCommandMimeDetector::new())
    } else {
        repository_detector()
    }
}

/// Creates a detector from a configured implementation name.
///
/// # Parameters
/// - `name`: Implementation selector.
///
/// # Returns
/// Matching detector, or `None` when the selector is empty or unknown.
fn detector_from_name(name: &str) -> Option<Box<dyn MimeDetector>> {
    match name.to_ascii_lowercase().as_str() {
        "repository" | "repository-mime-detector" => Some(repository_detector()),
        "file" | "file-command" | "file-command-mime-detector" => {
            Some(Box::new(FileCommandMimeDetector::new()))
        }
        _ => None,
    }
}

/// Creates the default repository detector.
///
/// # Returns
/// Boxed repository detector.
fn repository_detector() -> Box<dyn MimeDetector> {
    Box::new(RepositoryMimeDetector::new().expect("embedded MIME repository should parse"))
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for default detector selection.

    use super::{
        MimeDetector, default_mime_detector, default_mime_detector_from_inputs, detector_from_name,
        repository_detector,
    };

    /// Exercises default detector paths and trait default methods.
    ///
    /// # Returns
    /// Summary strings from detector selections.
    pub(crate) fn exercise_detector_defaults() -> Vec<String> {
        let default_detector = default_mime_detector();
        let configured_default = default_mime_detector_from_inputs("repository", false);
        let file_default = default_mime_detector_from_inputs("", true);
        let repository_default = default_mime_detector_from_inputs("", false);
        let file_detector =
            detector_from_name("file").expect("file selector should build a file command detector");
        let named_repository = detector_from_name("repository")
            .expect("repository selector should build a repository detector");
        let repository_detector = repository_detector();
        vec![
            default_detector
                .detect_by_filename("file.pdf")
                .is_some()
                .to_string(),
            configured_default
                .detect_by_filename("file.pdf")
                .is_some()
                .to_string(),
            file_default
                .detect_by_filename("file.pdf")
                .is_some()
                .to_string(),
            repository_default
                .detect_by_filename("file.pdf")
                .is_some()
                .to_string(),
            format!(
                "{:?}",
                repository_detector.detect(
                    b"%PDF-1.7\n",
                    Some("file.bin"),
                    crate::MimeDetectionPolicy::VerifyContent,
                )
            ),
            file_detector
                .detect_by_filename("file.pdf")
                .is_some()
                .to_string(),
            named_repository
                .detect_by_filename("file.pdf")
                .is_some()
                .to_string(),
            detector_from_name("unknown").is_none().to_string(),
            Box::<dyn MimeDetector>::default()
                .detect_by_filename("image.png")
                .is_some()
                .to_string(),
        ]
    }
}
