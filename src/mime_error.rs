// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error type used by MIME database parsing and detection.

use qubit_command::CommandError;
use qubit_config::ConfigError;
use qubit_fs::FsError;
use qubit_local_files::LocalFileError;
use roxmltree::Error as XmlError;
use thiserror::Error;

/// Error type for MIME repository parsing and I/O backed detection.
#[derive(Debug, Error)]
pub enum MimeError {
    /// A glob weight was outside the freedesktop MIME range `0..=100`.
    #[error("invalid MIME glob weight: {weight}")]
    InvalidGlobWeight {
        /// Invalid glob weight.
        weight: u16,
    },

    /// A magic matcher definition is internally inconsistent.
    #[error("invalid MIME magic matcher: {reason}")]
    InvalidMagicMatcher {
        /// Human-readable validation failure.
        reason: String,
    },

    /// An XML attribute is missing or malformed.
    #[error("invalid XML attribute '{attribute}' on <{element}>: '{value}' ({reason})")]
    InvalidXmlAttribute {
        /// Element carrying the invalid attribute.
        element: String,
        /// Invalid attribute name.
        attribute: String,
        /// Invalid attribute value.
        value: String,
        /// Human-readable validation failure.
        reason: String,
    },

    /// An XML element is missing required content or has invalid children.
    #[error("invalid XML element <{element}>: {reason}")]
    InvalidXmlElement {
        /// Invalid element name.
        element: String,
        /// Human-readable validation failure.
        reason: String,
    },

    /// A detector or classifier input cannot be processed.
    #[error("invalid MIME classifier input: {reason}")]
    InvalidClassifierInput {
        /// Human-readable validation failure.
        reason: String,
    },

