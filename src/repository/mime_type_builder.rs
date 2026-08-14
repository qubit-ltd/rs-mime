// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Builder for MIME type metadata.

use std::collections::HashMap;

use crate::MimeGlob;
use crate::MimeMagic;
use crate::MimeType;

/// Builder for [`MimeType`].
#[derive(Debug, Clone)]
pub struct MimeTypeBuilder {
    name: String,
    descriptions: HashMap<String, String>,
    aliases: Vec<String>,
    globs: Vec<MimeGlob>,
    magics: Vec<MimeMagic>,
    super_types: Vec<String>,
}

impl MimeTypeBuilder {
    /// Creates a MIME type builder.
    ///
    /// # Parameters
    /// - `name`: Canonical MIME type name.
    ///
    /// # Returns
    /// A new builder.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            descriptions: HashMap::new(),
            aliases: Vec::new(),
            globs: Vec::new(),
            magics: Vec::new(),
            super_types: Vec::new(),
        }
    }

    /// Adds a localized description.
    ///
    /// # Parameters
    /// - `language`: Language key such as `en`; use an empty string for the
    ///   default.
    /// - `description`: Human-readable description.
    ///
    /// # Returns
    /// The updated builder.
    pub fn description(mut self, language: &str, description: &str) -> Self {
        self.descriptions
            .insert(language.to_owned(), description.to_owned());
        self
    }

    /// Adds an alias.
    ///
    /// # Parameters
    /// - `alias`: Alias MIME type name.
    ///
    /// # Returns
    /// The updated builder.
    pub fn alias(mut self, alias: &str) -> Self {
        self.aliases.push(alias.to_owned());
        self
    }

    /// Adds a filename glob.
    ///
    /// # Parameters
    /// - `glob`: Glob rule to associate with this type.
    ///
    /// # Returns
    /// The updated builder.
    pub fn glob(mut self, glob: MimeGlob) -> Self {
        self.globs.push(glob);
        self
    }

    /// Adds a magic rule.
    ///
    /// # Parameters
    /// - `magic`: Magic rule to associate with this type.
    ///
    /// # Returns
    /// The updated builder.
    pub fn magic(mut self, magic: MimeMagic) -> Self {
        self.magics.push(magic);
        self
    }

    /// Adds a super type.
    ///
    /// # Parameters
    /// - `super_type`: Parent MIME type name.
    ///
    /// # Returns
    /// The updated builder.
    pub fn super_type(mut self, super_type: &str) -> Self {
        self.super_types.push(super_type.to_owned());
        self
    }

    /// Builds the MIME type.
    ///
    /// # Returns
    /// A [`MimeType`] containing the accumulated metadata.
    pub fn build(self) -> MimeType {
        MimeType::from_parts(
            self.name,
            self.descriptions,
            self.aliases,
            self.globs,
            self.magics,
            self.super_types,
        )
    }
}
