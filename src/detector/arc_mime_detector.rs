/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Shared MIME detector wrapper.

use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use qubit_io::ReadSeek;

use crate::{
    BoxMimeDetector,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeResult,
};

use super::MimeDetectorRegistry;

/// A MIME detector stored in an [`Arc`].
#[derive(Debug, Clone)]
pub struct ArcMimeDetector {
    inner: Arc<dyn MimeDetector>,
}

impl ArcMimeDetector {
    /// Wraps an existing shared MIME detector.
    ///
    /// # Parameters
    /// - `detector`: Detector to wrap.
    ///
    /// # Returns
    /// Wrapped shared detector.
    pub fn new(detector: Arc<dyn MimeDetector>) -> Self {
        Self { inner: detector }
    }

    /// Creates a shared detector from an implementation name.
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

    /// Creates a shared detector from a built-in implementation name and config.
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
        BoxMimeDetector::from_name_with_config(name, config).map(Self::from_box)
    }

    /// Creates a shared detector from a registry provider name.
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
        BoxMimeDetector::from_registry_name(registry, name, config).map(Self::from_box)
    }

    /// Creates a shared detector from MIME configuration.
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
        BoxMimeDetector::from_config(config).map(Self::from_box)
    }

    /// Creates a shared detector from MIME configuration and explicit registry.
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
        BoxMimeDetector::from_registry(registry, config).map(Self::from_box)
    }

    /// Creates a shared detector from a boxed wrapper.
    ///
    /// # Parameters
    /// - `detector`: Boxed detector wrapper.
    ///
    /// # Returns
    /// Shared detector wrapper.
    fn from_box(detector: BoxMimeDetector) -> Self {
        let boxed = detector.into_inner();
        Self::new(Arc::from(boxed))
    }

    /// Unwraps this wrapper into the inner shared detector.
    ///
    /// # Returns
    /// Inner shared detector.
    pub fn into_inner(self) -> Arc<dyn MimeDetector> {
        self.inner
    }
}

impl Deref for ArcMimeDetector {
    type Target = dyn MimeDetector;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl AsRef<dyn MimeDetector> for ArcMimeDetector {
    fn as_ref(&self) -> &(dyn MimeDetector + 'static) {
        self.inner.as_ref()
    }
}

impl From<Arc<dyn MimeDetector>> for ArcMimeDetector {
    fn from(detector: Arc<dyn MimeDetector>) -> Self {
        Self::new(detector)
    }
}

impl From<ArcMimeDetector> for Arc<dyn MimeDetector> {
    fn from(detector: ArcMimeDetector) -> Self {
        detector.into_inner()
    }
}

impl MimeDetector for ArcMimeDetector {
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
