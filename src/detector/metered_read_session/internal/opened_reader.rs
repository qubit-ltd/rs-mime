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
pub(in crate::detector::metered_read_session) enum OpenedReader {
    /// Reads sequentially from the current object position.
    Sequential(#[doc = "Sequential reader for the opened provider object."] AsyncFileReader),
    /// Reads at explicit offsets from the same opened object.
    RandomAccess(#[doc = "Random-access reader for the opened provider object."] AsyncRandomAccessReader),
}
