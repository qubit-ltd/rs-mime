// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::path::PathBuf;

fn manifest_field(manifest: &str, field: &str) -> String {
    let in_package = manifest
        .split("[package]")
        .nth(1)
        .expect("manifest has a [package] section")
        .split("\n[")
        .next()
        .expect("package section exists");
    in_package
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == field).then(|| value.trim().trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| panic!("package manifest has a {field} field"))
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref()).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.as_ref().display()))
}

fn documented_example<'a>(contents: &'a str, marker: &str) -> &'a str {
    let marked = contents
        .split_once(marker)
        .unwrap_or_else(|| panic!("missing documentation example marker {marker}"))
        .1;
    let code = marked
        .strip_prefix("\n```rust\n")
        .unwrap_or_else(|| panic!("{marker} must immediately precede a Rust code block"));
    code.split_once("\n```")
        .unwrap_or_else(|| panic!("{marker} Rust code block is not closed"))
        .0
}

fn documentation_magika_version(contents: &str) -> Result<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            (key.trim() == "magika").then(|| value.trim().trim_matches('"').to_owned())
        })
        .find(|version| !version.is_empty())
        .ok_or_else(|| "documentation versions fixture is missing the magika field".to_owned())
}

#[test]
fn test_provider_configuration_examples_match_compilable_consumer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let marker = "<!-- mime-doc-example:provider-configuration -->";
    let documents = [
        "README.md",
        "README.zh_CN.md",
        "doc/user_guide.md",
        "doc/user_guide.zh_CN.md",
    ]
    .map(|path| read(root.join(path)));
    let examples = documents
        .iter()
        .map(|document| documented_example(document, marker))
        .collect::<Vec<_>>();

    for example in examples.iter().skip(1) {
        assert_eq!(*example, examples[0], "provider configuration examples drifted");
    }

    let fixture = read(root.join("tests/fixtures/documentation_consumer/src/main.rs"));
    assert!(fixture.ends_with("fn main() {}\n"));
    let fixture_code = fixture.replace("#[allow(dead_code)]\n", "");
    assert!(
        fixture_code.contains(examples[0]),
        "consumer fixture is out of sync with documentation"
    );
}

#[test]
fn test_user_guides_match_current_package_versions() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mime = manifest_field(&read(root.join("Cargo.toml")), "version");
    let mime_requirement = mime.split('.').take(2).collect::<Vec<_>>().join(".");
    let magika_version = documentation_magika_version(&read(root.join("tests/fixtures/documentation_versions.toml")))
        .unwrap_or_else(|error| panic!("{error}"));

    for guide in ["doc/user_guide.md", "doc/user_guide.zh_CN.md"] {
        let contents = read(root.join(guide));
        assert!(
            contents.contains(&format!("qubit-mime` {mime_requirement}"))
                && contents.contains(&format!("qubit-mime = \"{}", mime_requirement))
                && contents.contains(&format!("qubit-mime = {{ version = \"{mime_requirement}\"")),
            "{guide} install or inventory examples are stale"
        );
        assert!(
            contents.contains(&format!("qubit-magika = {{ version = \"{magika_version}")),
            "{guide} inventory example does not match the documentation version fixture"
        );
    }
}

#[test]
fn test_documentation_magika_version_fixture_accepts_blank_lines_and_whitespace() {
    assert_eq!(
        documentation_magika_version("\n  \n magika  =  \"0.18\"  \n").as_deref(),
        Ok("0.18")
    );
}

#[test]
fn test_documentation_magika_version_fixture_reports_missing_field() {
    assert_eq!(
        documentation_magika_version("\n other = \"0.18\"\n")
            .as_deref()
            .map_err(String::as_str),
        Err("documentation versions fixture is missing the magika field")
    );
}
