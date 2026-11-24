// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! MIME content magic matcher.

use qubit_operation::OperationScope;
use qubit_operation::StopReason;

use crate::MagicValueType;
use crate::MimeError;
use crate::MimeResult;

/// Maximum number of matcher work units evaluated before yielding.
pub(crate) const MATCH_WORK_PER_BATCH: usize = 4096;

/// Tracks matcher work across repository rules and matcher nodes.
#[derive(Debug, Default)]
pub(crate) struct MatcherWorkBudget {
    work_in_poll: usize,
    maximum_work_in_poll: usize,
    total_work: usize,
    yields: usize,
}

impl MatcherWorkBudget {
    /// Counts one unit of matching work and reports whether the batch is full.
    pub(crate) fn charge(&mut self) -> bool {
        self.work_in_poll += 1;
        self.total_work += 1;
        if self.work_in_poll == MATCH_WORK_PER_BATCH {
            self.maximum_work_in_poll = self.maximum_work_in_poll.max(self.work_in_poll);
            self.work_in_poll = 0;
            self.yields += 1;
            true
        } else {
            false
        }
    }

    /// Records the final partial batch after matching completes.
    pub(crate) fn finish(&mut self) {
        self.maximum_work_in_poll = self.maximum_work_in_poll.max(self.work_in_poll);
    }

    /// Returns the largest work batch observed by this matcher operation.
    #[cfg(test)]
    pub(crate) fn maximum_work_in_poll(&self) -> usize {
        self.maximum_work_in_poll
    }

    /// Returns total counted work units.
    #[cfg(test)]
    pub(crate) fn total_work(&self) -> usize {
        self.total_work
    }

    /// Returns the number of cooperative yields caused by matcher work.
    #[cfg(test)]
    pub(crate) fn yields(&self) -> usize {
        self.yields
    }
}

