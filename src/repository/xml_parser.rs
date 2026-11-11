// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! XML preprocessing helpers for the MIME repository.

use std::borrow::Cow;

const DOCTYPE: &str = "<!DOCTYPE";

/// Removes an optional XML doctype declaration from the XML prolog.
///
/// Comments and processing instructions are skipped while scanning the
/// prolog. The declaration is removed without changing any other input bytes;
/// incomplete declarations and doctype-like text outside the prolog are left
/// for the XML parser to handle.
///
/// # Parameters
/// - `xml`: XML source that may contain a doctype declaration.
///
/// # Returns
/// The original XML borrowed when no complete declaration is found, or an
/// owned string with the declaration removed.
#[must_use]
pub(crate) fn strip_doctype(xml: &str) -> Cow<'_, str> {
    let bytes = xml.as_bytes();
    let mut position = usize::from(bytes.starts_with(&[0xEF, 0xBB, 0xBF])) * 3;

    loop {
        while position < bytes.len() && is_xml_whitespace(bytes[position]) {
            position += 1;
        }
        if bytes[position..].starts_with(b"<!--") {
            let Some(end) = xml_comment_end(bytes, position) else {
                return Cow::Borrowed(xml);
            };
            position = end;
        } else if bytes[position..].starts_with(b"<?") {
            let Some(end) = find_bytes(bytes, position + 2, b"?>") else {
                return Cow::Borrowed(xml);
            };
            position = end + 2;
        } else if bytes[position..].starts_with(DOCTYPE.as_bytes()) {
            let Some(end) = doctype_end(bytes, position + DOCTYPE.len()) else {
                return Cow::Borrowed(xml);
            };
            if !std::str::from_utf8(&bytes[position..end])
                .is_ok_and(|declaration| declaration.chars().all(is_xml_character))
            {
                return Cow::Borrowed(xml);
            }
            if has_repeated_doctype(bytes, end) {
                return Cow::Borrowed(xml);
            }
            let mut cleaned = String::with_capacity(xml.len() - (end - position));
            cleaned.push_str(&xml[..position]);
            cleaned.push_str(&xml[end..]);
            return Cow::Owned(cleaned);
        } else {
            // The root element (or any other markup) ends the prolog scan.
            return Cow::Borrowed(xml);
        }
    }
}

/// Checks whether another doctype appears before the root element.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset immediately after the first complete doctype.
///
/// # Returns
/// `true` when another doctype or an incomplete prolog comment/instruction is
/// encountered before the root element.
fn has_repeated_doctype(bytes: &[u8], mut position: usize) -> bool {
    loop {
        while position < bytes.len() && is_xml_whitespace(bytes[position]) {
            position += 1;
        }
        if bytes[position..].starts_with(b"<!--") {
            let Some(end) = xml_comment_end(bytes, position) else {
                return true;
            };
            position = end;
        } else if bytes[position..].starts_with(b"<?") {
            let Some(end) = find_bytes(bytes, position + 2, b"?>") else {
                return true;
            };
            position = end + 2;
        } else {
            return bytes[position..].starts_with(DOCTYPE.as_bytes());
        }
    }
}

