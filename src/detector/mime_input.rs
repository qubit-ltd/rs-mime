// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Owned and borrowed inputs for managed MIME detection.

mod borrowed_mime_input;
mod input_extent;
mod inspected_extent;
mod owned_mime_input;

pub use borrowed_mime_input::BorrowedMimeInput;
pub use input_extent::InputExtent;
pub use inspected_extent::InspectedExtent;
pub use owned_mime_input::OwnedMimeInput;
