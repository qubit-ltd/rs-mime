/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Shared media stream classifier helpers.

use std::fs;
use std::path::Path;

use crate::{MimeError, MimeResult};

/// Common validation and staging helpers for media stream classifiers.
#[derive(Debug, Clone, Copy, Default)]
pub struct AbstractMediaStreamClassifier;

impl AbstractMediaStreamClassifier {
    /// Validates that a path is a readable local file.
    ///
    /// # Parameters
    /// - `path`: Path to validate.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when metadata cannot be read or the path is
    /// not a regular file.
    pub fn validate_readable_file(path: &Path) -> MimeResult<()> {
        let metadata = fs::metadata(path)?;
        if metadata.is_file() {
            Ok(())
        } else {
            Err(MimeError::invalid_classifier_input(format!(
                "path is not a regular file: {}",
                path.display()
            )))
        }
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for classifier validation.

    use std::path::Path;

    use super::AbstractMediaStreamClassifier;

    /// Exercises readable-file validation paths.
    ///
    /// # Returns
    /// Summary strings from validation.
    pub(crate) fn exercise_abstract_classifier_edges() -> Vec<String> {
        let valid = AbstractMediaStreamClassifier::validate_readable_file(Path::new("Cargo.toml"))
            .is_ok()
            .to_string();
        let invalid = AbstractMediaStreamClassifier::validate_readable_file(Path::new("."))
            .expect_err("directory should not validate")
            .to_string();
        vec![valid, invalid]
    }
}