/// Tests whether a byte is XML whitespace.
///
/// # Parameters
/// - `byte`: Byte to test.
///
/// # Returns
/// `true` for space, tab, carriage return, or line feed.
fn is_xml_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// Tests whether a character belongs to the XML 1.0 `Char` production.
///
/// # Parameters
/// - `character`: Unicode scalar value to test.
///
/// # Returns
/// `true` when XML 1.0 permits the character in source text.
fn is_xml_character(character: char) -> bool {
    matches!(
        character,
        '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}'
            | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

/// Skips XML whitespace from a mutable byte offset.
///
/// # Parameters
/// - `bytes`: Input to scan.
/// - `position`: Mutable offset to advance past whitespace.
///
/// # Returns
/// `true` when at least one byte was skipped.
fn skip_xml_whitespace(bytes: &[u8], position: &mut usize) -> bool {
    let start = *position;
    while bytes.get(*position).is_some_and(|byte| is_xml_whitespace(*byte)) {
        *position += 1;
    }
    *position != start
}

/// Finds the end of a quoted XML external identifier literal.
///
/// # Parameters
/// - `bytes`: Input containing the quoted literal.
/// - `position`: Offset of the opening quote.
/// - `public_identifier`: Whether to validate XML public-identifier characters.
///
/// # Returns
/// The offset after the closing quote, or `None` for malformed input.
fn quoted_literal_end(bytes: &[u8], position: usize, public_identifier: bool) -> Option<usize> {
    let quote = *bytes.get(position)?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let contents = &bytes[position + 1..];
    let end = contents.iter().position(|byte| *byte == quote)?;
    if public_identifier && !contents[..end].iter().copied().all(is_public_identifier_char) {
        return None;
    }
    Some(position + end + 2)
}

/// Tests whether a byte is allowed in an XML public identifier literal.
///
/// # Parameters
/// - `byte`: Byte to test.
///
/// # Returns
/// `true` when the byte belongs to the XML `PubidChar` set.
fn is_public_identifier_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b' ' | b'\r'
                | b'\n'
                | b'-'
                | b'\''
                | b'('
                | b')'
                | b'+'
                | b','
                | b'.'
                | b'/'
                | b':'
                | b'='
                | b'?'
                | b';'
                | b'!'
                | b'*'
                | b'#'
                | b'@'
                | b'$'
                | b'_'
                | b'%'
        )
}

/// Finds the first occurrence of a byte sequence at or after a position.
///
/// # Parameters
/// - `bytes`: Input to search.
/// - `position`: Starting byte offset.
/// - `needle`: Byte sequence to find.
///
/// # Returns
/// The starting offset of the match, or `None` when it is absent.
fn find_bytes(bytes: &[u8], mut position: usize, needle: &[u8]) -> Option<usize> {
    while position + needle.len() <= bytes.len() {
        if bytes[position..].starts_with(needle) {
            return Some(position);
        }
        position += 1;
    }
    None
}

/// Finds a well-formed XML comment's end, rejecting internal double hyphens.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset at the comment's opening `<!--`.
///
/// # Returns
/// The offset after `-->`, or `None` for an incomplete or malformed comment.
fn xml_comment_end(bytes: &[u8], position: usize) -> Option<usize> {
    let content_start = position + 4;
    let end = find_bytes(bytes, content_start, b"-->")?;
    if find_bytes(bytes, content_start, b"--").is_some_and(|hyphen| hyphen != end) {
        return None;
    }
    Some(end + 3)
}

/// Finds the byte immediately after a complete doctype declaration.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset immediately after the `<!DOCTYPE` token.
///
/// # Returns
/// The offset after the declaration, or `None` when it is incomplete or
/// malformed.
fn doctype_end(bytes: &[u8], mut position: usize) -> Option<usize> {
    if bytes.get(position).is_none_or(|byte| !is_xml_whitespace(*byte)) {
        return None;
    }
    while position < bytes.len() && is_xml_whitespace(bytes[position]) {
        position += 1;
    }
    let name = std::str::from_utf8(bytes.get(position..)?).ok()?;
    let mut characters = name.char_indices();
    let (_, first) = characters.next()?;
    if !is_xml_name_start(first) {
        return None;
    }
    let mut name_end = first.len_utf8();
    for (offset, character) in characters {
        if !is_xml_name_char(character) {
            break;
        }
        name_end = offset + character.len_utf8();
    }
    position += name_end;
    let had_space = skip_xml_whitespace(bytes, &mut position);
    if bytes.get(position) == Some(&b'>') {
        return Some(position + 1);
    }
    let in_subset;
    if bytes.get(position) == Some(&b'[') {
        in_subset = true;
        position += 1;
    } else {
        if !had_space {
            return None;
        }
        let external_kind = if bytes[position..].starts_with(b"SYSTEM") {
            position += b"SYSTEM".len();
            1
        } else if bytes[position..].starts_with(b"PUBLIC") {
            position += b"PUBLIC".len();
            2
        } else {
            return None;
        };
        if bytes.get(position).is_some_and(|byte| !is_xml_whitespace(*byte))
            || !skip_xml_whitespace(bytes, &mut position)
        {
            return None;
        }
        position = quoted_literal_end(bytes, position, external_kind == 2)?;
        if external_kind == 2 {
            if !skip_xml_whitespace(bytes, &mut position) {
                return None;
            }
            position = quoted_literal_end(bytes, position, false)?;
        }
        skip_xml_whitespace(bytes, &mut position);
        if bytes.get(position) == Some(&b'[') {
            in_subset = true;
            position += 1;
        } else if bytes.get(position) == Some(&b'>') {
            return Some(position + 1);
        } else {
            return None;
        }
    }

    if in_subset {
        position = internal_subset_end(bytes, position)?;
        skip_xml_whitespace(bytes, &mut position);
    }
    (bytes.get(position) == Some(&b'>')).then_some(position + 1)
}

