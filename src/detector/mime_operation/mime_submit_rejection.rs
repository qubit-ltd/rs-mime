// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Reasons that prevent a MIME operation from starting.

use qubit_operation::AdmissionError;
use qubit_operation::GroupBindingError;

/// Local and manager admission reasons for a not-yet-started submission.
///
/// # Variants
/// - `Admission`: The operation manager rejected admission.
/// - `ExecutionServicesRequired`: Managed execution services are unavailable.
/// - `AsyncUnsupported`: The selected provider has no async backend.
/// - `InvalidOptions`: Request options could not be applied.
/// - `BudgetExceeded`: A configured request budget would be exceeded.
/// - `GroupBinding`: The child operation could not bind to the MIME group.
#[derive(Debug)]
pub enum MimeSubmitRejection {
    /// Admission rejected by operation's manager.
    Admission(#[doc = "The operation manager's rejection details."] AdmissionError),
    /// No execution services were configured.
    ExecutionServicesRequired,
    /// Selected provider has no explicit asynchronous backend capability.
    AsyncUnsupported,
    /// Request options were invalid.
    InvalidOptions(#[doc = "Description of the invalid request options."] String),
    /// A finite request budget would be exceeded.
    BudgetExceeded,
    /// The selected operation group could not be bound.
    GroupBinding(#[doc = "The operation manager's group-binding error."] GroupBindingError),
}
