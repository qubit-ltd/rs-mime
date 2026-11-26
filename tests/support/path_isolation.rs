// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

const CHILD_MARKER: &str = "QUBIT_MIME_PATH_ISOLATED_TEST_CHILD";
const CHILD_PATH_DIRECTORY: &str = "QUBIT_MIME_PATH_ISOLATED_TEST_DIRECTORY";
const CHILD_PATH_REPLACE: &str = "QUBIT_MIME_PATH_ISOLATED_TEST_REPLACE";

/// Runs the current exact test with a private PATH. Returns true in the parent,
/// which should return from the test after the child has completed.
pub(crate) fn run_test_in_path_child(test_name: &str, directory: &Path, replace: bool) -> bool {
    if std::env::var_os(CHILD_MARKER).is_some() {
        if std::env::var_os(CHILD_PATH_REPLACE).as_deref() != Some(std::ffi::OsStr::new("1")) {
            let path_directory = std::env::var_os(CHILD_PATH_DIRECTORY)
                .map(std::path::PathBuf::from)
                .expect("isolated child must know its private PATH directory");
            let command = if test_name.contains("ffprobe") {
                "ffprobe"
            } else {
                "file"
            };
            let source = directory.join(command);
            let target = path_directory.join(command);
            let _ = std::fs::remove_file(&target);
            std::os::unix::fs::symlink(source, target)
                .expect("private PATH should link the child executable");
        }
        return false;
    }

    let original_path = std::env::var_os("PATH");
    let mut entries = if replace {
        Vec::new()
    } else {
        std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    entries.insert(0, directory.to_path_buf());
    let path: OsString = std::env::join_paths(entries).expect("native PATH entries should join");
    let executable =
        std::env::current_exe().expect("integration test executable should be available");
    let status = Command::new(executable)
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_MARKER, "1")
        .env(CHILD_PATH_DIRECTORY, directory)
        .env(CHILD_PATH_REPLACE, if replace { "1" } else { "" })
        .env("PATH", path)
        .status()
        .expect("isolated PATH test child should start");
    assert!(status.success(), "isolated test child failed: {test_name}");
    assert_eq!(
        original_path,
        std::env::var_os("PATH"),
        "parent PATH must remain unchanged"
    );
    true
}