/// Scans the top-level items in an XML internal subset.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset immediately after the subset's opening bracket.
///
/// # Returns
/// The offset after the closing bracket, or `None` for an incomplete or
/// unrecognized item.
fn internal_subset_end(bytes: &[u8], mut position: usize) -> Option<usize> {
    loop {
        skip_xml_whitespace(bytes, &mut position);
        if bytes.get(position) == Some(&b']') {
            return Some(position + 1);
        }
        if bytes[position..].starts_with(b"<!--") {
            position = xml_comment_end(bytes, position)?;
        } else if bytes[position..].starts_with(b"<?") {
            position = find_bytes(bytes, position + 2, b"?>")? + 2;
        } else if bytes[position..].starts_with(b"<!") {
            position = markup_declaration_end(bytes, position)?;
        } else if bytes.get(position) == Some(&b'%') {
            position += 1;
            let name_end = xml_name_end(bytes, position)?;
            position = name_end;
            if bytes.get(position) != Some(&b';') {
                return None;
            }
            position += 1;
        } else {
            return None;
        }
    }
}

/// Scans a DTD markup declaration after checking its declaration keyword.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset at the declaration's opening `<`.
///
/// # Returns
/// The offset after its unquoted `>`, or `None` for malformed input.
fn markup_declaration_end(bytes: &[u8], mut position: usize) -> Option<usize> {
    position += 2;
    let declarations = [
        (b"ELEMENT".as_slice(), 0),
        (b"ATTLIST", 1),
        (b"ENTITY", 2),
        (b"NOTATION", 3),
    ];
    let (keyword, kind) = declarations
        .into_iter()
        .find(|(keyword, _)| bytes[position..].starts_with(keyword))?;
    position += keyword.len();
    if !bytes.get(position).is_some_and(|byte| is_xml_whitespace(*byte)) {
        return None;
    }
    skip_xml_whitespace(bytes, &mut position);
    let parameter_entity = kind == 2 && bytes.get(position) == Some(&b'%');
    if parameter_entity {
        position += 1;
        if !skip_xml_whitespace(bytes, &mut position) {
            return None;
        }
    }
    position = xml_name_end(bytes, position)?;
    if !skip_xml_whitespace(bytes, &mut position) {
        return None;
    }
    let payload_is_valid = match kind {
        0 => {
            bytes[position..].starts_with(b"EMPTY")
                || bytes[position..].starts_with(b"ANY")
                || bytes.get(position) == Some(&b'(')
        }
        1 => xml_name_end(bytes, position).is_some(),
        2 => {
            matches!(bytes.get(position), Some(b'\'' | b'"'))
                || bytes[position..].starts_with(b"SYSTEM")
                || bytes[position..].starts_with(b"PUBLIC")
        }
        3 => bytes[position..].starts_with(b"SYSTEM") || bytes[position..].starts_with(b"PUBLIC"),
        _ => false,
    };
    if !payload_is_valid {
        return None;
    }

    match kind {
        1 => return attlist_declaration_end(bytes, position),
        0 if bytes[position..].starts_with(b"EMPTY") => {
            position += b"EMPTY".len();
            return simple_keyword_declaration_end(bytes, position);
        }
        0 if bytes[position..].starts_with(b"ANY") => {
            position += b"ANY".len();
            return simple_keyword_declaration_end(bytes, position);
        }
        0 if bytes.get(position) == Some(&b'(') => {
            position = element_content_spec_end(bytes, position)?;
            return simple_keyword_declaration_end(bytes, position);
        }
        2 => return entity_declaration_end(bytes, position, parameter_entity),
        3 if bytes[position..].starts_with(b"SYSTEM") => {
            position += b"SYSTEM".len();
            if !skip_xml_whitespace(bytes, &mut position) {
                return None;
            }
            position = quoted_literal_end(bytes, position, false)?;
            return simple_keyword_declaration_end(bytes, position);
        }
        3 if bytes[position..].starts_with(b"PUBLIC") => {
            position += b"PUBLIC".len();
            if !skip_xml_whitespace(bytes, &mut position) {
                return None;
            }
            position = quoted_literal_end(bytes, position, true)?;
            if skip_xml_whitespace(bytes, &mut position) && matches!(bytes.get(position), Some(b'\'' | b'"')) {
                position = quoted_literal_end(bytes, position, false)?;
            }
            return simple_keyword_declaration_end(bytes, position);
        }
        _ => {}
    }

    let mut quote = None;
    while let Some(&byte) = bytes.get(position) {
        match (quote, byte) {
            (Some(current), value) if value == current => quote = None,
            (Some(_), _) => {}
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'<') => return None,
            (None, b'>') => return Some(position + 1),
            _ => {}
        }
        position += 1;
    }
    None
}

