/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Repository of MIME types parsed from shared MIME-info XML.

use std::borrow::Cow;
use std::collections::HashMap;

use roxmltree::{Document, Node};

use crate::{
    MagicValueType, MimeDetectionPolicy, MimeError, MimeGlob, MimeMagic, MimeMagicMatcher,
    MimeType, MimeTypeBuilder,
};

/// A repository of MIME types and detection indexes.
#[derive(Debug, Clone)]
pub struct MimeRepository {
    mime_types: Vec<MimeType>,
    name_map: HashMap<String, usize>,
    literal_globs: HashMap<String, Vec<GlobEntry>>,
    extension_globs: HashMap<String, Vec<GlobEntry>>,
    other_globs: Vec<GlobEntry>,
    max_test_bytes: usize,
}

#[derive(Debug, Clone)]
struct GlobEntry {
    glob: MimeGlob,
    mime_index: usize,
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
    /// Returns [`MimeError`] when XML is malformed or a rule contains an
    /// unsupported value.
    pub fn from_xml(xml: &str) -> Result<Self, MimeError> {
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
                repository.add_mime_type(parse_mime_type(child)?);
            }
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
            literal_globs: HashMap::new(),
            extension_globs: HashMap::new(),
            other_globs: Vec::new(),
            max_test_bytes: 0,
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
        self.max_test_bytes
    }

    /// Detects MIME types from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename. Only the final path component is used.
    ///
    /// # Returns
    /// Matching MIME types ordered by best glob weight and pattern length. Returns
    /// an empty vector when no glob matches.
    pub fn detect_by_filename(&self, filename: &str) -> Vec<&MimeType> {
        let exact_filename = filename_from_path(filename);
        if exact_filename.is_empty() {
            return Vec::new();
        }
        let lookup_filename = exact_filename.to_lowercase();
        let mut result = GlobDetectionResult::new();
        if let Some(entries) = self.literal_globs.get(&lookup_filename) {
            result.add_matching_entries(entries, exact_filename);
        }
        for extension in extension_suffixes(&lookup_filename) {
            if let Some(entries) = self.extension_globs.get(extension) {
                result.add_matching_entries(entries, exact_filename);
            }
        }
        for entry in &self.other_globs {
            if entry.glob.matches(exact_filename) {
                result.compare_add(entry);
            }
        }
        result
            .entries
            .into_iter()
            .filter_map(|entry| self.mime_types.get(entry.mime_index))
            .collect()
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
        let mut result = MagicDetectionResult::new();
        for mime_type in &self.mime_types {
            for magic in mime_type.magics() {
                let priority = magic.priority();
                if priority >= result.best_priority && magic.matches(bytes) {
                    result.compare_add(priority, mime_type);
                }
            }
        }
        result.mime_types
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
    pub fn detect(
        &self,
        filename: &str,
        bytes: &[u8],
        policy: MimeDetectionPolicy,
    ) -> Vec<&MimeType> {
        let from_filename = self.detect_by_filename(filename);
        if from_filename.len() == 1 && !policy.should_verify_content() {
            return from_filename;
        }
        let from_content = self.detect_by_content(bytes);
        merge_results(from_filename, from_content)
    }

    /// Adds a MIME type and updates lookup indexes.
    ///
    /// # Parameters
    /// - `mime_type`: MIME type to insert.
    fn add_mime_type(&mut self, mime_type: MimeType) {
        let mime_index = self.mime_types.len();
        self.index_names(mime_index, &mime_type);
        self.index_globs(mime_index, &mime_type);
        self.index_magics(&mime_type);
        self.mime_types.push(mime_type);
    }

    /// Adds canonical name and aliases to the name index.
    ///
    /// # Parameters
    /// - `mime_index`: Index of `mime_type` in `mime_types`.
    /// - `mime_type`: MIME type to index.
    fn index_names(&mut self, mime_index: usize, mime_type: &MimeType) {
        self.name_map
            .insert(normalize_mime_name(mime_type.name()), mime_index);
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
            let entry = GlobEntry {
                glob: glob.clone(),
                mime_index,
            };
            if let Some(extension) = extension_pattern(glob.pattern()) {
                self.extension_globs
                    .entry(extension.to_lowercase())
                    .or_default()
                    .push(entry);
            } else if is_literal_pattern(glob.pattern()) {
                self.literal_globs
                    .entry(glob.pattern().to_lowercase())
                    .or_default()
                    .push(entry);
            } else {
                self.other_globs.push(entry);
            }
        }
    }

    /// Updates the repository-wide maximum magic read length.
    ///
    /// # Parameters
    /// - `mime_type`: MIME type whose magic rules should be inspected.
    fn index_magics(&mut self, mime_type: &MimeType) {
        for magic in mime_type.magics() {
            self.max_test_bytes = self.max_test_bytes.max(magic.max_test_bytes());
        }
    }
}

