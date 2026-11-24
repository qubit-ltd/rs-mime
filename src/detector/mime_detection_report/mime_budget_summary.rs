// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Summary of bytes retained, read, and temporarily staged for one request.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeBudgetSummary;
///
/// let summary = MimeBudgetSummary {
///     owned_bytes: 1024,
///     bytes_read: 512,
///     staging_bytes: 2048,
/// };
/// assert_eq!(summary.owned_bytes, 1024);
/// assert_eq!(summary.staging_bytes, 2048);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MimeBudgetSummary {
    /// Input-payload bytes held by this request at the report boundary.
    ///
    /// Staging read buffers and copied chunks are temporary work memory and
    /// are not included here; they are accounted against the service-wide
    /// owned-memory budget while in use.
    pub owned_bytes: usize,
    /// Bytes successfully delivered by MIME-metered provider reads initiated
    /// by this request, including repeated or overlapping reads. Descendant
    /// request reads are reported on their own reports and do not contribute
    /// here. Provider I/O performed by an extension without the MIME metered
    /// read session is outside this count.
    pub bytes_read: u64,
    /// Peak staging bytes simultaneously charged to this request.
    ///
    /// This is a high-water mark, not cumulative bytes written or the bytes
    /// still retained when the report is assembled. Concurrent requests are
    /// accounted independently. It also does not indicate that resource
    /// cleanup or operation settlement has completed.
    pub staging_bytes: u64,
}