/// Scans an XML element content specification.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset at the content specification's opening parenthesis.
///
/// # Returns
/// The offset after the content specification, or `None` for invalid input.
fn element_content_spec_end(bytes: &[u8], mut position: usize) -> Option<usize> {
    let group_start = position;
    position += 1;
    skip_xml_whitespace(bytes, &mut position);
    if bytes[position..].starts_with(b"#PCDATA") {
        position += b"#PCDATA".len();
        let mut has_names = false;
        loop {
            skip_xml_whitespace(bytes, &mut position);
            if bytes.get(position) == Some(&b'|') {
                position += 1;
                skip_xml_whitespace(bytes, &mut position);
                position = xml_name_end(bytes, position)?;
                has_names = true;
            } else if bytes.get(position) == Some(&b')') {
                position += 1;
                if has_names {
                    if bytes.get(position) != Some(&b'*') {
                        return None;
                    }
                    position += 1;
                }
                return Some(position);
            } else {
                return None;
            }
        }
    }
    position = children_group_end(bytes, group_start, 0)?;
    if matches!(bytes.get(position), Some(b'?' | b'*' | b'+')) {
        position += 1;
    }
    Some(position)
}

/// Scans a `children` content group and checks its separators and particles.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset at the group's opening parenthesis.
/// - `depth`: Current nested-group depth.
///
/// # Returns
/// The offset after the group, or `None` for invalid or deeply nested input.
fn children_group_end(bytes: &[u8], mut position: usize, depth: usize) -> Option<usize> {
    if depth >= 128 || bytes.get(position) != Some(&b'(') {
        return None;
    }
    position += 1;
    skip_xml_whitespace(bytes, &mut position);
    position = content_particle_end(bytes, position, depth + 1)?;
    let mut separator = None;
    loop {
        skip_xml_whitespace(bytes, &mut position);
        match bytes.get(position) {
            Some(b')') => return Some(position + 1),
            Some(b'|' | b',') => {
                let current = bytes[position];
                if separator.is_some_and(|expected| expected != current) {
                    return None;
                }
                separator = Some(current);
                position += 1;
                skip_xml_whitespace(bytes, &mut position);
                position = content_particle_end(bytes, position, depth + 1)?;
            }
            _ => return None,
        }
    }
}

/// Scans one element content particle, including its optional occurrence.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset at the particle's first character.
/// - `depth`: Current nested-group depth.
///
/// # Returns
/// The offset after the particle, or `None` for invalid input.
fn content_particle_end(bytes: &[u8], mut position: usize, depth: usize) -> Option<usize> {
    if bytes.get(position) == Some(&b'(') {
        position = children_group_end(bytes, position, depth)?;
    } else {
        position = xml_name_end(bytes, position)?;
    }
    if matches!(bytes.get(position), Some(b'?' | b'*' | b'+')) {
        position += 1;
    }
    Some(position)
}

