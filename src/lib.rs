/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # Qubit MIME
//!
//! MIME type detection based on filename glob rules and content magic rules.
//!
//! # Author
//!
//! Haixing Hu

mod magic_value_type;
mod mime_error;
mod mime_glob;
mod mime_magic;
mod mime_magic_matcher;
mod mime_repository;
mod mime_type;
mod mime_type_builder;
mod repository_mime_detector;

pub use magic_value_type::MagicValueType;
pub use mime_error::MimeError;
pub use mime_glob::MimeGlob;
pub use mime_magic::MimeMagic;
pub use mime_magic_matcher::MimeMagicMatcher;
pub use mime_repository::MimeRepository;
pub use mime_type::MimeType;
pub use mime_type_builder::MimeTypeBuilder;
pub use repository_mime_detector::RepositoryMimeDetector;

#[cfg(coverage)]
#[doc(hidden)]
pub mod coverage_support {
    //! Coverage-only hooks for branches that require synthetic failures.

    /// Exercises coverage-only branches across modules.
    ///
    /// # Returns
    /// Summary strings from each exercised branch group.
    pub fn exercise_all() -> Vec<String> {
        let mut result = Vec::new();
        result.extend(crate::magic_value_type::coverage_support::exercise_magic_value_types());
        result.extend(crate::mime_error::coverage_support::exercise_error_builders());
        result.extend(crate::mime_glob::coverage_support::exercise_glob_edges());
        result.extend(crate::mime_magic::coverage_support::exercise_magic_edges());
        result.extend(crate::mime_magic_matcher::coverage_support::exercise_matcher_edges());
        result.extend(crate::mime_repository::coverage_support::exercise_repository_edges());
        result.extend(crate::mime_type::coverage_support::exercise_mime_type_edges());
        result.extend(crate::repository_mime_detector::coverage_support::exercise_detector_edges());
        result.extend(crate::repository_mime_detector::coverage_support::exercise_reader_errors());
        result
    }
}
