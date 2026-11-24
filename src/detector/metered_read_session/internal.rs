// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private implementation details for metered provider reads.

// Defines the provider reader representation retained by a session.
mod opened_reader;

pub(super) use self::opened_reader::OpenedReader;
