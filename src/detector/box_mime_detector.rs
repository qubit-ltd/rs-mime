/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Boxed MIME detector wrapper.
//!
//! [`BoxMimeDetector`] is the owning detector wrapper used when callers want a
//! single dynamically selected detector instance. It delegates construction to a
//! [`MimeDetectorRegistry`], so the same wrapper can be backed by built-in
//! providers or by application-provided detectors.
//!
//! # Examples
//!
//! Create a detector from the built-in registry and default configuration:
//!
//! ```rust
//! use qubit_mime::{
//!     BoxMimeDetector,
//!     MimeConfig,
//!     MimeDetector,
//!     MimeResult,
//! };
//!
//! # fn main() -> MimeResult<()> {
//! let detector = BoxMimeDetector::from_config(&MimeConfig::default())?;
//! assert_eq!(
//!     Some("application/json".to_owned()),
//!     detector.detect_by_filename("payload.json"),
//! );
//! # Ok(())
//! # }
//! ```

use std::ops::Deref;
use std::path::Path;

use qubit_io::ReadSeek;

use crate::{
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeResult,
};

use super::MimeDetectorRegistry;

/// A MIME detector stored in a [`Box`].
#[derive(Debug)]
pub struct BoxMimeDetector {
    inner: Box<dyn MimeDetector>,
}

impl BoxMimeDetector {
    /// Wraps an existing boxed MIME detector.
    ///
    /// # Parameters
    /// - `detector`: Detector to wrap.
    ///
    /// # Returns
    /// Wrapped boxed detector.
    pub fn new(detector: Box<dyn MimeDetector>) -> Self {
        Self { inner: detector }
    }

    /// Creates a boxed detector from an implementation name.
    ///
    /// # Parameters
    /// - `name`: Detector selector.
    ///
    /// # Returns
    /// Matching detector from the built-in registry.
    ///
    /// # Errors
    /// Returns a [`MimeError`](crate::MimeError) when no built-in provider
    /// matches `name`, when the provider is unavailable, or initialization
    /// fails.
    pub fn from_name(name: &str) -> MimeResult<Self> {
        Self::from_name_with_config(name, &MimeConfig::default())
    }

    /// Creates a boxed detector from a built-in implementation name and config.
    ///
    /// # Parameters
    /// - `name`: Detector selector.
    /// - `config`: MIME configuration passed to the provider.
    ///
    /// # Returns
    /// Matching detector from the built-in registry.
    ///
    /// # Errors
    /// Returns a [`MimeError`](crate::MimeError) when no built-in provider
    /// matches `name`, when the provider is unavailable, or initialization
    /// fails.
    pub fn from_name_with_config(name: &str, config: &MimeConfig) -> MimeResult<Self> {
        MimeDetectorRegistry::with_builtin().create(name, config)
    }

    /// Creates a boxed detector from a registry provider name.
    ///
    /// # Parameters
    /// - `registry`: Registry containing available providers.
    /// - `name`: Detector selector.
    /// - `config`: MIME configuration passed to the provider.
    ///
    /// # Returns
    /// Matching detector from `registry`.
    ///
    /// # Errors
    /// Returns a [`MimeError`](crate::MimeError) when no provider matches
    /// `name`, when the provider is unavailable, or initialization fails.
    pub fn from_registry_name(
        registry: &MimeDetectorRegistry,
        name: &str,
        config: &MimeConfig,
    ) -> MimeResult<Self> {
        registry.create(name, config)
    }

    /// Creates a boxed detector from MIME configuration.
    ///
    /// The built-in registry is used. The configured default selector is tried
    /// first unless it is empty or `auto`; configured fallbacks are tried only
    /// when an explicit default cannot be created.
    ///
    /// # Parameters
    /// - `config`: MIME configuration containing the default detector selector.
    ///
    /// # Returns
    /// Configured detector wrapper from the built-in registry.
    ///
    /// # Errors
    /// Returns a [`MimeError`](crate::MimeError) when the configured detector
    /// cannot be created.
    pub fn from_config(config: &MimeConfig) -> MimeResult<Self> {
        Self::from_registry(&MimeDetectorRegistry::with_builtin(), config)
    }

    /// Creates a boxed detector from MIME configuration and explicit registry.
    ///
    /// Use this constructor when an application has registered custom detector
    /// providers or wants to restrict the available providers. Resolution uses
    /// the same default, `auto`, and fallback semantics as
    /// [`MimeDetectorRegistry::create_default`].
    ///
    /// # Parameters
    /// - `registry`: Registry containing available providers.
    /// - `config`: MIME configuration containing the default detector selector
    ///   and fallback chain.
    ///
    /// # Returns
    /// Configured detector wrapper from `registry`.
    ///
    /// # Errors
    /// Returns a [`MimeError`](crate::MimeError) when no configured provider can
    /// be created.
    pub fn from_registry(registry: &MimeDetectorRegistry, config: &MimeConfig) -> MimeResult<Self> {
        registry.create_default(config)
    }

    /// Unwraps this wrapper into the inner boxed detector.
    ///
    /// # Returns
    /// Inner boxed detector.
    pub fn into_inner(self) -> Box<dyn MimeDetector> {
        self.inner
    }
}

impl Deref for BoxMimeDetector {
    type Target = dyn MimeDetector;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl AsRef<dyn MimeDetector> for BoxMimeDetector {
    fn as_ref(&self) -> &(dyn MimeDetector + 'static) {
        self.inner.as_ref()
    }
}

impl From<Box<dyn MimeDetector>> for BoxMimeDetector {
    fn from(detector: Box<dyn MimeDetector>) -> Self {
        Self::new(detector)
    }
}

impl From<BoxMimeDetector> for Box<dyn MimeDetector> {
    fn from(detector: BoxMimeDetector) -> Self {
        detector.into_inner()
    }
}

impl MimeDetector for BoxMimeDetector {
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        self.inner.detect_by_filename(filename)
    }

    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        self.inner.detect_by_content(content)
    }

    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        self.inner.detect(content, filename, policy)
    }

    fn detect_reader(
        &self,
        reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.inner.detect_reader(reader, filename, policy)
    }

    fn detect_file(&self, file: &Path, policy: MimeDetectionPolicy) -> MimeResult<Option<String>> {
        self.inner.detect_file(file, policy)
    }
}
