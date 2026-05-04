/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Provider contract for pluggable MIME detector implementations.
//!
//! A provider gives the registry a stable name, optional aliases, availability
//! checks, and a factory method for creating one detector instance. This keeps
//! detector selection independent from the concrete detector type, so
//! applications can register their own backend without changing
//! [`BoxMimeDetector`](crate::BoxMimeDetector) or
//! [`ArcMimeDetector`](crate::ArcMimeDetector).

use std::fmt::Debug;

use crate::{
    MimeConfig,
    MimeDetector,
    MimeResult,
};

use super::MimeDetectorAvailability;

/// Factory contract for MIME detector implementations.
///
/// Provider ids and aliases are matched case-insensitively by
/// [`MimeDetectorRegistry`](crate::MimeDetectorRegistry). Returning
/// [`MimeDetectorAvailability::Unavailable`] lets a provider participate in
/// fallback selection without failing the whole registry when an optional
/// backend, such as an external command, is not installed.
///
/// # Examples
///
/// Register a custom provider and create it by alias:
///
/// ```rust
/// use std::path::Path;
///
/// use qubit_io::ReadSeek;
/// use qubit_mime::{
///     MimeConfig,
///     MimeDetectionPolicy,
///     MimeDetector,
///     MimeDetectorProvider,
///     MimeDetectorRegistry,
///     MimeResult,
/// };
///
/// #[derive(Debug)]
/// struct StaticDetector;
///
/// impl MimeDetector for StaticDetector {
///     fn detect_by_filename(&self, filename: &str) -> Option<String> {
///         filename
///             .ends_with(".static")
///             .then(|| "application/x-static".to_owned())
///     }
///
///     fn detect_by_content(&self, _content: &[u8]) -> Option<String> {
///         None
///     }
///
///     fn detect(
///         &self,
///         _content: &[u8],
///         filename: Option<&str>,
///         _policy: MimeDetectionPolicy,
///     ) -> Option<String> {
///         filename.and_then(|name| self.detect_by_filename(name))
///     }
///
///     fn detect_reader(
///         &self,
///         _reader: &mut dyn ReadSeek,
///         filename: Option<&str>,
///         policy: MimeDetectionPolicy,
///     ) -> MimeResult<Option<String>> {
///         Ok(self.detect(&[], filename, policy))
///     }
///
///     fn detect_file(
///         &self,
///         file: &Path,
///         policy: MimeDetectionPolicy,
///     ) -> MimeResult<Option<String>> {
///         Ok(self.detect(
///             &[],
///             file.file_name().and_then(|name| name.to_str()),
///             policy,
///         ))
///     }
/// }
///
/// #[derive(Debug)]
/// struct StaticProvider;
///
/// impl MimeDetectorProvider for StaticProvider {
///     fn id(&self) -> &'static str {
///         "static"
///     }
///
///     fn aliases(&self) -> &'static [&'static str] {
///         &["static-detector"]
///     }
///
///     fn create(&self, _config: &MimeConfig) -> MimeResult<Box<dyn MimeDetector>> {
///         Ok(Box::new(StaticDetector))
///     }
/// }
///
/// # fn main() -> MimeResult<()> {
/// let mut registry = MimeDetectorRegistry::new();
/// registry.register(StaticProvider)?;
///
/// let detector = registry.create("static-detector", &MimeConfig::default())?;
/// assert_eq!(
///     Some("application/x-static".to_owned()),
///     detector.detect_by_filename("sample.static"),
/// );
/// # Ok(())
/// # }
/// ```
pub trait MimeDetectorProvider: Debug + Send + Sync {
    /// Gets the canonical provider identifier.
    ///
    /// # Returns
    /// Stable lowercase provider identifier.
    fn id(&self) -> &'static str;

    /// Gets additional names accepted for this provider.
    ///
    /// # Returns
    /// Alias names. Matching is case-insensitive.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Gets provider priority used by `auto` selection.
    ///
    /// # Returns
    /// Larger values are preferred.
    fn priority(&self) -> i32 {
        0
    }

    /// Checks whether this provider can create a detector.
    ///
    /// # Parameters
    /// - `config`: MIME configuration used for provider-specific checks.
    ///
    /// # Returns
    /// Provider availability.
    fn availability(&self, _config: &MimeConfig) -> MimeDetectorAvailability {
        MimeDetectorAvailability::Available
    }

    /// Creates a detector instance.
    ///
    /// # Parameters
    /// - `config`: MIME configuration used to initialize the detector.
    ///
    /// # Returns
    /// Boxed detector implementation.
    ///
    /// # Errors
    /// Returns a [`MimeError`](crate::MimeError) when initialization fails.
    fn create(&self, config: &MimeConfig) -> MimeResult<Box<dyn MimeDetector>>;
}
