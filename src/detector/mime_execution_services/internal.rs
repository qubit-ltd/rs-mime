// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private state and guards for MIME request admission.

mod admission_permit;
mod admission_state;
mod admission_tracker;
mod admissions_settled;
mod waiter_registration;

pub(super) use admission_permit::AdmissionPermit;
pub(super) use admission_state::AdmissionState;
pub(super) use admission_tracker::AdmissionTracker;
pub(super) use admissions_settled::AdmissionsSettled;
pub(super) use waiter_registration::WaiterRegistration;
