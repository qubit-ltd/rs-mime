// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Repository of MIME types parsed from shared MIME-info XML.

use std::collections::HashMap;

use qubit_codec_misc::CIntegerLiteralCodec;
use qubit_codec_misc::CStringLiteralCodec;
use qubit_codec_misc::HexCodec;
use qubit_codec_misc::MiscCodecError;
use roxmltree::Document;
use roxmltree::NS_XML_URI;
use roxmltree::Node;

use super::glob_index::GlobIndex;
use super::magic_index::MagicIndex;
use super::xml_parser::strip_doctype;
use crate::MagicValueType;
use crate::MimeDetectionPolicy;
use crate::MimeError;
use crate::MimeGlob;
use crate::MimeMagic;
use crate::MimeMagicMatcher;
use crate::MimeResult;
use crate::MimeType;
use crate::MimeTypeBuilder;

/// A repository of MIME types and detection indexes.
#[derive(Debug, Clone)]
pub struct MimeRepository {
    mime_types: Vec<MimeType>,
    name_map: HashMap<String, usize>,
    glob_index: GlobIndex,
    magic_index: MagicIndex,
}

impl MimeRepository {
    /// Parses a MIME repository from shared MIME-info XML.
    ///
    /// # Parameters
    /// - `xml`: XML document whose root element is `mime-info`.
    ///
    /// # Returns
    /// A parsed repository with filename and alias indexes.
    ///
    /// # Errors
    /// Returns [`MimeError`](crate::MimeError) when XML is malformed or a rule
    /// contains an unsupported value.
    pub fn from_xml(xml: &str) -> MimeResult<Self> {
        let xml = strip_doctype(xml);
        let document = Document::parse(&xml)?;
        let root = document.root_element();
        if root.tag_name().name() != "mime-info" {
            return Err(MimeError::invalid_element(
                root.tag_name().name(),
                "root element must be <mime-info>",
            ));
        }
        let mut repository = Self::empty();
        for child in root.children().filter(Node::is_element) {
            if child.tag_name().name() == "mime-type" {
                repository.add_mime_type(parse_mime_type(child)?)?;
            }
        }
        if root.tag_name().namespace() != Some("http://www.freedesktop.org/standards/shared-mime-info") {
            return Err(MimeError::invalid_element(
                root.tag_name().name(),
                "root element must declare the shared-mime-info namespace",
            ));
        }
        Ok(repository)
    }

    /// Creates an empty repository.
    ///
    /// # Returns
    /// A repository with no MIME types.
    pub fn empty() -> Self {
        Self {
            mime_types: Vec::new(),
            name_map: HashMap::new(),
            glob_index: GlobIndex::default(),
            magic_index: MagicIndex::default(),
        }
    }

    /// Gets all MIME types in database order.
    ///
    /// # Returns
    /// Slice of all parsed MIME types.
    pub fn all(&self) -> &[MimeType] {
        &self.mime_types
    }

    /// Gets a MIME type by canonical name or alias.
    ///
    /// # Parameters
    /// - `name`: MIME type name or alias.
    ///
    /// # Returns
    /// The matching MIME type, or `None`.
    pub fn get(&self, name: &str) -> Option<&MimeType> {
        self.name_map
            .get(&normalize_mime_name(name))
            .and_then(|index| self.mime_types.get(*index))
    }

    /// Gets the maximum number of bytes needed by any magic rule.
    ///
    /// # Returns
    /// Buffer size sufficient for all content magic checks.
    pub fn max_test_bytes(&self) -> usize {
        self.magic_index.max_test_bytes
    }

