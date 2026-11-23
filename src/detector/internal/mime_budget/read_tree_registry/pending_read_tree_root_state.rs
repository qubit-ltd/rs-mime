// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::ResourceManageErrorKind;
use qubit_operation::ResourceReservation;

/// Records whether the first parent anchor is pending, detached, or rejected.
pub(super) enum PendingReadTreeRootState {
    /// Holds the parent slot until an accepted child commits the root.
    Reserved(
        /// Parent resource reservation awaiting commit or rollback.
        Option<ResourceReservation>,
    ),
    /// The parent ended after admission, so accepted siblings share the root
    /// without retaining an anchor to the ended scope.
    Detached,
    /// A non-terminal commit failure shared by the accepted sibling tickets.
    Failed(
        /// Resource-scope failure observed by the first accepted binding.
        ResourceManageErrorKind,
    ),
}
