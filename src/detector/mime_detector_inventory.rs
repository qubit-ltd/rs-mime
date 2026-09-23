// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Link-time MIME detector provider inventory.

qubit_spi::declare_sync_provider_inventory! {
    pub mod providers {
        spec = crate::detector::MimeDetectorSpec;
    }
}

pub use providers::Entry;
pub use providers::build_registry;
