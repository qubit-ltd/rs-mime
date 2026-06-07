// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Magic matcher value type.

/// Value type used by a MIME magic matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MagicValueType {
    /// A byte string.
    String,
    /// A native-endian 16-bit integer.
    Host16,
    /// A native-endian 32-bit integer.
    Host32,
    /// A big-endian 16-bit integer.
    Big16,
    /// A big-endian 32-bit integer.
    Big32,
    /// A little-endian 16-bit integer.
    Little16,
    /// A little-endian 32-bit integer.
    Little32,
    /// A single byte.
    Byte,
}

impl MagicValueType {
    /// Parses a freedesktop magic type name.
    ///
    /// # Parameters
    /// - `name`: XML type attribute value such as `string` or `little32`.
    ///
    /// # Returns
    /// The matching [`MagicValueType`], or `None` when the name is unknown.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "string" => Some(Self::String),
            "host16" => Some(Self::Host16),
            "host32" => Some(Self::Host32),
            "big16" => Some(Self::Big16),
            "big32" => Some(Self::Big32),
            "little16" => Some(Self::Little16),
            "little32" => Some(Self::Little32),
            "byte" => Some(Self::Byte),
            _ => None,
        }
    }

    /// Gets the freedesktop magic type name.
    ///
    /// # Returns
    /// Static type name used in XML.
    pub fn name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Host16 => "host16",
            Self::Host32 => "host32",
            Self::Big16 => "big16",
            Self::Big32 => "big32",
            Self::Little16 => "little16",
            Self::Little32 => "little32",
            Self::Byte => "byte",
        }
    }

    /// Gets the fixed byte width for numeric matchers.
    ///
    /// # Returns
    /// `None` for string matchers; otherwise the required byte length.
    pub(crate) fn numeric_width(self) -> Option<usize> {
        match self {
            Self::String => None,
            Self::Byte => Some(1),
            Self::Host16 | Self::Big16 | Self::Little16 => Some(2),
            Self::Host32 | Self::Big32 | Self::Little32 => Some(4),
        }
    }

    /// Tells whether this type should be matched in little-endian byte order.
    ///
    /// # Returns
    /// `true` when the stored big-endian bytes must be reversed before
    /// matching.
    pub(crate) fn uses_little_endian_order(self) -> bool {
        matches!(self, Self::Little16 | Self::Little32)
            || (cfg!(target_endian = "little")
                && matches!(self, Self::Host16 | Self::Host32))
    }
}
