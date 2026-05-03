/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! MIME content magic matcher.

use crate::{MagicValueType, MimeError, MimeResult};

/// A single MIME magic matcher, optionally with nested sub-matchers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeMagicMatcher {
    value_type: MagicValueType,
    offset_begin: usize,
    offset_end: usize,
    value: Vec<u8>,
    mask: Option<Vec<u8>>,
    sub_matchers: Vec<MimeMagicMatcher>,
}

impl MimeMagicMatcher {
    /// Creates a magic matcher.
    ///
    /// # Parameters
    /// - `value_type`: Matcher value type.
    /// - `offset_begin`: First byte offset where the value may match.
    /// - `offset_end`: Last byte offset where the value may match.
    /// - `value`: Expected value bytes. Numeric values must be stored big-endian.
    /// - `mask`: Optional mask bytes with the same length as `value`.
    /// - `sub_matchers`: Nested matchers; at least one child must match after the parent.
    ///
    /// # Returns
    /// A validated [`MimeMagicMatcher`].
    ///
    /// # Errors
    /// Returns [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher) when offsets are inverted,
    /// when numeric value widths are wrong, or when the mask length differs
    /// from the value length.
    pub fn new(
        value_type: MagicValueType,
        offset_begin: usize,
        offset_end: usize,
        value: Vec<u8>,
        mask: Option<Vec<u8>>,
        sub_matchers: Vec<MimeMagicMatcher>,
    ) -> MimeResult<Self> {
        validate_offsets(offset_begin, offset_end)?;
        validate_value_width(value_type, &value)?;
        validate_mask_width(&value, mask.as_deref())?;
        Ok(Self {
            value_type,
            offset_begin,
            offset_end,
            value,
            mask,
            sub_matchers,
        })
    }

    /// Gets the matcher value type.
    ///
    /// # Returns
    /// The value type used by this matcher.
    pub fn value_type(&self) -> MagicValueType {
        self.value_type
    }

    /// Gets the first offset that may match.
    ///
    /// # Returns
    /// Inclusive starting offset.
    pub fn offset_begin(&self) -> usize {
        self.offset_begin
    }

    /// Gets the last offset that may match.
    ///
    /// # Returns
    /// Inclusive ending offset.
    pub fn offset_end(&self) -> usize {
        self.offset_end
    }

    /// Gets the expected value bytes.
    ///
    /// # Returns
    /// Expected value bytes. Numeric values are stored big-endian.
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Gets the optional mask bytes.
    ///
    /// # Returns
    /// `Some` mask bytes when the matcher masks input bytes before comparing,
    /// otherwise `None`.
    pub fn mask(&self) -> Option<&[u8]> {
        self.mask.as_deref()
    }

    /// Gets nested sub-matchers.
    ///
    /// # Returns
    /// Child matchers evaluated after this matcher succeeds.
    pub fn sub_matchers(&self) -> &[MimeMagicMatcher] {
        &self.sub_matchers
    }

    /// Gets the maximum number of bytes needed to evaluate this matcher.
    ///
    /// # Returns
    /// The highest tested offset plus value width, including sub-matchers.
    pub fn max_test_bytes(&self) -> usize {
        let own_bytes = self.offset_end.saturating_add(self.value.len());
        self.sub_matchers
            .iter()
            .map(MimeMagicMatcher::max_test_bytes)
            .max()
            .map_or(own_bytes, |child_bytes| own_bytes.max(child_bytes))
    }

