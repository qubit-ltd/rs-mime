// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashSet;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const DOCUMENTS: [&str; 4] = [
    "README.md",
    "README.zh_CN.md",
    "doc/user_guide.md",
    "doc/user_guide.zh_CN.md",
];

#[test]
fn test_packaged_markdown_links_resolve_inside_package_file_set() {
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = temporary_target_dir();
    let _target_cleanup = TargetCleanup(target_dir.clone());
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["package", "--list", "--allow-dirty", "--manifest-path"])
        .arg(package_root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .current_dir(package_root)
        .output()
        .expect("run cargo package --list");
    assert!(
        output.status.success(),
        "cargo package --list failed (status {}):\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let packaged = String::from_utf8(output.stdout)
        .expect("cargo package file list must be UTF-8")
        .lines()
        .map(normalize_package_path)
        .collect::<HashSet<_>>();
    assert!(!packaged.is_empty(), "cargo package returned an empty file list");

    for document in DOCUMENTS {
        let markdown =
            fs::read_to_string(package_root.join(document)).unwrap_or_else(|error| panic!("read {document}: {error}"));
        for target in local_markdown_links(&markdown) {
            let resolved = resolve_link(document, &target);
            assert!(
                packaged.contains(&resolved),
                "{document} links to {target:?}, which resolves to {resolved:?} but is absent from cargo package --list"
            );
        }
    }

    assert!(
        !packaged
            .iter()
            .any(|path| path.contains("implementation-plan-") || path.contains("design-20261117")),
        "the package must not include this round's design or implementation plan"
    );
}

fn local_markdown_links(markdown: &str) -> Vec<String> {
    let mut visible = String::with_capacity(markdown.len());
    let mut fenced = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            visible.push('\n');
        } else if !fenced {
            visible.push_str(line);
            visible.push('\n');
        }
    }

    let mut links = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = visible[cursor..].find("](") {
        let start = cursor + relative + 2;
        let Some(end_relative) = visible[start..].find(')') else {
            break;
        };
        let end = start + end_relative;
        let mut target = visible[start..end].trim();
        if target.starts_with('<') && target.ends_with('>') && target.len() >= 2 {
            target = &target[1..target.len() - 1];
        } else if let Some(space) = target.find(char::is_whitespace) {
            target = &target[..space];
        }
        let path = target.split(['#', '?']).next().unwrap_or_default();
        if path.ends_with(".md") && !path.contains("://") && !path.starts_with("mailto:") {
            links.push(path.to_owned());
        }
        cursor = end + 1;
    }
    links
}

fn resolve_link(document: &str, target: &str) -> String {
    let parent = Path::new(document).parent().unwrap_or_else(|| Path::new(""));
    let target = target.split(['#', '?']).next().unwrap_or_default();
    let mut resolved = PathBuf::new();
    for component in parent.join(target).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(value) => resolved.push(value),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    normalize_package_path(resolved.to_str().expect("Markdown path must be UTF-8"))
}

fn normalize_package_path(path: &str) -> String {
    path.trim_start_matches('/')
        .replace('\\', "/")
        .split('/')
        .fold(Vec::new(), |mut components, component| {
            match component {
                "" | "." => {}
                ".." => {
                    components.pop();
                }
                value => components.push(value),
            }
            components
        })
        .join("/")
}

fn temporary_target_dir() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after UNIX epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("qubit-mime-package-links-{}-{timestamp}", std::process::id()))
}

struct TargetCleanup(PathBuf);

impl Drop for TargetCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::local_markdown_links;
    use super::resolve_link;

    #[test]
    fn test_link_extraction_skips_external_anchors_and_fenced_examples() {
        let markdown = "[local](../guide.md#part) [external](https://example.test/a.md) [anchor](#part)\n```md\n[example](missing.md)\n```\n";
        assert_eq!(vec!["../guide.md".to_owned()], local_markdown_links(markdown));
        assert_eq!("guide.md", resolve_link("doc/page.md", "../guide.md"));
    }

    #[test]
    fn test_link_resolution_normalizes_parent_and_current_components() {
        assert_eq!(
            "doc/other.md",
            resolve_link("doc/section/page.md", ".././other.md#heading")
        );
    }
}
