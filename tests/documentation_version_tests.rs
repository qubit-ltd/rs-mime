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

#[test]
fn test_user_guides_match_current_package_versions() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mime = manifest_field(&read(root.join("Cargo.toml")), "version");
    let magika_manifest = root
        .ancestors()
        .map(|ancestor| ancestor.join("rs-magika/Cargo.toml"))
        .find(|candidate| candidate.is_file())
        .expect("the local rs-magika manifest is available beside the workspace");
    let magika_manifest_text = read(&magika_manifest);
    let magika = manifest_field(&magika_manifest_text, "version");
    let mime_dependency = magika_manifest_text
        .split("[dependencies]")
        .nth(1)
        .expect("rs-magika has a [dependencies] section")
        .lines()
        .find(|line| line.trim_start().starts_with("qubit-mime = "))
        .expect("rs-magika depends on qubit-mime");
    let mime_requirement = mime.split('.').take(2).collect::<Vec<_>>().join(".");
    assert!(
        mime_dependency.contains(&format!("version = \"{mime_requirement}\"")),
        "local rs-magika manifest must accept this qubit-mime version"
    );

    for guide in ["doc/user_guide.md", "doc/user_guide.zh_CN.md"] {
        let contents = read(root.join(guide));
        assert!(
            contents.contains(&format!("qubit-mime` {mime_requirement}"))
                && contents.contains(&format!("qubit-mime = \"{}", mime_requirement))
                && contents.contains(&format!(
                    "qubit-magika = {{ version = \"{}",
                    magika.split('.').take(2).collect::<Vec<_>>().join(".")
                )),
            "{guide} install or inventory examples are stale"
        );
    }
}