    /// Tests whether one MIME type is equal to or a descendant of another.
    pub fn is_a(&self, child: &str, parent: &str) -> bool {
        let child = self.canonical_name_or_input(child);
        let parent = self.canonical_name_or_input(parent);
        if child == parent {
            return true;
        }
        if parent == "application/octet-stream" && self.get(&child).is_some() {
            return true;
        }
        if child.starts_with("text/") && parent == "text/plain" {
            return true;
        }
        let mut pending = vec![child];
        let mut visited = std::collections::HashSet::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }
            let Some(mime_type) = self.get(&name) else {
                continue;
            };
            for super_type in mime_type.super_types() {
                let canonical = self.canonical_name_or_input(super_type);
                if canonical == parent {
                    return true;
                }
                pending.push(canonical);
            }
        }
        false
    }

    /// Detects MIME types from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename. Only the final path component is
    ///   used.
    ///
    /// # Returns
    /// Matching MIME types ordered by best glob weight and pattern length.
    /// Returns an empty vector when no glob matches.
    pub fn detect_by_filename(&self, filename: &str) -> Vec<&MimeType> {
        self.glob_index
            .matches(filename)
            .into_iter()
            .filter_map(|entry| self.mime_types.get(entry.mime_index))
            .collect()
    }

    fn canonical_name_or_input(&self, name: &str) -> String {
        let normalized = normalize_mime_name(name);
        self.name_map
            .get(&normalized)
            .and_then(|index| self.mime_types.get(*index))
            .map(|mime_type| mime_type.name().to_owned())
            .unwrap_or(normalized)
    }

    /// Detects MIME types from content bytes.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix to test against magic rules.
    ///
    /// # Returns
    /// Matching MIME types ordered by highest magic priority. Returns an empty
    /// vector when no magic rule matches.
    pub fn detect_by_content(&self, bytes: &[u8]) -> Vec<&MimeType> {
        self.magic_index
            .matches(bytes)
            .into_iter()
            .filter_map(|index| self.mime_types.get(index))
            .collect()
    }

    /// Detects MIME type by merging filename and content results.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    /// - `bytes`: Content prefix to test.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// A vector containing the selected MIME type, or an empty vector when no
    /// rule matches.
    pub fn detect(&self, filename: &str, bytes: &[u8], policy: MimeDetectionPolicy) -> Vec<&MimeType> {
        let from_filename = self.detect_by_filename(filename);
        if from_filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
            return from_filename;
        }
        let from_content = self.detect_by_content(bytes);
        merge_results(from_filename, from_content)
    }

    /// Adds a MIME type and updates lookup indexes.
    ///
    /// # Parameters
    /// - `mime_type`: MIME type to insert.
    fn add_mime_type(&mut self, mime_type: MimeType) -> MimeResult<()> {
        let mime_index = self.mime_types.len();
        for name in std::iter::once(mime_type.name()).chain(mime_type.aliases().iter().map(String::as_str)) {
            let normalized = normalize_mime_name(name);
            if self.name_map.contains_key(&normalized) {
                return Err(MimeError::DuplicateMimeName { name: normalized });
            }
        }
        self.index_names(mime_index, &mime_type);
        self.index_globs(mime_index, &mime_type);
        self.index_magics(mime_index, &mime_type);
        self.mime_types.push(mime_type);
        Ok(())
    }

    /// Adds canonical name and aliases to the name index.
    ///
    /// # Parameters
    /// - `mime_index`: Index of `mime_type` in `mime_types`.
    /// - `mime_type`: MIME type to index.
    fn index_names(&mut self, mime_index: usize, mime_type: &MimeType) {
        self.name_map.insert(normalize_mime_name(mime_type.name()), mime_index);
        for alias in mime_type.aliases() {
            self.name_map.insert(normalize_mime_name(alias), mime_index);
        }
    }

    /// Adds glob rules to the optimized filename indexes.
    ///
    /// # Parameters
    /// - `mime_index`: Index of `mime_type` in `mime_types`.
    /// - `mime_type`: MIME type to index.
    fn index_globs(&mut self, mime_index: usize, mime_type: &MimeType) {
        for glob in mime_type.globs() {
            self.glob_index.add(mime_index, glob);
        }
    }

    /// Updates the repository-wide maximum magic read length.
    ///
    /// # Parameters
    /// - `mime_type`: MIME type whose magic rules should be inspected.
    fn index_magics(&mut self, mime_index: usize, mime_type: &MimeType) {
        for magic in mime_type.magics() {
            self.magic_index.add(mime_index, magic);
        }
    }
}

