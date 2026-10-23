// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! XML preprocessing helpers for the MIME repository.

use std::borrow::Cow;

/// Removes an optional XML doctype declaration before parsing MIME data.
///
/// The declaration is removed through the first unquoted `]>` after its
/// opening marker. Input without a complete declaration is returned unchanged.
///
/// # Returns
/// The original XML borrowed when no complete declaration is found, or an
/// owned string with the declaration removed.
#[must_use]
pub(crate) fn strip_doctype(xml: &str) -> Cow<'_, str> {
    let Some(start) = xml.find("<!DOCTYPE") else {
        return Cow::Borrowed(xml);
    };
    let mut quote = None;
    let mut end = None;
    for (offset, character) in xml[start..].char_indices() {
        match (quote, character) {
            (Some(current), value) if value == current => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, ']') if xml[start + offset..].starts_with("]>") => {
                end = Some(start + offset + 2);
                break;
            }
            _ => {}
        }
    }
    let Some(end) = end else {
        return Cow::Borrowed(xml);
    };
    let mut cleaned = String::with_capacity(xml.len().saturating_sub(end - start));
    cleaned.push_str(&xml[..start]);
    cleaned.push_str(&xml[end..]);
    Cow::Owned(cleaned)
}