    /// A detector read path would allocate a buffer above the configured limit.
    #[error(
        "MIME detector buffer allocation exceeds configured limit: requested {requested} bytes, limit {limit} bytes"
    )]
    BufferLimitExceeded {
        /// Requested byte buffer size.
        requested: usize,
        /// Configured maximum byte buffer size.
        limit: usize,
    },

    /// A detector read path could not reserve its requested buffer capacity.
    #[error("MIME detector could not allocate a {requested}-byte buffer: {source}")]
    BufferAllocationFailed {
        /// Requested buffer capacity.
        requested: usize,
        /// Allocation failure reported by `Vec::try_reserve_exact`.
        #[source]
        source: std::collections::TryReserveError,
    },

    /// A detector provider name or alias is already registered.
    #[error("duplicate MIME detector name or alias: {name}")]
    DuplicateDetectorName {
        /// Duplicate provider name or alias.
        name: String,
    },

    /// A detector provider name or alias is empty.
    #[error("MIME detector name must not be empty")]
    EmptyDetectorName,

    /// A detector provider name or alias is malformed.
    #[error("invalid MIME detector name '{name}': {reason}")]
    InvalidDetectorName {
        /// Invalid provider name.
        name: String,
        /// Human-readable validation failure.
        reason: String,
    },

    /// A detector provider could not be found.
    #[error("unknown MIME detector: {name}")]
    UnknownDetector {
        /// Requested provider name or alias.
        name: String,
    },

    /// A detector provider exists but is not available in this environment.
    #[error("MIME detector '{name}' is unavailable: {reason}")]
    DetectorUnavailable {
        /// Requested provider name or alias.
        name: String,
        /// Human-readable unavailability reason.
        reason: String,
    },

    /// No configured detector provider could be created.
    #[error("no available MIME detector: {reason}")]
    NoAvailableDetector {
        /// Human-readable failure summary.
        reason: String,
    },

    /// A detector backend failed with an implementation-specific error.
    #[error("MIME detector backend '{backend}' failed: {reason}")]
    DetectorBackend {
        /// Backend identifier.
        backend: String,
        /// Human-readable failure reason.
        reason: String,
    },

    /// A media stream classifier provider name or alias is already registered.
    #[error("duplicate media stream classifier name or alias: {name}")]
    DuplicateClassifierName {
        /// Duplicate provider name or alias.
        name: String,
    },

    /// A media stream classifier provider name or alias is empty.
    #[error("media stream classifier name must not be empty")]
    EmptyClassifierName,

    /// A media stream classifier provider name or alias is malformed.
    #[error("invalid media stream classifier name '{name}': {reason}")]
    InvalidClassifierName {
        /// Invalid provider name.
        name: String,
        /// Human-readable validation failure.
        reason: String,
    },

    /// A media stream classifier provider could not be found.
    #[error("unknown media stream classifier: {name}")]
    UnknownClassifier {
        /// Requested provider name or alias.
        name: String,
    },

    /// A media stream classifier provider exists but is not available in this
    /// environment.
    #[error("media stream classifier '{name}' is unavailable: {reason}")]
    ClassifierUnavailable {
        /// Requested provider name or alias.
        name: String,
        /// Human-readable unavailability reason.
        reason: String,
    },

    /// No configured media stream classifier provider could be created.
    #[error("no available media stream classifier: {reason}")]
    NoAvailableClassifier {
        /// Human-readable failure summary.
        reason: String,
    },

    /// A media stream classifier backend failed with an implementation-specific
    /// error.
    #[error("media stream classifier backend '{backend}' failed: {reason}")]
    ClassifierBackend {
        /// Backend identifier.
        backend: String,
        /// Human-readable failure reason.
        reason: String,
    },

    /// The XML document could not be parsed.
    #[error("failed to parse MIME XML: {0}")]
    Xml(#[from] XmlError),

    /// Detection from a path or reader failed due to I/O.
    #[error("I/O error while detecting MIME type: {0}")]
    Io(#[from] std::io::Error),

    /// Staging or backend inspection failed and temporary cleanup also failed.
    ///
    /// The primary error retains its original variant and is the source-chain
    /// entry. The independent cleanup error exposes its resource path and
    /// cause.
    #[error("{primary}; temporary-file cleanup failed: {cleanup}")]
    TemporaryCleanup {
        /// Original staging or inspection error.
        #[source]
        primary: Box<MimeError>,
        /// Structured cleanup failure, including the residual resource path.
        cleanup: Box<LocalFileError>,
    },

    /// Detection through a provider-neutral filesystem resource failed.
    #[error("filesystem error while detecting MIME type: {0}")]
    FileSystem(#[from] FsError),

    /// Detection using an external command failed.
    #[error("command error while detecting MIME type: {0}")]
    Command(#[from] CommandError),

    /// Loading MIME configuration failed.
    #[error("configuration error while loading MIME settings: {0}")]
    Config(#[from] ConfigError),
}

impl MimeError {
    /// Builds an invalid XML attribute error.
    ///
    /// # Parameters
    /// - `element`: Element carrying the attribute.
    /// - `attribute`: Attribute name.
    /// - `value`: Attribute value.
    /// - `reason`: Why the value is invalid.
    ///
    /// # Returns
    /// A [`MimeError::InvalidXmlAttribute`](crate::MimeError::InvalidXmlAttribute) value.
    pub(crate) fn invalid_attr(element: &str, attribute: &str, value: &str, reason: impl Into<String>) -> Self {
        Self::InvalidXmlAttribute {
            element: element.to_owned(),
            attribute: attribute.to_owned(),
            value: value.to_owned(),
            reason: reason.into(),
        }
    }

    /// Builds an invalid XML element error.
    ///
    /// # Parameters
    /// - `element`: Invalid element name.
    /// - `reason`: Why the element is invalid.
    ///
    /// # Returns
    /// A [`MimeError::InvalidXmlElement`](crate::MimeError::InvalidXmlElement)
    /// value.
    pub(crate) fn invalid_element(element: &str, reason: impl Into<String>) -> Self {
        Self::InvalidXmlElement {
            element: element.to_owned(),
            reason: reason.into(),
        }
    }

    /// Builds an invalid magic matcher error.
    ///
    /// # Parameters
    /// - `reason`: Why the matcher is invalid.
    ///
    /// # Returns
    /// A [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher) value.
    pub(crate) fn invalid_matcher(reason: impl Into<String>) -> Self {
        Self::InvalidMagicMatcher { reason: reason.into() }
    }

    /// Builds an invalid classifier input error.
    ///
    /// # Parameters
    /// - `reason`: Why the input cannot be classified.
    ///
    /// # Returns
    /// A [`MimeError::InvalidClassifierInput`](crate::MimeError::InvalidClassifierInput) value.
    pub(crate) fn invalid_classifier_input(reason: impl Into<String>) -> Self {
        Self::InvalidClassifierInput { reason: reason.into() }
    }

    /// Builds a detector backend error.
    ///
    /// # Parameters
    /// - `backend`: Detector backend identifier.
    /// - `reason`: Why the backend failed.
    ///
    /// # Returns
    /// A [`MimeError::DetectorBackend`](crate::MimeError::DetectorBackend)
    /// value.
    pub fn detector_backend(backend: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::DetectorBackend {
            backend: backend.into(),
            reason: reason.into(),
        }
    }
}
