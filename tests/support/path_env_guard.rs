// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::sync::{
    Mutex,
    MutexGuard,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Guard that prepends one directory to `PATH` for the current process.
pub(crate) struct PathEnvGuard {
    _guard: MutexGuard<'static, ()>,
    original_path: Option<String>,
}

impl PathEnvGuard {
    /// Prepends `directory` to `PATH` and holds a process-wide environment
    /// lock.
    pub(crate) fn prepend(directory: &Path) -> Self {
        let guard = ENV_LOCK
            .lock()
            .expect("environment lock should not be poisoned");
        let original_path = std::env::var("PATH").ok();
        let separator = if cfg!(windows) { ";" } else { ":" };
        let mut path = directory.display().to_string();
        if let Some(original_path) = &original_path {
            path.push_str(separator);
            path.push_str(original_path);
        }
        unsafe {
            std::env::set_var("PATH", path);
        }
        Self {
            _guard: guard,
            original_path,
        }
    }
}

impl Drop for PathEnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(original_path) = &self.original_path {
                std::env::set_var("PATH", original_path);
            } else {
                std::env::remove_var("PATH");
            }
        }
    }
}