#[derive(Debug)]
struct GlobDetectionResult<'a> {
    best_weight: u16,
    best_length: usize,
    entries: Vec<&'a GlobEntry>,
}

impl<'a> GlobDetectionResult<'a> {
    /// Creates an empty glob detection result.
    ///
    /// # Returns
    /// New result with no entries.
    fn new() -> Self {
        Self {
            best_weight: 0,
            best_length: 0,
            entries: Vec::new(),
        }
    }

    /// Adds matching entries that beat or tie the current best result.
    ///
    /// # Parameters
    /// - `entries`: Candidate glob entries.
    /// - `filename`: Original-case filename to test against case-sensitive globs.
    fn add_matching_entries(&mut self, entries: &'a [GlobEntry], filename: &str) {
        for entry in entries {
            if entry.glob.matches(filename) {
                self.compare_add(entry);
            }
        }
    }

    /// Compares one glob entry against the current best result.
    ///
    /// # Parameters
    /// - `entry`: Matching glob entry.
    fn compare_add(&mut self, entry: &'a GlobEntry) {
        let weight = entry.glob.weight();
        let length = entry.glob.pattern().len();
        if self.entries.is_empty() || weight > self.best_weight {
            self.entries.clear();
            self.entries.push(entry);
            self.best_weight = weight;
            self.best_length = length;
        } else if weight == self.best_weight {
            if length > self.best_length {
                self.entries.clear();
                self.entries.push(entry);
                self.best_length = length;
            } else if length == self.best_length {
                self.entries.push(entry);
            }
        }
    }
}

/// Removes a DTD declaration before parsing with `roxmltree`.
///
/// # Parameters
/// - `xml`: Source XML text.
///
/// # Returns
/// Borrowed XML when no DTD exists; otherwise an owned XML string with the DTD
/// declaration removed.
fn strip_doctype(xml: &str) -> Cow<'_, str> {
    let Some(start) = xml.find("<!DOCTYPE") else {
        return Cow::Borrowed(xml);
    };
    let Some(rest) = xml.get(start..) else {
        return Cow::Borrowed(xml);
    };
    let end_offset = rest
        .find("]>")
        .map(|index| index + 2)
        .or_else(|| rest.find('>').map(|index| index + 1));
    let Some(end_offset) = end_offset else {
        return Cow::Borrowed(xml);
    };
    let mut cleaned = String::with_capacity(xml.len().saturating_sub(end_offset));
    cleaned.push_str(&xml[..start]);
    cleaned.push_str(&xml[start + end_offset..]);
    Cow::Owned(cleaned)
}

#[derive(Debug)]
struct MagicDetectionResult<'a> {
    best_priority: u16,
    mime_types: Vec<&'a MimeType>,
}

impl<'a> MagicDetectionResult<'a> {
    /// Creates an empty magic detection result.
    ///
    /// # Returns
    /// New result with no MIME types.
    fn new() -> Self {
        Self {
            best_priority: 0,
            mime_types: Vec::new(),
        }
    }

