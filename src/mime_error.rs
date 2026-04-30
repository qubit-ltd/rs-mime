/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Error type used by MIME database parsing and detection.

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

    /// The XML document could not be parsed.
    #[error("failed to parse MIME XML: {0}")]
    Xml(#[from] roxmltree::Error),

    /// Detection from a path or reader failed due to I/O.
    #[error("I/O error while detecting MIME type: {0}")]
    Io(#[from] std::io::Error),
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
    /// A [`MimeError::InvalidXmlAttribute`] value.
    pub(crate) fn invalid_attr(
        element: &str,
        attribute: &str,
        value: &str,
        reason: impl Into<String>,
    ) -> Self {
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
    /// A [`MimeError::InvalidXmlElement`] value.
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
    /// A [`MimeError::InvalidMagicMatcher`] value.
    pub(crate) fn invalid_matcher(reason: impl Into<String>) -> Self {
        Self::InvalidMagicMatcher {
            reason: reason.into(),
        }
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for error builder branches.

    use super::MimeError;

    /// Exercises internal error constructors.
    ///
    /// # Returns
    /// Display strings for constructed errors.
    pub(crate) fn exercise_error_builders() -> Vec<String> {
        vec![
            MimeError::invalid_attr("match", "value", "bad", "invalid").to_string(),
            MimeError::invalid_element("magic", "missing match").to_string(),
            MimeError::invalid_matcher("bad matcher").to_string(),
        ]
    }
}
