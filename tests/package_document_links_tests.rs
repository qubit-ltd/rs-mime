// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const DOCUMENTS: [&str; 5] = [
    "README.md",
    "README.zh_CN.md",
    "doc/acceptance-0.22.0-operation-0.2.zh_CN.md",
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
    assert!(
        !packaged.is_empty(),
        "cargo package returned an empty file list"
    );

    verify_reachable_markdown(&DOCUMENTS.map(str::to_owned), &packaged, |document| {
        fs::read_to_string(package_root.join(document))
            .map_err(|error| format!("read {document}: {error}"))
    })
    .unwrap_or_else(|error| panic!("packaged Markdown links are invalid: {error}"));

    assert!(
        !packaged
            .iter()
            .any(|path| path.contains("implementation-plan-") || path.contains("design-20261117")),
        "the package must not include this round's design or implementation plan"
    );
}

fn local_file_links_with_fragments(markdown: &str) -> Vec<(String, Option<String>)> {
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
        if target.starts_with("//") || target.contains("://") || target.starts_with("mailto:") {
            cursor = end + 1;
            continue;
        }
        let (path, fragment) = target.split_once('#').unwrap_or((target, ""));
        let path = path.split('?').next().unwrap_or_default();
        if !path.is_empty() || !fragment.is_empty() {
            links.push((
                path.to_owned(),
                (!fragment.is_empty()).then(|| fragment.to_owned()),
            ));
        }
        cursor = end + 1;
    }
    links
}

fn verify_reachable_markdown<F>(
    roots: &[String],
    packaged: &HashSet<String>,
    mut read_document: F,
) -> Result<(), String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let mut pending = VecDeque::from(roots.to_vec());
    let mut visited = HashSet::new();
    let mut documents: HashMap<String, String> = HashMap::new();
    while let Some(document) = pending.pop_front() {
        if !visited.insert(document.clone()) {
            continue;
        }
        if !packaged.contains(&document) {
            return Err(format!(
                "reachable Markdown document {document:?} is absent from cargo package --list"
            ));
        }
        let markdown = read_document(&document)?;
        if let Some(target) = machine_local_file_link(&markdown) {
            return Err(format!(
                "{document} contains machine-local file URL {target:?}; use a package-relative path"
            ));
        }
        for (target, fragment) in local_file_links_with_fragments(&markdown) {
            let resolved = if target.is_empty() {
                document.clone()
            } else {
                resolve_link(&document, &target)
            };
            if !packaged.contains(&resolved) {
                return Err(format!(
                    "{document} links to {target:?}, which resolves to {resolved:?} but is absent from cargo package --list"
                ));
            }
            let is_markdown = resolved.to_ascii_lowercase().ends_with(".md");
            if is_markdown {
                if let Some(fragment) = fragment {
                    let target_markdown = if resolved == document {
                        markdown.as_str()
                    } else if let Some(cached) = documents.get(&resolved) {
                        cached.as_str()
                    } else {
                        let content = read_document(&resolved)?;
                        documents.insert(resolved.clone(), content);
                        documents
                            .get(&resolved)
                            .expect("document was inserted")
                            .as_str()
                    };
                    if !markdown_anchors(target_markdown).contains(&fragment) {
                        return Err(format!(
                            "{document} links to missing anchor #{fragment} in {resolved}"
                        ));
                    }
                }
                pending.push_back(resolved);
            }
        }
        documents.insert(document, markdown);
    }
    Ok(())
}

fn machine_local_file_link(markdown: &str) -> Option<String> {
    let mut fenced = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut cursor = 0;
        while let Some(relative) = line[cursor..].find("](") {
            let start = cursor + relative + 2;
            let Some(end_relative) = line[start..].find(')') else {
                break;
            };
            let end = start + end_relative;
            let mut target = line[start..end].trim();
            if target.starts_with('<') && target.ends_with('>') && target.len() >= 2 {
                target = &target[1..target.len() - 1];
            } else if let Some(space) = target.find(char::is_whitespace) {
                target = &target[..space];
            }
            if target.to_ascii_lowercase().starts_with("file://") {
                return Some(target.to_owned());
            }
            cursor = end + 1;
        }
    }
    None
}

fn markdown_anchors(markdown: &str) -> HashSet<String> {
    markdown
        .lines()
        .filter_map(|line| {
            let heading = line.trim_start().trim_start_matches('#');
            if heading.len() == line.trim_start().len() || !line.trim_start().starts_with('#') {
                return None;
            }
            let heading = heading.trim().trim_matches('#').trim();
            let mut slug = String::new();
            let mut previous_dash = false;
            for character in heading.chars().flat_map(char::to_lowercase) {
                if character.is_alphanumeric() {
                    slug.push(character);
                    previous_dash = false;
                } else if (character.is_whitespace() || character == '-')
                    && !slug.is_empty()
                    && !previous_dash
                {
                    slug.push('-');
                    previous_dash = true;
                }
            }
            while slug.ends_with('-') {
                slug.pop();
            }
            Some(slug)
        })
        .collect()
}