    /// Tests whether this matcher matches a content buffer.
    ///
    /// # Parameters
    /// - `bytes`: Content bytes to test.
    ///
    /// # Returns
    /// `true` when this matcher matches and, if present, at least one nested
    /// matcher also matches.
    pub fn matches(&self, bytes: &[u8]) -> bool {
        let parent_matches = match self.value_type {
            MagicValueType::String => self.matches_bytes(bytes, &self.value, self.mask.as_deref()),
            MagicValueType::Byte => self.matches_bytes(bytes, &self.value, self.mask.as_deref()),
            MagicValueType::Host16
            | MagicValueType::Host32
            | MagicValueType::Big16
            | MagicValueType::Big32
            | MagicValueType::Little16
            | MagicValueType::Little32 => {
                let value = ordered_numeric_bytes(self.value_type, &self.value);
                let mask = self
                    .mask
                    .as_deref()
                    .map(|mask| ordered_numeric_bytes(self.value_type, mask));
                self.matches_bytes(bytes, &value, mask.as_deref())
            }
        };
        if !parent_matches {
            return false;
        }
        self.sub_matchers.is_empty()
            || self
                .sub_matchers
                .iter()
                .any(|sub_matcher| sub_matcher.matches(bytes))
    }

    /// Tests raw value bytes over this matcher's offset range.
    ///
    /// # Parameters
    /// - `bytes`: Content bytes.
    /// - `value`: Value bytes in the byte order used for matching.
    /// - `mask`: Optional mask bytes in the same order as `value`.
    ///
    /// # Returns
    /// `true` when any allowed offset matches.
    fn matches_bytes(&self, bytes: &[u8], value: &[u8], mask: Option<&[u8]>) -> bool {
        if value.is_empty() || bytes.len() < value.len() || self.offset_begin >= bytes.len() {
            return false;
        }
        let last_possible = bytes.len() - value.len();
        let end = self.offset_end.min(last_possible);
        if self.offset_begin > end {
            return false;
        }
        (self.offset_begin..=end).any(|offset| value_matches_at(bytes, offset, value, mask))
    }
}

/// Validates matcher offsets.
///
/// # Parameters
/// - `offset_begin`: First allowed offset.
/// - `offset_end`: Last allowed offset.
///
/// # Errors
/// Returns [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher) when the range is inverted.
fn validate_offsets(offset_begin: usize, offset_end: usize) -> MimeResult<()> {
    if offset_begin > offset_end {
        return Err(MimeError::invalid_matcher(
            "offset begin must not be greater than offset end",
        ));
    }
    Ok(())
}

/// Validates value width for numeric matchers.
///
/// # Parameters
/// - `value_type`: Matcher value type.
/// - `value`: Value bytes to validate.
///
/// # Errors
/// Returns [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher) when a numeric value has the wrong width.
fn validate_value_width(value_type: MagicValueType, value: &[u8]) -> MimeResult<()> {
    if value.is_empty() {
        return Err(MimeError::invalid_matcher(
            "magic matcher value must not be empty",
        ));
    }
    if let Some(width) = value_type.numeric_width()
        && value.len() != width
    {
        return Err(MimeError::invalid_matcher(format!(
            "{} matcher requires {width} value byte(s)",
            value_type.name()
        )));
    }
    Ok(())
}

/// Validates that an optional mask is aligned with the value.
///
/// # Parameters
/// - `value`: Matcher value bytes.
/// - `mask`: Optional mask bytes.
///
/// # Errors
/// Returns [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher) when the mask length differs from value length.
fn validate_mask_width(value: &[u8], mask: Option<&[u8]>) -> MimeResult<()> {
    if let Some(mask) = mask
        && mask.len() != value.len()
    {
        return Err(MimeError::invalid_matcher(
            "magic matcher mask length must match value length",
        ));
    }
    Ok(())
}

/// Orders numeric bytes for matching.
///
/// # Parameters
/// - `value_type`: Numeric matcher type.
/// - `bytes`: Big-endian bytes stored in the matcher.
///
/// # Returns
/// Bytes in the order expected in the input file.
fn ordered_numeric_bytes(value_type: MagicValueType, bytes: &[u8]) -> Vec<u8> {
    if value_type.uses_little_endian_order() {
        bytes.iter().rev().copied().collect()
    } else {
        bytes.to_vec()
    }
}

