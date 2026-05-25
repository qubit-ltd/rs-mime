/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! MIME type metadata and matching helpers.

use std::collections::{
    HashMap,
    HashSet,
};

use crate::{
    MimeGlob,
    MimeMagic,
    MimeRepository,
    MimeTypeBuilder,
};

/// Metadata and matching rules for a MIME type.
#[derive(Debug, Clone)]
pub struct MimeType {
    pub(crate) name: String,
    pub(crate) descriptions: HashMap<String, String>,
    pub(crate) aliases: Vec<String>,
    pub(crate) globs: Vec<MimeGlob>,
    pub(crate) magics: Vec<MimeMagic>,
    pub(crate) super_types: Vec<String>,
}

impl MimeType {
    /// Starts building a MIME type.
    ///
    /// # Parameters
    /// - `name`: Canonical MIME type name.
    ///
    /// # Returns
    /// A builder initialized with `name`.
    pub fn builder(name: &str) -> MimeTypeBuilder {
        MimeTypeBuilder::new(name)
    }

    /// Creates a MIME type from parsed parts.
    ///
    /// # Parameters
    /// - `name`: Canonical MIME type name.
    /// - `descriptions`: Localized descriptions by language key.
    /// - `aliases`: MIME aliases.
    /// - `globs`: Filename globs.
    /// - `magics`: Content magic rules.
    /// - `super_types`: Parent MIME types.
    ///
    /// # Returns
    /// A MIME type value.
    pub(crate) fn from_parts(
        name: String,
        descriptions: HashMap<String, String>,
        aliases: Vec<String>,
        globs: Vec<MimeGlob>,
        magics: Vec<MimeMagic>,
        super_types: Vec<String>,
    ) -> Self {
        Self {
            name,
            descriptions,
            aliases,
            globs,
            magics,
            super_types,
        }
    }

    /// Gets the canonical MIME type name.
    ///
    /// # Returns
    /// Canonical MIME type name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets a human-readable description.
    ///
    /// # Returns
    /// The default description, an English description, or any available
    /// description. Returns `None` when no descriptions are present.
    pub fn description(&self) -> Option<&str> {
        self.descriptions
            .get("")
            .or_else(|| self.descriptions.get("en"))
            .or_else(|| self.descriptions.get("en_US"))
            .or_else(|| self.descriptions.get("en_GB"))
            .or_else(|| self.descriptions.values().next())
            .map(String::as_str)
    }

    /// Gets MIME aliases.
    ///
    /// # Returns
    /// Alias names for this MIME type.
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// Gets filename globs.
    ///
    /// # Returns
    /// Glob rules associated with this MIME type.
    pub fn globs(&self) -> &[MimeGlob] {
        &self.globs
    }

    /// Gets content magic rules.
    ///
    /// # Returns
    /// Magic rules associated with this MIME type.
    pub fn magics(&self) -> &[MimeMagic] {
        &self.magics
    }

    /// Gets parent MIME type names.
    ///
    /// # Returns
    /// Super type names from `sub-class-of` XML entries.
    pub fn super_types(&self) -> &[String] {
        &self.super_types
    }

    /// Gets the preferred filename extension.
    ///
    /// # Returns
    /// The highest-weight simple extension, or the highest-weight complex
    /// extension when no simple extension exists.
    pub fn preferred_extension(&self) -> Option<&str> {
        let simple = best_extension(&self.globs, false);
        if simple.is_some() {
            simple
        } else {
            best_extension(&self.globs, true)
        }
    }

    /// Gets all extensions sorted by descending glob weight.
    ///
    /// # Returns
    /// Extension strings without leading dots.
    pub fn all_extensions(&self) -> Vec<&str> {
        let mut extensions: Vec<(&str, u16)> = self
            .globs
            .iter()
            .filter_map(|glob| extension_from_pattern(glob.pattern()).map(|ext| (ext, glob.weight())))
            .collect();
        extensions.sort_by_key(|(_, weight)| std::cmp::Reverse(*weight));
        extensions.into_iter().map(|(extension, _)| extension).collect()
    }

