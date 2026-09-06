// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Minimal filesystem provider used by path-prefix detector integration tests.

use std::io::Cursor;
use std::sync::Arc;
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

    fn properties_snapshot(&self) -> ProviderProperties {
        ProviderProperties::new(
            FileSystemInfo::new(
                FileSystemId::new("prefix-test").expect("fixture id should be valid"),
                "prefix-test",
                self.semantics,
            ),
            ProviderOperations::new().with(ProviderOperation::OpenReader),
            FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
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
        let reader: Box<dyn Input<Item = u8> + Send> = Box::new(PrefixReader {
            inner: Cursor::new((*self.content).clone()),
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

    unsafe fn read_unchecked(
        &mut self,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> std::io::Result<usize> {
        self.requested_read_bytes.fetch_add(count, Ordering::Relaxed);
        std::io::Read::read(&mut self.inner, &mut output[index..index + count])
    }
}