/// Parses one `mime-type` element.
///
/// # Parameters
/// - `node`: XML element to parse.
///
/// # Returns
/// Parsed MIME type.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when required attributes or child
/// rules are invalid.
fn parse_mime_type(node: Node<'_, '_>) -> MimeResult<MimeType> {
    let name = required_attr(node, "type")?.to_owned();
    let mut builder = MimeTypeBuilder::new(&name);
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "comment" => {
                let language = comment_language(child);
                builder = builder.description(language, child.text().unwrap_or(""));
            }
            "alias" => builder = builder.alias(required_attr(child, "type")?),
            "sub-class-of" => builder = builder.super_type(required_attr(child, "type")?),
            "glob" => builder = builder.glob(parse_glob(child)?),
            "magic" => builder = builder.magic(parse_magic(child)?),
            _ => {}
        }
    }
    builder.build()
}

/// Parses one `glob` element.
///
/// # Parameters
/// - `node`: XML element to parse.
///
/// # Returns
/// Parsed glob.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when attributes are invalid.
fn parse_glob(node: Node<'_, '_>) -> MimeResult<MimeGlob> {
    let pattern = required_attr(node, "pattern")?;
    let weight = optional_u16_attr(
        node,
        "weight",
        MimeGlob::MIN_WEIGHT,
        MimeGlob::MAX_WEIGHT,
        MimeGlob::DEFAULT_WEIGHT,
    )?;
    let case_sensitive = optional_bool_attr(node, "case-sensitive", false)?;
    MimeGlob::new(pattern, weight, case_sensitive)
}

/// Parses one `magic` element.
///
/// # Parameters
/// - `node`: XML element to parse.
///
/// # Returns
/// Parsed magic rule.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when priority or matchers are
/// invalid.
fn parse_magic(node: Node<'_, '_>) -> MimeResult<MimeMagic> {
    let priority = optional_u16_attr(
        node,
        "priority",
        MimeMagic::MIN_PRIORITY,
        MimeMagic::MAX_PRIORITY,
        MimeMagic::DEFAULT_PRIORITY,
    )?;
    let matchers: MimeResult<Vec<_>> = node
        .children()
        .filter(Node::is_element)
        .filter(|child| child.tag_name().name() == "match")
        .map(parse_matcher)
        .collect();
    let matchers = matchers?;
    if matchers.is_empty() {
        return Err(MimeError::invalid_element(
            "magic",
            "magic must contain at least one match",
        ));
    }
    MimeMagic::new(priority, matchers)
}

/// Parses one recursive `match` element.
///
/// # Parameters
/// - `node`: XML element to parse.
///
/// # Returns
/// Parsed magic matcher.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when matcher attributes are invalid.
fn parse_matcher(node: Node<'_, '_>) -> MimeResult<MimeMagicMatcher> {
    let type_name = required_attr(node, "type")?;
    let value_type = MagicValueType::from_name(type_name)
        .ok_or_else(|| MimeError::invalid_attr("match", "type", type_name, "unknown type"))?;
    let (offset_begin, offset_end) = parse_offset(required_attr(node, "offset")?)?;
    let value = parse_value(value_type, required_attr(node, "value")?)?;
    let mask = match node.attribute("mask") {
        Some(mask) => Some(parse_mask(value_type, mask)?),
        None => None,
    };
    let sub_matchers: MimeResult<Vec<_>> = node
        .children()
        .filter(Node::is_element)
        .filter(|child| child.tag_name().name() == "match")
        .map(parse_matcher)
        .collect();
    MimeMagicMatcher::new(value_type, offset_begin, offset_end, value, mask, sub_matchers?)
}

/// Reads the language key from a `comment` element.
///
/// # Parameters
/// - `node`: `comment` element to inspect.
///
/// # Returns
/// XML language key, or an empty string for the default comment.
fn comment_language<'a>(node: Node<'a, '_>) -> &'a str {
    node.attribute((NS_XML_URI, "lang"))
        .or_else(|| node.attribute("xml:lang"))
        .unwrap_or("")
}

