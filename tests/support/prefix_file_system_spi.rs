// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Minimal filesystem provider used by path-prefix detector integration tests.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use qubit_fs::FsError;
use qubit_fs::FsResult;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::metadata::FileSystemCapabilities;
use qubit_fs::metadata::FileSystemCapability;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::metadata::FileSystemInfo;
use qubit_fs::metadata::FileSystemLimits;
use qubit_fs::metadata::OpenedFileInfo;
use qubit_fs::metadata::SymlinkPolicy;
use qubit_fs::path::PathConstraints;
use qubit_fs::path::PathSemantics;
use qubit_fs::spi::FileSystemSpi;
use qubit_fs::spi::OpenReaderRequest;
use qubit_fs::spi::OpenedReader;
use qubit_fs::spi::ProviderOperation;
use qubit_fs::spi::ProviderOperations;
use qubit_fs::spi::ProviderProperties;
use qubit_fs::spi::StatRequest;
use qubit_fs::spi::StatResponse;
use qubit_io::Input;

/// Provider fixture which serves one in-memory object and records stat calls.
#[derive(Clone)]
pub(crate) struct PrefixFileSystemSpi {
    content: Arc<Vec<u8>>,
    opened: Arc<AtomicUsize>,
    requested_read_bytes: Arc<AtomicUsize>,
    stats: Arc<AtomicUsize>,
    semantics: PathSemantics,
    guaranteed_range: bool,
    range_length: Arc<Mutex<Option<u64>>>,
}

impl PrefixFileSystemSpi {
    /// Creates a hierarchical provider serving `content`.
    pub(crate) fn hierarchical(content: Vec<u8>) -> Self {
        Self::new(content, PathSemantics::Hierarchical)
    }

    /// Creates an object-key provider serving `content`.
    pub(crate) fn object_key(content: Vec<u8>) -> Self {
        Self::new(content, PathSemantics::ObjectKey)
    }

    fn new(content: Vec<u8>, semantics: PathSemantics) -> Self {
        Self {
            content: Arc::new(content),
            opened: Arc::new(AtomicUsize::new(0)),
            requested_read_bytes: Arc::new(AtomicUsize::new(0)),
            stats: Arc::new(AtomicUsize::new(0)),
            semantics,
            guaranteed_range: false,
            range_length: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the number of metadata requests received by this provider.
    pub(crate) fn stat_calls(&self) -> usize {
        self.stats.load(Ordering::Relaxed)
    }

    /// Returns the number of reader sessions opened by this provider.
    pub(crate) fn opened(&self) -> usize {
        self.opened.load(Ordering::Relaxed)
    }

    /// Returns the total number of bytes requested from opened readers.
    pub(crate) fn requested_read_bytes(&self) -> usize {
        self.requested_read_bytes.load(Ordering::Relaxed)
    }

    /// Enables the guaranteed range contract for request-planning tests.
    pub(crate) fn with_guaranteed_range(mut self) -> Self {
        self.guaranteed_range = true;
        self
    }

    /// Returns the actual length received at the provider boundary.
    pub(crate) fn range_length(&self) -> Option<u64> {
        *self.range_length.lock().unwrap()
    }

    fn properties_snapshot(&self) -> ProviderProperties {
        let mut capabilities = FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read);
        if self.guaranteed_range {
            capabilities = capabilities.with_guaranteed(FileSystemCapability::RangeRead);
        }
        ProviderProperties::new(
            FileSystemInfo::new(
                FileSystemId::new("prefix-test").expect("fixture id should be valid"),
                "prefix-test",
                self.semantics,
            ),
            ProviderOperations::new().with(ProviderOperation::OpenReader),
            capabilities,
            FileSystemLimits::unknown(),
            PathConstraints::either(),
            SymlinkPolicy::Reject,
        )
        .expect("fixture properties should be valid")
    }
}

impl FileSystemSpi for PrefixFileSystemSpi {
    fn properties(&self) -> ProviderProperties {
        self.properties_snapshot()
    }

    fn stat(&self, request: StatRequest<'_>) -> FsResult<StatResponse> {
        self.stats.fetch_add(1, Ordering::Relaxed);
        Err(
            FsError::new(FsErrorKind::Io, FsOperation::Stat, "stat is not supported by fixture")
                .with_path(request.path().clone()),
        )
    }

    fn open_reader(&self, request: OpenReaderRequest<'_>) -> FsResult<OpenedReader> {
        self.opened.fetch_add(1, Ordering::Relaxed);
        let length = request.options().options().length();
        *self.range_length.lock().unwrap() = length;
        let maximum = usize::try_from(length.unwrap_or(u64::MAX))
            .unwrap_or(usize::MAX)
            .min(self.content.len());
        let reader: Box<dyn Input<Item = u8> + Send> = Box::new(PrefixReader {
            inner: Cursor::new(self.content[..maximum].to_vec()),
            requested_read_bytes: Arc::clone(&self.requested_read_bytes),
        });
        Ok(OpenedReader::new(
            OpenedFileInfo::new(
                FileSystemId::new("prefix-test").expect("fixture id should be valid"),
                request.path().clone(),
            ),
            reader,
        ))
    }
}

struct PrefixReader {
    inner: Cursor<Vec<u8>>,
    requested_read_bytes: Arc<AtomicUsize>,
}

impl Input for PrefixReader {
    type Item = u8;

    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> std::io::Result<usize> {
        self.requested_read_bytes.fetch_add(count, Ordering::Relaxed);
        std::io::Read::read(&mut self.inner, &mut output[index..index + count])
    }
}