    /// Compares one content match against the current best result.
    ///
    /// # Parameters
    /// - `priority`: Priority of the matched magic rule.
    /// - `mime_type`: MIME type matched by the rule.
    fn compare_add(&mut self, priority: u16, mime_type: &'a MimeType) {
        if self.mime_types.is_empty() || priority > self.best_priority {
            self.mime_types.clear();
            self.mime_types.push(mime_type);
            self.best_priority = priority;
        } else if priority == self.best_priority && !self.mime_types.contains(&mime_type) {
            self.mime_types.push(mime_type);
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
/// Returns [`MimeError`] when required attributes or child rules are invalid.
fn parse_mime_type(node: Node<'_, '_>) -> Result<MimeType, MimeError> {
    let name = required_attr(node, "type")?.to_owned();
    let mut builder = MimeTypeBuilder::new(&name);
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "comment" => {
                let language = child.attribute("xml:lang").unwrap_or("");
                builder = builder.description(language, child.text().unwrap_or(""));
            }
            "alias" => builder = builder.alias(required_attr(child, "type")?),
            "sub-class-of" => builder = builder.super_type(required_attr(child, "type")?),
            "glob" => builder = builder.glob(parse_glob(child)?),
            "magic" => builder = builder.magic(parse_magic(child)?),
            _ => {}
        }
    }
    Ok(builder.build())
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
/// Returns [`MimeError`] when attributes are invalid.
fn parse_glob(node: Node<'_, '_>) -> Result<MimeGlob, MimeError> {
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
/// Returns [`MimeError`] when priority or matchers are invalid.
fn parse_magic(node: Node<'_, '_>) -> Result<MimeMagic, MimeError> {
    let priority = optional_u16_attr(
        node,
        "priority",
        MimeMagic::MIN_PRIORITY,
        MimeMagic::MAX_PRIORITY,
        MimeMagic::DEFAULT_PRIORITY,
    )?;
    let matchers: Result<Vec<_>, _> = node
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
    Ok(MimeMagic::new(priority, matchers))
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
/// Returns [`MimeError`] when matcher attributes are invalid.
fn parse_matcher(node: Node<'_, '_>) -> Result<MimeMagicMatcher, MimeError> {
    let type_name = required_attr(node, "type")?;
    let value_type = MagicValueType::from_name(type_name)
        .ok_or_else(|| MimeError::invalid_attr("match", "type", type_name, "unknown type"))?;
    let (offset_begin, offset_end) = parse_offset(required_attr(node, "offset")?)?;
    let value = parse_value(value_type, required_attr(node, "value")?)?;
    let mask = match node.attribute("mask") {
        Some(mask) => Some(parse_mask(value_type, mask)?),
        None => None,
    };
    let sub_matchers: Result<Vec<_>, _> = node
        .children()
        .filter(Node::is_element)
        .filter(|child| child.tag_name().name() == "match")
        .map(parse_matcher)
        .collect();
    MimeMagicMatcher::new(
        value_type,
        offset_begin,
        offset_end,
        value,
        mask,
        sub_matchers?,
    )
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
/// Returns [`MimeError`] when the attribute is missing or empty.
fn required_attr<'a>(node: Node<'a, '_>, name: &str) -> Result<&'a str, MimeError> {
    node.attribute(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            MimeError::invalid_attr(
                node.tag_name().name(),
                name,
                "",
                "required attribute is missing",
            )
        })
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
/// Returns [`MimeError`] when the value is not an integer or is out of range.
fn optional_u16_attr(
    node: Node<'_, '_>,
    name: &str,
    min: u16,
    max: u16,
    default: u16,
) -> Result<u16, MimeError> {
    let Some(value) = node.attribute(name) else {
        return Ok(default);
    };
    let parsed = value.parse::<u16>().map_err(|error| {
        MimeError::invalid_attr(node.tag_name().name(), name, value, error.to_string())
    })?;
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
/// Returns [`MimeError`] when the value is not `true` or `false`.
fn optional_bool_attr(node: Node<'_, '_>, name: &str, default: bool) -> Result<bool, MimeError> {
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
/// Returns [`MimeError`] when the range is invalid.
fn parse_offset(value: &str) -> Result<(usize, usize), MimeError> {
    let (begin, end) = value.split_once(':').map_or((value, value), |parts| parts);
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
/// Returns [`MimeError`] when the number is invalid.
fn parse_usize(value: &str, attribute: &str) -> Result<usize, MimeError> {
    value.parse::<usize>().map_err(|error| {
        MimeError::invalid_attr(
            "match",
            attribute,
            value,
            format!("invalid integer: {error}"),
        )
    })
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
/// Returns [`MimeError`] when the value cannot be decoded.
fn parse_value(value_type: MagicValueType, value: &str) -> Result<Vec<u8>, MimeError> {
    match value_type {
        MagicValueType::String => decode_c_string(value),
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
/// Returns [`MimeError`] when the mask cannot be decoded.
fn parse_mask(value_type: MagicValueType, value: &str) -> Result<Vec<u8>, MimeError> {
    match value_type {
        MagicValueType::String => parse_hex_bytes(value),
        _ => parse_numeric_bytes(value_type, value),
    }
}

/// Parses a C-style string literal used by shared MIME-info magic values.
///
/// # Parameters
/// - `value`: Attribute value after XML entity decoding.
///
/// # Returns
/// Decoded bytes.
///
/// # Errors
/// Returns [`MimeError`] when an escape sequence is incomplete or invalid.
fn decode_c_string(value: &str) -> Result<Vec<u8>, MimeError> {
    let chars: Vec<char> = value.chars().collect();
    let mut bytes = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '\\' {
            push_char_bytes(chars[index], &mut bytes);
            index += 1;
            continue;
        }
        index += 1;
        if index >= chars.len() {
            return Err(MimeError::invalid_attr(
                "match",
                "value",
                value,
                "trailing backslash",
            ));
        }
        match chars[index] {
            'n' => bytes.push(b'\n'),
            'r' => bytes.push(b'\r'),
            't' => bytes.push(b'\t'),
            '\\' => bytes.push(b'\\'),
            '"' => bytes.push(b'"'),
            '\'' => bytes.push(b'\''),
            'x' | 'X' => index = decode_hex_escape(&chars, index, value, &mut bytes)?,
            ch if ch.is_ascii_digit() && ch < '8' => {
                index = decode_octal_escape(&chars, index, &mut bytes);
            }
            ch => push_char_bytes(ch, &mut bytes),
        }
        index += 1;
    }
    Ok(bytes)
}

/// Appends a Unicode scalar value as UTF-8 bytes.
///
/// # Parameters
/// - `ch`: Character to append.
/// - `bytes`: Destination byte buffer.
fn push_char_bytes(ch: char, bytes: &mut Vec<u8>) {
    let mut buffer = [0; 4];
    bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
}

/// Decodes a `\xNN` escape.
///
/// # Parameters
/// - `chars`: Source characters.
/// - `index`: Index of the `x` marker.
/// - `source`: Original attribute value for diagnostics.
/// - `bytes`: Destination byte buffer.
///
/// # Returns
/// Index of the last consumed hex digit.
///
/// # Errors
/// Returns [`MimeError`] when the escape has no hex digit.
fn decode_hex_escape(
    chars: &[char],
    mut index: usize,
    source: &str,
    bytes: &mut Vec<u8>,
) -> Result<usize, MimeError> {
    let mut value = 0u8;
    let mut digits = 0;
    while index + 1 < chars.len() && digits < 2 {
        let Some(digit) = chars[index + 1].to_digit(16) else {
            break;
        };
        value = (value << 4) | digit as u8;
        digits += 1;
        index += 1;
    }
    if digits == 0 {
        return Err(MimeError::invalid_attr(
            "match",
            "value",
            source,
            "hex escape requires at least one digit",
        ));
    }
    bytes.push(value);
    Ok(index)
}

/// Decodes an octal escape.
///
/// # Parameters
/// - `chars`: Source characters.
/// - `index`: Index of the first octal digit.
/// - `bytes`: Destination byte buffer.
///
/// # Returns
/// Index of the last consumed octal digit.
fn decode_octal_escape(chars: &[char], mut index: usize, bytes: &mut Vec<u8>) -> usize {
    let mut value = 0u8;
    let mut digits = 0;
    while index < chars.len() && digits < 3 {
        let Some(digit) = chars[index].to_digit(8) else {
            break;
        };
        value = (value << 3) | digit as u8;
        digits += 1;
        index += 1;
    }
    bytes.push(value);
    index - 1
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
/// Returns [`MimeError`] when the value is invalid.
fn parse_numeric_bytes(value_type: MagicValueType, value: &str) -> Result<Vec<u8>, MimeError> {
    let number = parse_c_integer(value)?;
    match value_type
        .numeric_width()
        .expect("numeric parser should only receive numeric magic types")
    {
        1 => Ok(vec![number as u8]),
        2 => Ok((number as u16).to_be_bytes().to_vec()),
        4 => Ok((number as u32).to_be_bytes().to_vec()),
        _ => unreachable!("unsupported numeric magic width"),
    }
}

/// Parses a C-style integer literal.
///
/// # Parameters
/// - `value`: Number text.
///
/// # Returns
/// Parsed integer as `u64`.
///
/// # Errors
/// Returns [`MimeError`] when parsing fails.
fn parse_c_integer(value: &str) -> Result<u64, MimeError> {
    let trimmed = value.trim();
    let (radix, digits) = if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        (16, hex)
    } else if trimmed.len() > 1 && trimmed.starts_with('0') {
        (8, &trimmed[1..])
    } else {
        (10, trimmed)
    };
    u64::from_str_radix(digits, radix).map_err(|error| {
        MimeError::invalid_attr("match", "value", value, format!("invalid integer: {error}"))
    })
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
/// Returns [`MimeError`] when the value is not an even-length hex string.
fn parse_hex_bytes(value: &str) -> Result<Vec<u8>, MimeError> {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .ok_or_else(|| {
            MimeError::invalid_attr("match", "mask", value, "string mask must start with 0x")
        })?;
    if digits.len() % 2 != 0 {
        return Err(MimeError::invalid_attr(
            "match",
            "mask",
            value,
            "hex mask must contain an even number of digits",
        ));
    }
    let mut bytes = Vec::with_capacity(digits.len() / 2);
    let mut index = 0;
    while index < digits.len() {
        let pair = &digits[index..index + 2];
        let byte = u8::from_str_radix(pair, 16).map_err(|error| {
            MimeError::invalid_attr("match", "mask", value, format!("invalid hex byte: {error}"))
        })?;
        bytes.push(byte);
        index += 2;
    }
    Ok(bytes)
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

/// Extracts a basename from a path-like string.
///
/// # Parameters
/// - `path`: Path or basename.
///
/// # Returns
/// Final path component.
fn filename_from_path(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Yields extension suffixes from longest to shortest by scanning dots.
///
/// # Parameters
/// - `filename`: Lowercase basename.
///
/// # Returns
/// Extension suffix slices such as `tar.gz` then `gz`.
fn extension_suffixes(filename: &str) -> Vec<&str> {
    filename
        .match_indices('.')
        .map(|(index, _)| &filename[index + 1..])
        .filter(|extension| !extension.is_empty())
        .collect()
}

/// Detects whether a glob is an extension pattern.
///
/// # Parameters
/// - `pattern`: Glob pattern.
///
/// # Returns
/// Extension without `*.`, or `None` when special glob syntax appears.
fn extension_pattern(pattern: &str) -> Option<&str> {
    let extension = pattern.strip_prefix("*.")?;
    if extension.is_empty()
        || extension
            .chars()
            .any(|ch| matches!(ch, '*' | '?' | '{' | '}' | '!' | '[' | ']' | '^'))
    {
        None
    } else {
        Some(extension)
    }
}

/// Detects whether a glob is a literal pattern.
///
/// # Parameters
/// - `pattern`: Glob pattern.
///
/// # Returns
/// `true` when the pattern contains no glob metacharacters.
fn is_literal_pattern(pattern: &str) -> bool {
    !pattern
        .chars()
        .any(|ch| matches!(ch, '*' | '?' | '{' | '}' | '!' | '[' | ']' | '^'))
}

/// Merges filename and content detection results.
///
/// # Parameters
/// - `from_filename`: Candidates from filename globs.
/// - `from_content`: Candidates from content magic.
///
/// # Returns
/// A single selected MIME type, or an empty vector when neither source matched.
fn merge_results<'a>(
    from_filename: Vec<&'a MimeType>,
    from_content: Vec<&'a MimeType>,
) -> Vec<&'a MimeType> {
    if from_filename.is_empty() {
        return from_content.into_iter().take(1).collect();
    }
    if from_content.is_empty() {
        return from_filename.into_iter().take(1).collect();
    }
    if let Some(common) = from_filename.iter().find(|mime_type| {
        from_content
            .iter()
            .any(|content| content.name() == mime_type.name())
    }) {
        vec![*common]
    } else {
        from_content.into_iter().take(1).collect()
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for repository parsing and merge branches.

    use super::{
        MimeRepository, extension_pattern, extension_suffixes, filename_from_path,
        is_literal_pattern, merge_results, parse_c_integer,
    };

    /// Exercises XML parsing errors and helper edge cases.
    ///
    /// # Returns
    /// Error and helper summaries.
    pub(crate) fn exercise_repository_edges() -> Vec<String> {
        let repository = MimeRepository::from_xml(
            r#"
<!DOCTYPE mime-info [
<!ELEMENT mime-info (mime-type)+>
]>
<mime-info>
  <mime-type type="text/plain">
    <comment>plain</comment>
    <glob pattern="*.txt"/>
  </mime-type>
</mime-info>
"#,
        )
        .expect("DTD-stripped repository should parse");
        let text = repository
            .get("text/plain")
            .expect("text/plain should exist");
        let from_filename = vec![text];
        let from_content = Vec::new();
        let glob_order_repository = MimeRepository::from_xml(
            r#"
<mime-info>
  <mime-type type="text/short">
    <comment>short</comment>
    <glob pattern="READ*" weight="50"/>
  </mime-type>
  <mime-type type="text/long">
    <comment>long</comment>
    <glob pattern="README*" weight="50"/>
  </mime-type>
  <mime-type type="text/tie-one">
    <comment>tie one</comment>
    <glob pattern="*.tie" weight="50"/>
  </mime-type>
  <mime-type type="text/tie-two">
    <comment>tie two</comment>
    <glob pattern="*.tie" weight="50"/>
  </mime-type>
  <mime-type type="application/magic-one">
    <comment>magic one</comment>
    <magic priority="50"><match type="string" value="TIE" offset="0"/></magic>
  </mime-type>
  <mime-type type="application/magic-two">
    <comment>magic two</comment>
    <magic priority="50"><match type="string" value="TIE" offset="0"/></magic>
  </mime-type>
</mime-info>
"#,
        )
        .expect("glob ordering repository should parse");
        let mut result = vec![
            MimeRepository::from_xml("<bad/>")
                .expect_err("bad root should fail")
                .to_string(),
            MimeRepository::from_xml("<mime-info><mime-type><comment>x</comment></mime-type></mime-info>")
                .expect_err("missing type should fail")
                .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><glob pattern="*.x" weight="101"/></mime-type></mime-info>"#,
            )
            .expect_err("bad weight should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><glob pattern="*.x" weight="abc"/></mime-type></mime-info>"#,
            )
            .expect_err("non-numeric weight should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><glob pattern="*.x" case-sensitive="maybe"/></mime-type></mime-info>"#,
            )
            .expect_err("bad bool should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic priority="101"><match type="string" value="x" offset="0"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("bad priority should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic/></mime-type></mime-info>"#,
            )
            .expect_err("empty magic should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="unknown" value="x" offset="0"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("unknown matcher type should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" offset="2:1"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("inverted offset should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" offset="bad"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("bad offset should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="\" offset="0"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("trailing escape should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="\x" offset="0"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("bad hex escape should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" mask="ff" offset="0"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("bad string mask prefix should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" mask="0xf" offset="0"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("odd string mask should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" mask="0xgg" offset="0"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("bad string mask byte should fail")
            .to_string(),
            MimeRepository::from_xml(
                r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="byte" value="bad" offset="0"/></magic></mime-type></mime-info>"#,
            )
            .expect_err("bad numeric value should fail")
            .to_string(),
            parse_c_integer("020").expect("octal integer should parse").to_string(),
            parse_c_integer("0x10").expect("hex integer should parse").to_string(),
            filename_from_path("C:\\tmp\\file.txt").to_owned(),
            extension_suffixes("archive.tar.gz").join(","),
            extension_pattern("*.txt").unwrap_or("none").to_owned(),
            extension_pattern("*.bad[")
                .unwrap_or("none")
                .to_owned(),
            is_literal_pattern("Makefile").to_string(),
            is_literal_pattern("README*").to_string(),
            match glob_order_repository
                .detect_by_filename("README.md")
                .first()
            {
                Some(mime_type) => mime_type.name().to_owned(),
                None => "none".to_owned(),
            },
            glob_order_repository
                .detect_by_filename("file.tie")
                .len()
                .to_string(),
            glob_order_repository
                .detect_by_content(b"TIE")
                .len()
                .to_string(),
            match merge_results(from_filename, from_content).first() {
                Some(mime_type) => mime_type.name().to_owned(),
                None => "none".to_owned(),
            },
        ];
        result.push(
            MimeRepository::from_xml("<!DOCTYPE mime-info [ <mime-info>")
                .expect_err("malformed DTD should fail")
                .to_string(),
        );
        result
    }
}