/// Reads a required XML attribute.
///
/// # Parameters
/// - `node`: Element to read from.
/// - `name`: Attribute name.
///
/// # Returns
/// Attribute value.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the attribute is missing or
/// empty.
fn required_attr<'a>(node: Node<'a, '_>, name: &str) -> MimeResult<&'a str> {
    node.attribute(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| MimeError::invalid_attr(node.tag_name().name(), name, "", "required attribute is missing"))
}

/// Reads an optional bounded `u16` XML attribute.
///
/// # Parameters
/// - `node`: Element to read from.
/// - `name`: Attribute name.
/// - `min`: Minimum allowed value.
/// - `max`: Maximum allowed value.
/// - `default`: Default value when the attribute is absent.
///
/// # Returns
/// Parsed value.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the value is not an integer or
/// is out of range.
fn optional_u16_attr(node: Node<'_, '_>, name: &str, min: u16, max: u16, default: u16) -> MimeResult<u16> {
    let Some(value) = node.attribute(name) else {
        return Ok(default);
    };
    let parsed = value
        .parse::<u16>()
        .map_err(|error| MimeError::invalid_attr(node.tag_name().name(), name, value, error.to_string()))?;
    if parsed < min || parsed > max {
        return Err(MimeError::invalid_attr(
            node.tag_name().name(),
            name,
            value,
            format!("value must be in {min}..={max}"),
        ));
    }
    Ok(parsed)
}

/// Reads an optional boolean XML attribute.
///
/// # Parameters
/// - `node`: Element to read from.
/// - `name`: Attribute name.
/// - `default`: Default value when the attribute is absent.
///
/// # Returns
/// Parsed boolean value.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the value is not `true` or
/// `false`.
fn optional_bool_attr(node: Node<'_, '_>, name: &str, default: bool) -> MimeResult<bool> {
    match node.attribute(name) {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(value) => Err(MimeError::invalid_attr(
            node.tag_name().name(),
            name,
            value,
            "expected true or false",
        )),
        None => Ok(default),
    }
}

/// Parses an offset or offset range.
///
/// # Parameters
/// - `value`: Offset attribute text such as `0` or `0:256`.
///
/// # Returns
/// Inclusive offset range.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the range is invalid.
fn parse_offset(value: &str) -> MimeResult<(usize, usize)> {
    let (begin, end) = value.split_once(':').unwrap_or((value, value));
    let offset_begin = parse_usize(begin, "offset")?;
    let offset_end = parse_usize(end, "offset")?;
    if offset_begin > offset_end {
        return Err(MimeError::invalid_attr(
            "match",
            "offset",
            value,
            "offset begin must not exceed offset end",
        ));
    }
    Ok((offset_begin, offset_end))
}

/// Parses a non-negative integer.
///
/// # Parameters
/// - `value`: Number text.
/// - `attribute`: Attribute name used in error messages.
///
/// # Returns
/// Parsed integer.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the number is invalid.
fn parse_usize(value: &str, attribute: &str) -> MimeResult<usize> {
    value
        .parse::<usize>()
        .map_err(|error| MimeError::invalid_attr("match", attribute, value, format!("invalid integer: {error}")))
}

/// Parses a magic value attribute.
///
/// # Parameters
/// - `value_type`: Matcher value type.
/// - `value`: Attribute value text.
///
/// # Returns
/// Parsed bytes. Numeric values are stored big-endian.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the value cannot be decoded.
fn parse_value(value_type: MagicValueType, value: &str) -> MimeResult<Vec<u8>> {
    match value_type {
        MagicValueType::String => parse_c_string_bytes(value),
        _ => parse_numeric_bytes(value_type, value),
    }
}

/// Parses a magic mask attribute.
///
/// # Parameters
/// - `value_type`: Matcher value type.
/// - `value`: Attribute value text.
///
/// # Returns
/// Parsed mask bytes.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the mask cannot be decoded.
fn parse_mask(value_type: MagicValueType, value: &str) -> MimeResult<Vec<u8>> {
    match value_type {
        MagicValueType::String => parse_hex_bytes(value),
        _ => parse_numeric_bytes(value_type, value),
    }
}