fn resolve_link(document: &str, target: &str) -> String {
    let parent = Path::new(document)
        .parent()
        .unwrap_or_else(|| Path::new(""));
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
    std::env::temp_dir().join(format!(
        "qubit-mime-package-links-{}-{timestamp}",
        std::process::id()
    ))
}

struct TargetCleanup(PathBuf);

impl Drop for TargetCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::HashSet;

    use super::local_file_links_with_fragments;
    use super::machine_local_file_link;
    use super::resolve_link;
    use super::verify_reachable_markdown;

    #[test]
    fn test_link_extraction_skips_external_targets_and_fenced_examples() {
        let markdown = "[local](../guide.md#part) [external](https://example.test/a.md) [anchor](#part)\n```md\n[example](missing.md)\n```\n";
        assert_eq!(
            vec![
                ("../guide.md".to_owned(), Some("part".to_owned())),
                (String::new(), Some("part".to_owned())),
            ],
            local_file_links_with_fragments(markdown)
        );
        assert_eq!("guide.md", resolve_link("doc/page.md", "../guide.md"));
    }

    #[test]
    fn test_link_extraction_includes_non_markdown_package_files() {
        let markdown = "[workflow](../.github/workflows/ci.yml) [manifest](fixture/Cargo.toml) [lock](fixture/Cargo.lock) [asset](image.png)";
        assert_eq!(
            vec![
                ("../.github/workflows/ci.yml".to_owned(), None),
                ("fixture/Cargo.toml".to_owned(), None),
                ("fixture/Cargo.lock".to_owned(), None),
                ("image.png".to_owned(), None),
            ],
            local_file_links_with_fragments(markdown)
        );
    }

    #[test]
    fn test_package_file_targets_reject_missing_workflow_manifest_and_lock() {
        let packaged = HashSet::from(["README.md".to_owned()]);
        for missing in [
            ".github/workflows/ci.yml",
            "fixture/Cargo.toml",
            "fixture/Cargo.lock",
        ] {
            let document = format!("[target]({missing})");
            let documents = HashMap::from([("README.md", document.as_str())]);
            let error = verify_reachable_markdown(&["README.md".to_owned()], &packaged, |path| {
                documents
                    .get(path)
                    .map(|text| (*text).to_owned())
                    .ok_or_else(|| format!("missing {path}"))
            })
            .unwrap_err();
            assert!(
                error.contains(missing),
                "expected missing target {missing:?}: {error}"
            );
        }
    }

    #[test]
    fn test_package_file_targets_accept_existing_workflow_manifest_and_lock() {
        let documents = HashMap::from([(
            "README.md",
            "[workflow](.github/workflows/ci.yml) [manifest](fixture/Cargo.toml) [lock](fixture/Cargo.lock)",
        )]);
        let packaged = HashSet::from([
            "README.md".to_owned(),
            ".github/workflows/ci.yml".to_owned(),
            "fixture/Cargo.toml".to_owned(),
            "fixture/Cargo.lock".to_owned(),
        ]);
        verify_reachable_markdown(&["README.md".to_owned()], &packaged, |path| {
            documents
                .get(path)
                .map(|text| (*text).to_owned())
                .ok_or_else(|| format!("missing {path}"))
        })
        .unwrap();
    }

    #[test]
    fn test_machine_local_file_urls_are_rejected_but_external_urls_are_allowed() {
        assert_eq!(
            None,
            machine_local_file_link(
                "[external](https://example.test/a)\n```md\n[example](file:///tmp/example.md)\n```\n"
            )
        );
        assert_eq!(
            Some("file:///tmp/report.md".to_owned()),
            machine_local_file_link("[report](<file:///tmp/report.md>)")
        );
        assert_eq!(
            Some("FILE:///tmp/report.md".to_owned()),
            machine_local_file_link("[report](FILE:///tmp/report.md)")
        );
    }

    #[test]
    fn test_reachable_document_rejects_machine_local_file_urls() {
        let packaged = HashSet::from(["README.md".to_owned()]);
        let error = verify_reachable_markdown(&["README.md".to_owned()], &packaged, |_| {
            Ok("[temporary report](file:///tmp/report.md)".to_owned())
        })
        .unwrap_err();

        assert!(error.contains("machine-local file URL"), "{error}");
    }

    #[test]
    fn test_link_resolution_normalizes_parent_and_current_components() {
        assert_eq!(
            "doc/other.md",
            resolve_link("doc/section/page.md", ".././other.md#heading")
        );
    }

    #[test]
    fn test_two_hop_missing_markdown_link_is_rejected() {
        let documents = HashMap::from([
            ("README.md", "[guide](doc/guide.md)"),
            ("doc/guide.md", "[missing](details.md)"),
        ]);
        let packaged = HashSet::from(["README.md".to_owned(), "doc/guide.md".to_owned()]);

        let error = verify_reachable_markdown(&["README.md".to_owned()], &packaged, |path| {
            documents
                .get(path)
                .map(|markdown| (*markdown).to_owned())
                .ok_or_else(|| format!("missing document {path}"))
        })
        .unwrap_err();

        assert!(error.contains("doc/details.md"), "{error}");
    }
}
