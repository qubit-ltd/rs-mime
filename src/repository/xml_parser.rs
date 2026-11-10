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
            let Some(end) = find_bytes(bytes, position + 4, b"-->") else {
                return Cow::Borrowed(xml);
            };
            position = end + 3;
        } else if bytes[position..].starts_with(b"<?") {
            let Some(end) = find_bytes(bytes, position + 2, b"?>") else {
                return Cow::Borrowed(xml);
            };
            position = end + 2;
        } else if bytes[position..].starts_with(DOCTYPE.as_bytes()) {
            let Some(end) = doctype_end(bytes, position + DOCTYPE.len()) else {
                return Cow::Borrowed(xml);
            };
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

fn is_xml_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn find_bytes(bytes: &[u8], mut position: usize, needle: &[u8]) -> Option<usize> {
    while position + needle.len() <= bytes.len() {
        if bytes[position..].starts_with(needle) {
            return Some(position);
        }
        position += 1;
    }
    None
}

/// Finds the byte immediately after a complete doctype declaration.
fn doctype_end(bytes: &[u8], mut position: usize) -> Option<usize> {
    let mut quote = None;
    let mut in_subset = false;
    let mut subset_closed = false;

    while position < bytes.len() {
        if quote.is_none() && in_subset && bytes[position..].starts_with(b"<!--") {
            let end = find_bytes(bytes, position + 4, b"-->")?;
            position = end + 3;
            continue;
        }

        let byte = bytes[position];
        if subset_closed && !is_xml_whitespace(byte) && byte != b'>' {
            // XML permits whitespace between the internal subset and the
            // declaration terminator, but no additional tokens.
            return None;
        }
        match (quote, byte) {
            (Some(current), value) if value == current => quote = None,
            (Some(_), _) => {}
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'[') if !in_subset && !subset_closed => in_subset = true,
            (None, b']') if in_subset => {
                in_subset = false;
                subset_closed = true;
            }
            (None, b'>') if !in_subset => return Some(position + 1),
            _ => {}
        }
        position += 1;
    }
    None
}