/// Tests whether value bytes match at a specific offset.
///
/// # Parameters
/// - `bytes`: Content bytes.
/// - `offset`: Starting offset.
/// - `value`: Expected bytes.
/// - `mask`: Optional mask bytes.
///
/// # Returns
/// `true` when the byte range matches.
fn value_matches_at(bytes: &[u8], offset: usize, value: &[u8], mask: Option<&[u8]>) -> bool {
    match mask {
        Some(mask) => {
            value
                .iter()
                .zip(mask.iter())
                .enumerate()
                .all(|(index, (value_byte, mask_byte))| {
                    (bytes[offset + index] & mask_byte) == (*value_byte & mask_byte)
                })
        }
        None => value
            .iter()
            .enumerate()
            .all(|(index, value_byte)| bytes[offset + index] == *value_byte),
    }
}

// qubit-style: allow coverage-cfg
#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for magic matcher branches.

    use crate::MagicValueType;

    use super::MimeMagicMatcher;

    /// Exercises numeric matchers, getters, and validation failures.
    ///
    /// # Returns
    /// Match and validation summaries.
    pub(crate) fn exercise_matcher_edges() -> Vec<String> {
        let big16 = MimeMagicMatcher::new(
            MagicValueType::Big16,
            0,
            0,
            0x1234u16.to_be_bytes().to_vec(),
            None,
            vec![],
        )
        .expect("big16 matcher should be valid");
        let big32 = MimeMagicMatcher::new(
            MagicValueType::Big32,
            0,
            0,
            0x12345678u32.to_be_bytes().to_vec(),
            Some(0xfffffff0u32.to_be_bytes().to_vec()),
            vec![],
        )
        .expect("big32 matcher should be valid");
        let host16 = MimeMagicMatcher::new(
            MagicValueType::Host16,
            0,
            0,
            0x1234u16.to_be_bytes().to_vec(),
            None,
            vec![],
        )
        .expect("host16 matcher should be valid");
        let host32 = MimeMagicMatcher::new(
            MagicValueType::Host32,
            0,
            0,
            0x12345678u32.to_be_bytes().to_vec(),
            None,
            vec![],
        )
        .expect("host32 matcher should be valid");
        let empty_value_error =
            MimeMagicMatcher::new(MagicValueType::String, 0, 0, Vec::new(), None, vec![])
                .expect_err("empty value should fail")
                .to_string();
        let width_error = MimeMagicMatcher::new(MagicValueType::Big16, 0, 0, vec![0], None, vec![])
            .expect_err("bad width should fail")
            .to_string();
        let mask_error = MimeMagicMatcher::new(
            MagicValueType::String,
            0,
            0,
            b"ABC".to_vec(),
            Some(vec![0xff]),
            vec![],
        )
        .expect_err("bad mask width should fail")
        .to_string();
        vec![
            big16.value_type().name().to_owned(),
            big16.offset_begin().to_string(),
            big16.offset_end().to_string(),
            big16.value().len().to_string(),
            big16.mask().is_none().to_string(),
            big16.sub_matchers().len().to_string(),
            big16.matches(&0x1234u16.to_be_bytes()).to_string(),
            big16.matches(&[0]).to_string(),
            big32.matches(&[0x12, 0x34, 0x56, 0x70]).to_string(),
            host16.matches(&ordered_host16_bytes(0x1234)).to_string(),
            host32
                .matches(&ordered_host32_bytes(0x12345678))
                .to_string(),
            empty_value_error,
            width_error,
            mask_error,
        ]
    }

    /// Gets host-endian bytes for a 16-bit integer.
    ///
    /// # Parameters
    /// - `value`: Integer value.
    ///
    /// # Returns
    /// Bytes in native endian order.
    fn ordered_host16_bytes(value: u16) -> [u8; 2] {
        if cfg!(target_endian = "little") {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    }

    /// Gets host-endian bytes for a 32-bit integer.
    ///
    /// # Parameters
    /// - `value`: Integer value.
    ///
    /// # Returns
    /// Bytes in native endian order.
    fn ordered_host32_bytes(value: u32) -> [u8; 4] {
        if cfg!(target_endian = "little") {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    }
}