/// Scans the attribute definitions in an ATTLIST declaration.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset immediately after the element name and whitespace.
///
/// # Returns
/// The offset after `>`, or `None` when a definition is incomplete.
fn attlist_declaration_end(bytes: &[u8], mut position: usize) -> Option<usize> {
    loop {
        skip_xml_whitespace(bytes, &mut position);
        if bytes.get(position) == Some(&b'>') {
            return Some(position + 1);
        }

        position = xml_name_end(bytes, position)?;
        if !skip_xml_whitespace(bytes, &mut position) {
            return None;
        }
        if bytes[position..].starts_with(b"NOTATION") {
            position += b"NOTATION".len();
            if !skip_xml_whitespace(bytes, &mut position) {
                return None;
            }
            position = enumeration_end(bytes, position, true)?;
        } else if bytes.get(position) == Some(&b'(') {
            position = enumeration_end(bytes, position, false)?;
        } else {
            let value_types = [
                b"CDATA".as_slice(),
                b"IDREFS",
                b"IDREF",
                b"ID",
                b"ENTITIES",
                b"ENTITY",
                b"NMTOKENS",
                b"NMTOKEN",
            ];
            let value_type = value_types
                .into_iter()
                .find(|value_type| bytes[position..].starts_with(value_type))?;
            position += value_type.len();
            if bytes.get(position).is_some_and(|byte| !is_xml_whitespace(*byte)) {
                return None;
            }
        }
        if !skip_xml_whitespace(bytes, &mut position) {
            return None;
        }

        if bytes[position..].starts_with(b"#REQUIRED") {
            position = require_dtd_keyword_end(bytes, position + b"#REQUIRED".len())?;
        } else if bytes[position..].starts_with(b"#IMPLIED") {
            position = require_dtd_keyword_end(bytes, position + b"#IMPLIED".len())?;
        } else if bytes[position..].starts_with(b"#FIXED") {
            position += b"#FIXED".len();
            if !skip_xml_whitespace(bytes, &mut position) {
                return None;
            }
            position = attribute_value_end(bytes, position)?;
        } else {
            position = attribute_value_end(bytes, position)?;
        }
    }
}

/// Scans a DTD enumeration enclosed in parentheses.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset at the opening parenthesis.
/// - `name_only`: Whether members use the XML `Name` rule instead of `Nmtoken`.
///
/// # Returns
/// The offset after the closing parenthesis, or `None` for invalid input.
fn enumeration_end(bytes: &[u8], mut position: usize, name_only: bool) -> Option<usize> {
    if bytes.get(position) != Some(&b'(') {
        return None;
    }
    position += 1;
    loop {
        skip_xml_whitespace(bytes, &mut position);
        position = if name_only {
            xml_name_end(bytes, position)?
        } else {
            xml_nmtoken_end(bytes, position)?
        };
        skip_xml_whitespace(bytes, &mut position);
        match bytes.get(position) {
            Some(b'|') => position += 1,
            Some(b')') => return Some(position + 1),
            _ => return None,
        }
    }
}

/// Requires a DTD keyword to be followed by whitespace or `>`.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset immediately after the keyword.
///
/// # Returns
/// The unchanged offset, or `None` when the keyword has no delimiter.
fn require_dtd_keyword_end(bytes: &[u8], position: usize) -> Option<usize> {
    if bytes
        .get(position)
        .is_some_and(|byte| !is_xml_whitespace(*byte) && *byte != b'>')
    {
        return None;
    }
    Some(position)
}

/// Scans a quoted attribute default value.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset at the opening quote.
///
/// # Returns
/// The offset after the closing quote, or `None` for invalid input.
fn attribute_value_end(bytes: &[u8], position: usize) -> Option<usize> {
    let end = quoted_literal_end(bytes, position, false)?;
    (!bytes[position..end].contains(&b'<')).then_some(end)
}

/// Skips optional whitespace and requires a declaration terminator.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset immediately after the declaration's payload.
///
/// # Returns
/// The offset after `>`, or `None` when other tokens follow the payload.
fn simple_keyword_declaration_end(bytes: &[u8], mut position: usize) -> Option<usize> {
    skip_xml_whitespace(bytes, &mut position);
    (bytes.get(position) == Some(&b'>')).then_some(position + 1)
}

