// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Link-time media stream classifier provider inventory.

qubit_spi::declare_sync_provider_inventory! {
    pub mod providers {
        spec = crate::classifier::MediaStreamClassifierSpec;
    }
}

pub use providers::Entry;
pub use providers::build_registry;
