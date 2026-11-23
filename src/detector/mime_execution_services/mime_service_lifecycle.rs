// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Lifecycle phases shared by clones of a MIME execution service.

/// Lifecycle phase shared by every clone of a MIME execution service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MimeServiceLifecycle {
    /// The service has not started its execution port.
    NotStarted = 0,
    /// Preparation succeeded and the service accepts root and child work.
    Running = 1,
    /// Root work is closed; valid child work remains admissible until frozen.
    Draining = 2,
    /// Shutdown began before preparation, so no requests were ever accepted.
    ClosedWithoutStart = 4,
    /// The execution port has stopped and the group drain was requested.
    Stopped = 3,
}

/// Converts the stored lifecycle discriminant back into its public phase.
impl MimeServiceLifecycle {
    /// Decodes one atomic lifecycle value, treating unknown values as not started.
    ///
    /// # Parameters
    /// - `value`: Stored lifecycle discriminant.
    ///
    /// # Returns
    /// The matching lifecycle phase, or [`Self::NotStarted`] for an unknown value.
    pub(super) fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::Running,
            2 => Self::Draining,
            3 => Self::Stopped,
            4 => Self::ClosedWithoutStart,
            _ => Self::NotStarted,
        }
    }
}