/// Parses a C string literal used by shared MIME-info magic values.
///
/// # Parameters
/// - `value`: Attribute value after XML entity decoding.
///
/// # Returns
/// Decoded bytes.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the literal cannot be decoded.
fn parse_c_string_bytes(value: &str) -> MimeResult<Vec<u8>> {
    CStringLiteralCodec::new()
        .decode(value)
        .map_err(|error| MimeError::invalid_attr("match", "value", value, format!("invalid C string literal: {error}")))
}

/// Parses a numeric magic value into big-endian bytes.
///
/// # Parameters
/// - `value_type`: Numeric matcher type.
/// - `value`: Numeric text in decimal, octal, or hexadecimal notation.
///
/// # Returns
/// Big-endian bytes with the width required by `value_type`.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the value is invalid.
fn parse_numeric_bytes(value_type: MagicValueType, value: &str) -> MimeResult<Vec<u8>> {
    let number = CIntegerLiteralCodec::new().decode(value).map_err(|error| {
        MimeError::invalid_attr("match", "value", value, format!("invalid C integer literal: {error}"))
    })?;
    match value_type
        .numeric_width()
        .expect("numeric parser should only receive numeric magic types")
    {
        1 => u8::try_from(number)
            .map(|number| vec![number])
            .map_err(|_| numeric_value_out_of_range(value_type, value)),
        2 => u16::try_from(number)
            .map(|number| number.to_be_bytes().to_vec())
            .map_err(|_| numeric_value_out_of_range(value_type, value)),
        4 => u32::try_from(number)
            .map(|number| number.to_be_bytes().to_vec())
            .map_err(|_| numeric_value_out_of_range(value_type, value)),
        _ => unreachable!("unsupported numeric magic width"),
    }
}

/// Builds an out-of-range numeric magic value error.
///
/// # Parameters
/// - `value_type`: Numeric matcher type.
/// - `value`: Numeric text from the XML attribute.
///
/// # Returns
/// Invalid XML attribute error for an oversized numeric value.
fn numeric_value_out_of_range(value_type: MagicValueType, value: &str) -> MimeError {
    let width = value_type
        .numeric_width()
        .expect("numeric parser should only receive numeric magic types");
    MimeError::invalid_attr(
        "match",
        "value",
        value,
        format!("{} value must fit in {width} byte(s)", value_type.name()),
    )
}

/// Parses `0x` prefixed hex bytes.
///
/// # Parameters
/// - `value`: Hex byte string.
///
/// # Returns
/// Decoded bytes.
///
/// # Errors
/// Returns [`MimeError`](crate::MimeError) when the value is not an
/// even-length hex string or contains non-hex characters.
fn parse_hex_bytes(value: &str) -> MimeResult<Vec<u8>> {
    HexCodec::new()
        .with_prefix("0x")
        .with_ignore_prefix_case(true)
        .decode(value)
        .map_err(|error| match error {
            MiscCodecError::MissingPrefix { .. } => {
                MimeError::invalid_attr("match", "mask", value, "string mask must start with 0x")
            }
            other => MimeError::invalid_attr("match", "mask", value, format!("invalid hex byte: {other}")),
        })
}

/// Gets normalized MIME type name.
///
/// # Parameters
/// - `name`: MIME type name.
///
/// # Returns
/// Lowercase name for map lookup.
fn normalize_mime_name(name: &str) -> String {
    name.to_lowercase()
}

/// Merges filename and content detection results.
///
/// # Parameters
/// - `from_filename`: Candidates from filename globs.
/// - `from_content`: Candidates from content magic.
///
/// # Returns
/// A single selected MIME type, or an empty vector when neither source matched.
fn merge_results<'a>(from_filename: Vec<&'a MimeType>, from_content: Vec<&'a MimeType>) -> Vec<&'a MimeType> {
    if from_filename.is_empty() {
        return from_content.into_iter().take(1).collect();
    }
    if from_content.is_empty() {
        return from_filename.into_iter().take(1).collect();
    }
    if let Some(common) = from_filename
        .iter()
        .find(|mime_type| from_content.iter().any(|content| content.name() == mime_type.name()))
    {
        vec![*common]
    } else {
        from_content.into_iter().take(1).collect()
    }
}