/// Scans an XML entity definition after its name.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset immediately after the entity name and whitespace.
/// - `parameter_entity`: Whether this is a parameter-entity declaration.
///
/// # Returns
/// The offset after `>`, or `None` when the definition is incomplete.
fn entity_declaration_end(bytes: &[u8], mut position: usize, parameter_entity: bool) -> Option<usize> {
    if matches!(bytes.get(position), Some(b'\'' | b'"')) {
        position = quoted_literal_end(bytes, position, false)?;
        return simple_keyword_declaration_end(bytes, position);
    }

    let public_identifier = if bytes[position..].starts_with(b"SYSTEM") {
        position += b"SYSTEM".len();
        false
    } else if bytes[position..].starts_with(b"PUBLIC") {
        position += b"PUBLIC".len();
        true
    } else {
        return None;
    };
    if !skip_xml_whitespace(bytes, &mut position) {
        return None;
    }
    position = quoted_literal_end(bytes, position, public_identifier)?;
    if public_identifier {
        if !skip_xml_whitespace(bytes, &mut position) {
            return None;
        }
        position = quoted_literal_end(bytes, position, false)?;
    }
    if skip_xml_whitespace(bytes, &mut position) && !parameter_entity && bytes[position..].starts_with(b"NDATA") {
        position += b"NDATA".len();
        if !skip_xml_whitespace(bytes, &mut position) {
            return None;
        }
        position = xml_name_end(bytes, position)?;
        return simple_keyword_declaration_end(bytes, position);
    }
    simple_keyword_declaration_end(bytes, position)
}

/// Finds the end of an XML name at a byte offset.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset of the name's first character.
///
/// # Returns
/// The offset after the name, or `None` when no valid name starts there.
fn xml_name_end(bytes: &[u8], position: usize) -> Option<usize> {
    let name = std::str::from_utf8(bytes.get(position..)?).ok()?;
    let mut characters = name.char_indices();
    let (_, first) = characters.next()?;
    if !is_xml_name_start(first) {
        return None;
    }
    let mut end = position + first.len_utf8();
    for (offset, character) in characters {
        if !is_xml_name_char(character) {
            break;
        }
        end = position + offset + character.len_utf8();
    }
    Some(end)
}

/// Finds the end of an XML `Nmtoken` at a byte offset.
///
/// # Parameters
/// - `bytes`: XML source bytes.
/// - `position`: Offset of the token's first character.
///
/// # Returns
/// The offset after the token, or `None` when no valid token starts there.
fn xml_nmtoken_end(bytes: &[u8], position: usize) -> Option<usize> {
    let token = std::str::from_utf8(bytes.get(position..)?).ok()?;
    let mut end = position;
    for (offset, character) in token.char_indices() {
        if !is_xml_name_char(character) {
            break;
        }
        end = position + offset + character.len_utf8();
    }
    (end > position).then_some(end)
}

/// Tests whether a character can begin an XML name.
///
/// # Parameters
/// - `character`: Unicode scalar value to test.
///
/// # Returns
/// `true` when it belongs to the XML 1.0 `NameStartChar` ranges.
fn is_xml_name_start(character: char) -> bool {
    matches!(
        character,
        ':' | '_'
            | 'A'..='Z'
            | 'a'..='z'
            | '\u{C0}'..='\u{D6}'
            | '\u{D8}'..='\u{F6}'
            | '\u{F8}'..='\u{2FF}'
            | '\u{370}'..='\u{37D}'
            | '\u{37F}'..='\u{1FFF}'
            | '\u{200C}'..='\u{200D}'
            | '\u{2070}'..='\u{218F}'
            | '\u{2C00}'..='\u{2FEF}'
            | '\u{3001}'..='\u{D7FF}'
            | '\u{F900}'..='\u{FDCF}'
            | '\u{FDF0}'..='\u{FFFD}'
            | '\u{10000}'..='\u{EFFFF}'
    )
}

/// Tests whether a character can continue an XML name.
///
/// # Parameters
/// - `character`: Unicode scalar value to test.
///
/// # Returns
/// `true` when it belongs to the XML 1.0 `NameChar` ranges.
fn is_xml_name_char(character: char) -> bool {
    is_xml_name_start(character)
        || matches!(
            character,
            '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}'
        )
}
