// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Accounts provider-prefix bytes and settles the associated read reservation.

use qubit_fs::read::ReadIoObserver;

use crate::detector::internal::mime_budget::ReadPermit;

/// Settles a bounded provider-prefix reservation with bytes delivered so far.
///
/// The guard commits its observed byte count when dropped, including when the
/// enclosing detection future is cancelled while a provider read is pending.
pub(super) struct PrefixReadAccounting {
    /// Provisional read reservation retained until settlement.
    permit: Option<ReadPermit>,
    /// Bytes reported by successful provider reads.
    bytes_read: u64,
}

impl PrefixReadAccounting {
    /// Creates an accounting guard for one bounded prefix request.
    pub(super) fn new(permit: ReadPermit) -> Self {
        Self {
            permit: Some(permit),
            bytes_read: 0,
        }
    }

    /// Returns bytes reported by successful provider reads so far.
    pub(super) fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    /// Commits the observed amount and disables cancellation settlement.
    pub(super) fn commit(mut self) -> u64 {
        self.permit
            .take()
            .expect("prefix read permit commits once")
            .commit(self.bytes_read)
    }
}

impl ReadIoObserver for PrefixReadAccounting {
    fn on_read_bytes(&mut self, bytes: usize) {
        let bytes = u64::try_from(bytes).expect("provider read byte count fits in u64");
        self.bytes_read = self
            .bytes_read
            .checked_add(bytes)
            .expect("bounded prefix byte count does not overflow");
    }
}

impl Drop for PrefixReadAccounting {
    fn drop(&mut self) {
        if let Some(permit) = self.permit.take() {
            permit.commit(self.bytes_read);
        }
    }
}