    /// Tests whether any glob rule matches a filename.
    ///
    /// # Parameters
    /// - `filename`: Basename to test.
    ///
    /// # Returns
    /// `true` when any glob matches.
    pub fn matches_filename(&self, filename: &str) -> bool {
        self.globs.iter().any(|glob| glob.matches(filename))
    }

    /// Tests whether this MIME type or its super types match content magic.
    ///
    /// # Parameters
    /// - `repository`: Repository used to resolve super types.
    /// - `bytes`: Content bytes to test.
    ///
    /// # Returns
    /// `true` when this type or a parent type has matching magic.
    pub fn matches_content(&self, repository: &MimeRepository, bytes: &[u8]) -> bool {
        self.matched_magic(repository, bytes, 0).is_some()
    }

    /// Gets the best matching magic rule for this type or its super types.
    ///
    /// # Parameters
    /// - `repository`: Repository used to resolve super type names.
    /// - `bytes`: Content bytes to test.
    /// - `best_priority`: Lowest priority that may improve the current result.
    ///
    /// # Returns
    /// The highest-priority matched magic rule, or `None`.
    pub(crate) fn matched_magic<'a>(
        &'a self,
        repository: &'a MimeRepository,
        bytes: &[u8],
        best_priority: u16,
    ) -> Option<&'a MimeMagic> {
        let mut visited = HashSet::new();
        self.matched_magic_inner(repository, bytes, best_priority, &mut visited)
    }

    /// Recursive implementation for matching this type and super types.
    ///
    /// # Parameters
    /// - `repository`: Repository used to resolve super type names.
    /// - `bytes`: Content bytes to test.
    /// - `best_priority`: Lowest priority that may improve the current result.
    /// - `visited`: MIME type names already visited during this lookup.
    ///
    /// # Returns
    /// The highest-priority matched magic rule, or `None`.
    fn matched_magic_inner<'a>(
        &'a self,
        repository: &'a MimeRepository,
        bytes: &[u8],
        mut best_priority: u16,
        visited: &mut HashSet<String>,
    ) -> Option<&'a MimeMagic> {
        if !visited.insert(self.name.clone()) {
            return None;
        }
        let mut result = None;
        for magic in &self.magics {
            let priority = magic.priority();
            if priority >= best_priority && magic.matches(bytes) {
                result = Some(magic);
                best_priority = priority;
            }
        }
        for parent_name in &self.super_types {
            if let Some(parent) = repository.get(parent_name)
                && let Some(magic) = parent.matched_magic_inner(repository, bytes, best_priority, visited)
            {
                best_priority = magic.priority();
                result = Some(magic);
            }
        }
        result
    }
}

impl PartialEq for MimeType {
    /// Compares MIME types by canonical name.
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for MimeType {}

/// Finds the best simple or complex extension.
///
/// # Parameters
/// - `globs`: Glob rules to scan.
/// - `complex`: Whether to select complex extensions containing dots.
///
/// # Returns
/// Best extension without the leading `*.`, or `None`.
fn best_extension(globs: &[MimeGlob], complex: bool) -> Option<&str> {
    globs
        .iter()
        .filter_map(|glob| {
            let extension = extension_from_pattern(glob.pattern())?;
            let is_complex = extension.contains('.');
            (is_complex == complex).then_some((extension, glob.weight()))
        })
        .max_by_key(|(_, weight)| *weight)
        .map(|(extension, _)| extension)
}

/// Extracts an extension from a simple `*.ext` glob pattern.
///
/// # Parameters
/// - `pattern`: Glob pattern to inspect.
///
/// # Returns
/// Extension without `*.`, or `None` when the pattern contains glob syntax.
fn extension_from_pattern(pattern: &str) -> Option<&str> {
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
