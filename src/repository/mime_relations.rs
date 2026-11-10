// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Immutable MIME name and inheritance context shared with detector cores.

use std::collections::HashMap;
use std::collections::HashSet;

use super::mime_type::MimeType;

/// The small part of a repository needed to compare MIME candidate relations.
#[derive(Debug, Clone, Default)]
pub(crate) struct MimeRelations {
    canonical_names: HashMap<String, String>,
    super_types: HashMap<String, Vec<String>>,
}

impl MimeRelations {
    /// Builds the relation context from parsed MIME types.
    pub(crate) fn from_mime_types(mime_types: &[MimeType]) -> Self {
        let mut relations = Self::default();
        for mime_type in mime_types {
            let canonical = normalize(mime_type.name());
            relations
                .canonical_names
                .insert(canonical.clone(), canonical.clone());
            for alias in mime_type.aliases() {
                relations
                    .canonical_names
                    .insert(normalize(alias), canonical.clone());
            }
            relations.super_types.insert(
                canonical,
                mime_type
                    .super_types()
                    .iter()
                    .map(|name| normalize(name))
                    .collect(),
            );
        }
        relations
    }

    /// Tests whether `child` equals or transitively inherits from `parent`.
    pub(crate) fn is_a(&self, child: &str, parent: &str) -> bool {
        let child = self.canonical_name_or_input(child);
        let parent = self.canonical_name_or_input(parent);
        if child == parent {
            return true;
        }
        if parent == "application/octet-stream" && self.canonical_names.contains_key(&child) {
            return true;
        }
        let mut pending = vec![child];
        let mut visited = HashSet::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }
            if name.starts_with("text/") && parent == "text/plain" {
                return true;
            }
            for super_type in self.super_types.get(&name).into_iter().flatten() {
                let canonical = self.canonical_name_or_input(super_type);
                if canonical == parent {
                    return true;
                }
                pending.push(canonical);
            }
        }
        false
    }

    fn canonical_name_or_input(&self, name: &str) -> String {
        let normalized = normalize(name);
        self.canonical_names
            .get(&normalized)
            .cloned()
            .unwrap_or(normalized)
    }
}

fn normalize(name: &str) -> String {
    name.to_lowercase()
}
