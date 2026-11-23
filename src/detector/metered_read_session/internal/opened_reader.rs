// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider reader variants supported by metered sessions.

use qubit_fs::read::AsyncFileReader;
use qubit_fs::read::AsyncRandomAccessReader;

/// Holds the opened provider reader and its available access pattern.
pub(super) enum OpenedReader {
    /// Reads sequentially from the current object position.
    Sequential(AsyncFileReader),
    /// Reads at explicit offsets from the same opened object.
    RandomAccess(AsyncRandomAccessReader),
}