/// Yields after a complete work batch and reports an accepted stop reason.
pub(crate) async fn yield_after_work(scope: &OperationScope) -> Result<(), StopReason> {
    yield_to_executor().await;
    match scope.context().stop_reason() {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

/// Maximum number of nested magic matchers, counting each root as level one.
pub(super) const MAX_MAGIC_DEPTH: usize = 128;

/// A single MIME magic matcher, optionally with nested sub-matchers.
///
/// # Examples
///
/// ```
/// use qubit_mime::{MagicValueType, MimeMagicMatcher};
/// let matcher = MimeMagicMatcher::new(MagicValueType::Byte, 0, 0, vec![0x89], None, vec![])?;
/// assert!(matcher.matches(&[0x89, 0x50]));
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeMagicMatcher {
    /// Representation used to interpret the expected value bytes.
    value_type: MagicValueType,
    /// Inclusive first byte offset tested by this matcher.
    offset_begin: usize,
    /// Inclusive last byte offset tested by this matcher.
    offset_end: usize,
    /// Expected bytes, stored big-endian for numeric value types.
    value: Vec<u8>,
    /// Optional bit mask aligned with `value` byte for byte.
    mask: Option<Vec<u8>>,
    /// Nested alternatives evaluated only after this matcher succeeds.
    sub_matchers: Vec<MimeMagicMatcher>,
    /// Cached number of matcher levels from this node to the deepest leaf.
    depth: usize,
}

impl MimeMagicMatcher {
    /// Creates a magic matcher.
    ///
    /// # Parameters
    /// - `value_type`: Matcher value type.
    /// - `offset_begin`: First byte offset where the value may match.
    /// - `offset_end`: Last byte offset where the value may match.
    /// - `value`: Expected value bytes. Numeric values must be stored
    ///   big-endian.
    /// - `mask`: Optional mask bytes with the same length as `value`.
    /// - `sub_matchers`: Nested matchers; at least one child must match after
    ///   the parent.
    ///
    /// # Returns
    /// A validated [`MimeMagicMatcher`].
    ///
    /// # Errors
    /// Returns [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher)
    /// when offsets are inverted, when numeric value widths are wrong, when
    /// the mask length differs from the value length, or when the nested
    /// matcher depth exceeds 128 levels (counting this matcher as level one).
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
        let depth = sub_matchers
            .iter()
            .map(|matcher| matcher.depth)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        if depth > MAX_MAGIC_DEPTH {
            return Err(MimeError::invalid_matcher(format!(
                "magic matcher depth {depth} exceeds the maximum of {MAX_MAGIC_DEPTH}"
            )));
        }
        Ok(Self {
            value_type,
            offset_begin,
            offset_end,
            value,
            mask,
            sub_matchers,
            depth,
        })
    }

    /// Gets the matcher value type.
    ///
    /// # Returns
    /// The value type used by this matcher.
    #[must_use]
    #[inline]
    pub fn value_type(&self) -> MagicValueType {
        self.value_type
    }

    /// Gets the first offset that may match.
    ///
    /// # Returns
    /// Inclusive starting offset.
    #[must_use]
    #[inline]
    pub fn offset_begin(&self) -> usize {
        self.offset_begin
    }

    /// Gets the last offset that may match.
    ///
    /// # Returns
    /// Inclusive ending offset.
    #[must_use]
    #[inline]
    pub fn offset_end(&self) -> usize {
        self.offset_end
    }

    /// Gets the expected value bytes.
    ///
    /// # Returns
    /// Expected value bytes. Numeric values are stored big-endian.
    #[must_use]
    #[inline]
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Gets the optional mask bytes.
    ///
    /// # Returns
    /// `Some` mask bytes when the matcher masks input bytes before comparing,
    /// otherwise `None`.
    #[must_use]
    #[inline]
    pub fn mask(&self) -> Option<&[u8]> {
        self.mask.as_deref()
    }

    /// Gets nested sub-matchers.
    ///
    /// # Returns
    /// Child matchers evaluated after this matcher succeeds.
    #[must_use]
    #[inline]
    pub fn sub_matchers(&self) -> &[MimeMagicMatcher] {
        &self.sub_matchers
    }

    /// Gets the maximum number of bytes needed to evaluate this matcher.
    ///
    /// # Returns
    /// The highest tested offset plus value width, including sub-matchers.
    #[must_use]
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
    #[must_use]
    pub fn matches(&self, bytes: &[u8]) -> bool {
        let parent_matches = match self.value_type {
            MagicValueType::String => self.matches_bytes(bytes, &self.value, self.mask.as_deref()),
            MagicValueType::Byte => self.matches_bytes(bytes, &self.value, self.mask.as_deref()),
            MagicValueType::Host16
            | MagicValueType::Host32
            | MagicValueType::Big16
            | MagicValueType::Big32
            | MagicValueType::Little16
            | MagicValueType::Little32 => self.matches_numeric(bytes),
        };
        if !parent_matches {
            return false;
        }
        self.sub_matchers.is_empty() || self.sub_matchers.iter().any(|sub_matcher| sub_matcher.matches(bytes))
    }

    /// Tests this matcher with bounded cooperative work and cancellation
    /// checks.
    ///
    /// The explicit stack preserves the parser's maximum-depth guarantee and
    /// evaluates nested alternatives in their original depth-first order.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix tested against this matcher tree.
    /// - `scope`: Operation scope checked between bounded work batches.
    ///
    /// # Returns
    /// `true` when the parent and one complete nested branch match.
    ///
    /// # Errors
    /// Returns the accepted stop reason when the operation is cancelled.
    pub(crate) async fn matches_cooperatively(
        &self,
        bytes: &[u8],
        scope: &OperationScope,
        work: &mut MatcherWorkBudget,
    ) -> Result<bool, StopReason> {
        let mut pending = vec![(self, false, 0usize)];
        while !pending.is_empty() {
            let frame = pending.len() - 1;
            let matcher = pending[frame].0;
            if !pending[frame].1 {
                pending[frame].1 = true;
                if work.charge() {
                    yield_after_work(scope).await?;
                }
                if !matcher.matches_parent_cooperatively(bytes, scope, work).await? {
                    pending.pop();
                    continue;
                }
                if matcher.sub_matchers.is_empty() {
                    return Ok(true);
                }
            }
            let next_child = pending[frame].2;
            if next_child < matcher.sub_matchers.len() {
                pending[frame].2 += 1;
                pending.push((&matcher.sub_matchers[next_child], false, 0));
            } else {
                pending.pop();
            }
        }
        Ok(false)
    }

    /// Tests the current node's own value without visiting its children.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix tested against the node value.
    /// - `scope`: Operation scope used for bounded yields and cancellation.
    ///
    /// # Returns
    /// `true` when one allowed offset matches the node value.
    ///
    /// # Errors
    /// Returns the accepted stop reason when the operation is cancelled.
    async fn matches_parent_cooperatively(
        &self,
        bytes: &[u8],
        scope: &OperationScope,
        work: &mut MatcherWorkBudget,
    ) -> Result<bool, StopReason> {
        match self.value_type {
            MagicValueType::String | MagicValueType::Byte => {
                self.matches_bytes_cooperatively(bytes, scope, work).await
            }
            MagicValueType::Host16
            | MagicValueType::Host32
            | MagicValueType::Big16
            | MagicValueType::Big32
            | MagicValueType::Little16
            | MagicValueType::Little32 => self.matches_numeric_cooperatively(bytes, scope, work).await,
        }
    }

    /// Scans raw value offsets and yields after bounded work.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix tested by the raw matcher.
    /// - `scope`: Operation scope checked while scanning offsets.
    ///
    /// # Returns
    /// `true` when the value matches at an allowed offset.
    ///
    /// # Errors
    /// Returns the accepted stop reason when the operation is cancelled.
    async fn matches_bytes_cooperatively(
        &self,
        bytes: &[u8],
        scope: &OperationScope,
        work: &mut MatcherWorkBudget,
    ) -> Result<bool, StopReason> {
        if self.value.is_empty() || bytes.len() < self.value.len() || self.offset_begin >= bytes.len() {
            return Ok(false);
        }
        let end = self.offset_end.min(bytes.len() - self.value.len());
        if self.offset_begin > end {
            return Ok(false);
        }
        for offset in self.offset_begin..=end {
            if work.charge() {
                yield_after_work(scope).await?;
            }
            if self.value_matches_cooperatively(bytes, offset, scope, work).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Compares one value while yielding if its byte width is unusually large.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix containing the candidate value.
    /// - `offset`: Starting byte offset for this comparison.
    /// - `scope`: Operation scope checked during unusually wide comparisons.
    ///
    /// # Returns
    /// `true` when every value byte matches at the candidate offset.
    ///
    /// # Errors
    /// Returns the accepted stop reason when the operation is cancelled.
    async fn value_matches_cooperatively(
        &self,
        bytes: &[u8],
        offset: usize,
        scope: &OperationScope,
        work: &mut MatcherWorkBudget,
    ) -> Result<bool, StopReason> {
        for (index, value_byte) in self.value.iter().enumerate() {
            if work.charge() {
                yield_after_work(scope).await?;
            }
            let input = bytes[offset + index];
            let matches = match self.mask.as_deref() {
                Some(mask) => (input & mask[index]) == (*value_byte & mask[index]),
                None => input == *value_byte,
            };
            if !matches {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Scans numeric value offsets and yields after bounded work.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix tested by the numeric matcher.
    /// - `scope`: Operation scope checked while scanning offsets.
    ///
    /// # Returns
    /// `true` when a numeric value matches at an allowed offset.
    ///
    /// # Errors
    /// Returns the accepted stop reason when the operation is cancelled.
    async fn matches_numeric_cooperatively(
        &self,
        bytes: &[u8],
        scope: &OperationScope,
        work: &mut MatcherWorkBudget,
    ) -> Result<bool, StopReason> {
        if bytes.len() < self.value.len() || self.offset_begin >= bytes.len() {
            return Ok(false);
        }
        let end = self.offset_end.min(bytes.len() - self.value.len());
        if self.offset_begin > end {
            return Ok(false);
        }
        let little_endian = self.value_type.uses_little_endian_order();
        for offset in self.offset_begin..=end {
            if work.charge() {
                yield_after_work(scope).await?;
            }
            let mut matches = true;
            for (index, value_byte) in self.value.iter().enumerate() {
                if work.charge() {
                    yield_after_work(scope).await?;
                }
                let input_index = if little_endian {
                    self.value.len() - index - 1
                } else {
                    index
                };
                let actual = bytes[offset + input_index];
                let byte_matches = match self.mask.as_deref() {
                    Some(mask) => (actual & mask[index]) == (*value_byte & mask[index]),
                    None => actual == *value_byte,
                };
                if !byte_matches {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Ok(true);
            }
        }
        Ok(false)
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
    #[must_use]
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

    /// Tests a numeric value directly in the input's byte order without
    /// allocating reordered value or mask buffers.
    ///
    /// # Parameters
    /// - `bytes`: Content bytes to test.
    ///
    /// # Returns
    /// `true` when a value within the configured offset range matches.
    #[must_use]
    fn matches_numeric(&self, bytes: &[u8]) -> bool {
        if bytes.len() < self.value.len() || self.offset_begin >= bytes.len() {
            return false;
        }
        let last_possible = bytes.len() - self.value.len();
        let end = self.offset_end.min(last_possible);
        if self.offset_begin > end {
            return false;
        }
        let little_endian = self.value_type.uses_little_endian_order();
        (self.offset_begin..=end).any(|offset| {
            self.value.iter().enumerate().all(|(index, value_byte)| {
                let input_index = if little_endian {
                    self.value.len() - index - 1
                } else {
                    index
                };
                let actual = bytes[offset + input_index];
                match self.mask.as_deref() {
                    Some(mask) => (actual & mask[index]) == (*value_byte & mask[index]),
                    None => actual == *value_byte,
                }
            })
        })
    }
}

/// Gives the current executor an opportunity to poll other tasks.
async fn yield_to_executor() {
    use std::future::poll_fn;
    use std::task::Poll;

    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

/// Validates matcher offsets.
///
/// # Parameters
/// - `offset_begin`: First allowed offset.
/// - `offset_end`: Last allowed offset.
///
/// # Errors
/// Returns [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher)
/// when the range is inverted.
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
/// Returns [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher)
/// when a numeric value has the wrong width.
fn validate_value_width(value_type: MagicValueType, value: &[u8]) -> MimeResult<()> {
    if value.is_empty() {
        return Err(MimeError::invalid_matcher("magic matcher value must not be empty"));
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
/// Returns [`MimeError::InvalidMagicMatcher`](crate::MimeError::InvalidMagicMatcher)
/// when the mask length differs from value length.
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
#[must_use]
#[inline]
fn value_matches_at(bytes: &[u8], offset: usize, value: &[u8], mask: Option<&[u8]>) -> bool {
    match mask {
        Some(mask) => value
            .iter()
            .zip(mask.iter())
            .enumerate()
            .all(|(index, (value_byte, mask_byte))| (bytes[offset + index] & mask_byte) == (*value_byte & mask_byte)),
        None => value
            .iter()
            .enumerate()
            .all(|(index, value_byte)| bytes[offset + index] == *value_byte),
    }
}
