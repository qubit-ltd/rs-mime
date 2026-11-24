// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Budgeted provider reads for third-party MIME backends.

use std::sync::Arc;

use qubit_fs::AsyncFileSystem;
use qubit_fs::Path as FsPath;
use qubit_fs::read::ReadOptions;
use qubit_io::AsyncInput;
use qubit_operation::OperationContext;

// Keeps the provider reader representation private to this session module.
mod internal;

use self::internal::OpenedReader;
use super::ReadTreeContext;
use super::mime_execution_services::MimeExecutionServices;
use crate::MimeError;
use crate::MimeResult;

const MAX_READ_CHUNK: usize = 8 * 1024;
const MAX_OWNED_BYTES: usize = 16 * 1024 * 1024;

/// A provider object whose reads are charged to request and ancestor budgets.
///
/// The session owns one opened provider reader. Each call is clipped to the
/// available read-tree allowance and reserves service owned-memory capacity
/// independently before touching that reader. It reads into private scratch
/// storage, then copies only a successful read's returned bytes into the
/// backend buffer and commits that count. A provider error or a dropped read
/// future may leave partial data in scratch, but that data is discarded and is
/// never exposed to the backend. Successfully returned bytes remain charged;
/// unused reservation is returned, and failed open attempts charge no bytes.
#[must_use = "retain the session for all provider reads in one backend call"]
pub struct MeteredReadSession {
    /// The one opened provider object used for every read in this session.
    reader: OpenedReader,
    /// The request services that account for temporary owned scratch memory.
    services: Arc<MimeExecutionServices>,
    /// The shared ancestor tree that accounts for bytes delivered to the
    /// backend.
    read_tree: Arc<ReadTreeContext>,
    /// The remaining cumulative allowance for this provider request.
    request_remaining: u64,
    operation_context: OperationContext,
    /// The maximum number of bytes any individual read may deliver.
    payload_limit: usize,
    /// The next sequential offset, advanced only by successful sequential
    /// reads.
    next_offset: u64,
}

impl MeteredReadSession {
    /// Opens one sequential reader for a managed Provider input.
    ///
    /// # Parameters
    ///
    /// - `file_system` is the provider whose object will be opened.
    /// - `path` identifies the object to open.
    /// - `services` supplies request-level owned-memory accounting and read
    ///   limits.
    /// - `read_tree` supplies the shared ancestor read allowance.
    /// - `request_limit` caps bytes delivered by this session.
    /// - `payload_limit` caps each individual read.
    ///
    /// # Errors
    ///
    /// Returns the provider's open error when the object cannot be opened.
    pub(crate) async fn open_sequential(
        file_system: &AsyncFileSystem,
        path: &FsPath,
        services: Arc<MimeExecutionServices>,
        read_tree: Arc<ReadTreeContext>,
        operation_context: OperationContext,
        request_limit: u64,
        payload_limit: usize,
    ) -> MimeResult<Self> {
        ensure_active(&operation_context)?;
        let reader = file_system
            .open_reader(path, ReadOptions::default())
            .await
            .map_err(MimeError::FileSystem)?;
        Ok(Self::new(
            OpenedReader::Sequential(reader),
            services,
            read_tree,
            operation_context,
            request_limit,
            payload_limit,
        ))
    }

    /// Opens one random-access reader for reads at offsets in the same object.
    ///
    /// # Parameters
    ///
    /// - `file_system` is the provider whose object will be opened.
    /// - `path` identifies the object to open.
    /// - `services` supplies request-level owned-memory accounting and read
    ///   limits.
    /// - `read_tree` supplies the shared ancestor read allowance.
    /// - `request_limit` caps bytes delivered by this session.
    /// - `payload_limit` caps each individual read.
    ///
    /// # Errors
    ///
    /// Returns the provider's open error when the object cannot be opened.
    pub(crate) async fn open_random_access(
        file_system: &AsyncFileSystem,
        path: &FsPath,
        services: Arc<MimeExecutionServices>,
        read_tree: Arc<ReadTreeContext>,
        operation_context: OperationContext,
        request_limit: u64,
        payload_limit: usize,
    ) -> MimeResult<Self> {
        ensure_active(&operation_context)?;
        let reader = file_system
            .open_random_access_reader(path, ReadOptions::default())
            .await
            .map_err(MimeError::FileSystem)?;
        Ok(Self::new(
            OpenedReader::RandomAccess(reader),
            services,
            read_tree,
            operation_context,
            request_limit,
            payload_limit,
        ))
    }

    /// Creates a session around an already opened reader and its accounting
    /// limits.
    ///
    /// # Parameters
    ///
    /// - `reader` is the provider reader retained for this session.
    /// - `services` accounts for temporary scratch memory.
    /// - `read_tree` is the shared ancestor read budget.
    /// - `request_limit` is the maximum cumulative transfer for this session.
    /// - `payload_limit` is the maximum transfer for one read.
    fn new(
        reader: OpenedReader,
        services: Arc<MimeExecutionServices>,
        read_tree: Arc<ReadTreeContext>,
        operation_context: OperationContext,
        request_limit: u64,
        payload_limit: usize,
    ) -> Self {
        let request_remaining = request_limit.min(services.max_read_bytes);
        Self {
            reader,
            services,
            read_tree,
            operation_context,
            request_remaining,
            payload_limit,
            next_offset: 0,
        }
    }

    /// Reads the next bytes from the opened object.
    ///
    /// Empty output returns zero without observing EOF. A nonempty read is
    /// clipped to the fixed work-buffer size, remaining request allowance,
    /// and all ancestor and service read-tree allowances.
    ///
    /// # Parameters
    ///
    /// - `output` receives only bytes from a successful provider read.
    ///
    /// # Returns
    ///
    /// The number of bytes copied into `output` and charged to the budgets.
    ///
    /// # Errors
    ///
    /// Returns an I/O, allocation, or budget error without exposing partial
    /// provider bytes in `output`.
    pub async fn read(&mut self, output: &mut [u8]) -> MimeResult<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.read_at_inner(self.next_offset, output, true).await
    }

    /// Reads bytes at an offset from the same opened object.
    ///
    /// This method is available only for a session opened for random-access
    /// reads. Repeated or overlapping reads are charged by the number of
    /// bytes each read returns successfully.
    ///
    /// # Parameters
    ///
    /// - `offset` is the byte position in the opened object.
    /// - `output` receives only bytes from a successful provider read.
    ///
    /// # Returns
    ///
    /// The number of bytes copied into `output` and charged to the budgets.
    ///
    /// # Errors
    ///
    /// Returns an error if random access is unavailable, the read fails, or a
    /// request or ancestor budget cannot reserve the next read.
    pub async fn read_at(&mut self, offset: u64, output: &mut [u8]) -> MimeResult<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if !matches!(&self.reader, OpenedReader::RandomAccess(_)) {
            return Err(MimeError::InvalidClassifierInput {
                reason: "random access is unavailable for this metered read session".into(),
            });
        }
        self.read_at_inner(offset, output, false).await
    }

    /// Reads one bounded chunk and commits only bytes delivered successfully.
    ///
    /// # Parameters
    ///
    /// - `offset` is the provider object position for this read.
    /// - `output` receives bytes only after the provider returns successfully.
    /// - `advance` updates the sequential offset after a successful transfer.
    ///
    /// # Returns
    ///
    /// The number of bytes copied into `output` and charged to the budgets.
    ///
    /// # Errors
    ///
    /// Returns an allocation, provider, or budget error without transferring
    /// partial scratch contents to `output`.
    async fn read_at_inner(&mut self, offset: u64, output: &mut [u8], advance: bool) -> MimeResult<usize> {
        ensure_active(&self.operation_context)?;
        let available = self.request_remaining.min(self.read_tree.remaining());
        let ask = output
            .len()
            .min(MAX_READ_CHUNK)
            .min(self.payload_limit)
            .min(usize::try_from(available).unwrap_or(usize::MAX));
        if ask == 0 {
            return Err(MimeError::BudgetExceeded {
                kind: "read",
                requested: 1,
                limit: available,
            });
        }

        let read_permit = self.read_tree.reserve(ask as u64).ok_or(MimeError::BudgetExceeded {
            kind: "read",
            requested: ask as u64,
            limit: self.read_tree.remaining(),
        })?;
        let _scratch_permit = self.services.reserve_owned(ask).ok_or(MimeError::BudgetExceeded {
            kind: "owned",
            requested: ask as u64,
            limit: MAX_OWNED_BYTES as u64,
        })?;
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(ask)
            .map_err(|source| MimeError::BufferAllocationFailed { requested: ask, source })?;
        scratch.resize(ask, 0);

        let count = match &mut self.reader {
            OpenedReader::Sequential(reader) if advance => {
                reader.read_async(&mut scratch).await.map_err(MimeError::Io)?
            }
            OpenedReader::Sequential(_) => {
                return Err(MimeError::InvalidClassifierInput {
                    reason: "random access is unavailable for this metered read session".into(),
                });
            }
            OpenedReader::RandomAccess(reader) => reader
                .read_at(offset, &mut scratch)
                .await
                .map_err(MimeError::FileSystem)?,
        };
        let count_u64 = u64::try_from(count).expect("a provider count fits the bounded scratch buffer");
        output[..count].copy_from_slice(&scratch[..count]);
        read_permit.commit(count_u64);
        self.request_remaining -= count_u64;
        if advance {
            self.next_offset = self.next_offset.saturating_add(count_u64);
        }
        Ok(count)
    }
}

fn ensure_active(context: &OperationContext) -> MimeResult<()> {
    if let Some(reason) = context.stop_reason() {
        return Err(MimeError::DetectorBackend {
            backend: "mime".into(),
            reason: format!("operation stopped: {reason:?}"),
        });
    }
    if context.is_expired() {
        return Err(MimeError::DetectorBackend {
            backend: "mime".into(),
            reason: "operation deadline expired".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_fs::AsyncFileSystem;
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
    use qubit_fs::spi::AsyncFileSystemSpi;
    use qubit_fs::spi::OpenReaderRequest;
    use qubit_fs::spi::OpenedAsyncReader;
    use qubit_fs::spi::ProviderOperation;
    use qubit_fs::spi::ProviderOperations;
    use qubit_fs::spi::ProviderProperties;
    use qubit_fs::spi::StatRequest;
    use qubit_fs::spi::StatResponse;
    use qubit_io::AsyncInput;
    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationContext;
    use qubit_operation::StopReason;

    use super::MeteredReadSession;
    use crate::FixedMimeExecutionPort;
    use crate::MimeError;
    use crate::MimeExecutionServices;
    use crate::detector::ReadTreeContext;

    struct MemoryFileSystem {
        properties: ProviderProperties,
        content: Vec<u8>,
        error_first_read: bool,
        pending_first_read: bool,
    }

    impl MemoryFileSystem {
        fn new(content: &[u8], error_first_read: bool) -> Self {
            let properties = ProviderProperties::new(
                FileSystemInfo::new(
                    FileSystemId::new("metered-test").unwrap(),
                    "metered-test",
                    PathSemantics::Hierarchical,
                ),
                ProviderOperations::new().with(ProviderOperation::OpenReader),
                FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
                FileSystemLimits::unknown(),
                PathConstraints::absolute(),
                SymlinkPolicy::Reject,
            )
            .unwrap();
            Self {
                properties,
                content: content.to_vec(),
                error_first_read,
                pending_first_read: false,
            }
        }
    }

    impl AsyncFileSystemSpi for MemoryFileSystem {
        fn properties(&self) -> ProviderProperties {
            self.properties.clone()
        }

        fn stat<'a>(&'a self, request: StatRequest<'a>) -> qubit_fs::spi::SpiFuture<'a, FsResult<StatResponse>> {
            Box::pin(async move {
                Err(
                    FsError::new(FsErrorKind::UnsupportedOperation, FsOperation::Stat, "not supported")
                        .with_path(request.path().clone()),
                )
            })
        }

        fn open_reader<'a>(
            &'a self,
            request: OpenReaderRequest<'a>,
        ) -> qubit_fs::spi::SpiFuture<'a, FsResult<OpenedAsyncReader>> {
            let content = self.content.clone();
            let error_first_read = self.error_first_read;
            let pending_first_read = self.pending_first_read;
            Box::pin(async move {
                let info = OpenedFileInfo::new(FileSystemId::new("metered-test").unwrap(), request.path().clone());
                Ok(OpenedAsyncReader::new(
                    info,
                    Box::new(MemoryInput {
                        content,
                        offset: 0,
                        error_first_read,
                        pending_first_read,
                    }),
                ))
            })
        }
    }

    struct MemoryInput {
        content: Vec<u8>,
        offset: usize,
        error_first_read: bool,
        pending_first_read: bool,
    }

    impl AsyncInput for MemoryInput {
        type Item = u8;

        unsafe fn poll_read_unchecked(
            mut self: Pin<&mut Self>,
            context: &mut Context<'_>,
            output: &mut [u8],
            index: usize,
            count: usize,
        ) -> Poll<std::io::Result<usize>> {
            let this = self.as_mut().get_mut();
            if this.error_first_read {
                this.error_first_read = false;
                if count > 0 {
                    output[index] = b'!';
                }
                return Poll::Ready(Err(std::io::Error::other("injected partial failure")));
            }
            if this.pending_first_read {
                this.pending_first_read = false;
                if count > 0 {
                    output[index] = b'!';
                }
                context.waker().wake_by_ref();
                return Poll::Pending;
            }
            let actual = count.min(this.content.len().saturating_sub(this.offset));
            output[index..index + actual].copy_from_slice(&this.content[this.offset..this.offset + actual]);
            this.offset += actual;
            Poll::Ready(Ok(actual))
        }
    }

    fn services() -> Arc<MimeExecutionServices> {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        Arc::new(MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 1)), 16).unwrap())
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        struct ThreadWake(std::thread::Thread);
        impl Wake for ThreadWake {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match Pin::as_mut(&mut future).poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    #[test]
    fn test_metered_reads_charge_actual_successful_transfers_and_enforce_ancestor_limit() {
        let services = services();
        let root = ReadTreeContext::new(8);
        let parent = root.child(3);
        let file_system = AsyncFileSystem::from_spi(MemoryFileSystem::new(b"abcdef", false)).unwrap();
        let path = qubit_fs::Path::parse("/input").unwrap();
        let mut session = block_on(MeteredReadSession::open_sequential(
            &file_system,
            &path,
            Arc::clone(&services),
            Arc::clone(&parent),
            OperationContext::new(Default::default()).unwrap(),
            7,
            8,
        ))
        .unwrap();

        let mut output = [0xCC; 6];
        assert_eq!(3, block_on(session.read(&mut output)).unwrap());
        assert_eq!(&output[..3], b"abc");
        assert_eq!(&output[3..], &[0xCC; 3]);
        assert_eq!(3, root.consumed());
        assert_eq!(3, parent.consumed());
        assert!(matches!(
            block_on(session.read(&mut output)),
            Err(MimeError::BudgetExceeded { kind: "read", .. })
        ));
        assert_eq!(3, root.consumed());
        assert_eq!(3, parent.consumed());
        let scratch_permit = services
            .reserve_owned(16 * 1024 * 1024)
            .expect("scratch permit released");
        drop(scratch_permit);
    }

    #[test]
    fn test_sibling_sessions_share_ancestor_read_allowance() {
        let services = services();
        let root = ReadTreeContext::new(5);
        let left = root.child(5);
        let right = root.child(5);
        let file_system = AsyncFileSystem::from_spi(MemoryFileSystem::new(b"abcdef", false)).unwrap();
        let path = qubit_fs::Path::parse("/input").unwrap();
        let mut left_session = block_on(MeteredReadSession::open_sequential(
            &file_system,
            &path,
            Arc::clone(&services),
            Arc::clone(&left),
            OperationContext::new(Default::default()).unwrap(),
            5,
            8,
        ))
        .unwrap();
        let mut right_session = block_on(MeteredReadSession::open_sequential(
            &file_system,
            &path,
            Arc::clone(&services),
            Arc::clone(&right),
            OperationContext::new(Default::default()).unwrap(),
            5,
            8,
        ))
        .unwrap();

        let mut output = [0xCC; 4];
        assert_eq!(4, block_on(left_session.read(&mut output)).unwrap());
        assert_eq!(&output, b"abcd");
        assert_eq!(1, block_on(right_session.read(&mut output)).unwrap());
        assert_eq!(b"a", &output[..1]);
        assert_eq!(5, root.consumed());
        assert_eq!(4, left.consumed());
        assert_eq!(1, right.consumed());
        assert!(matches!(
            block_on(right_session.read(&mut output)),
            Err(MimeError::BudgetExceeded { kind: "read", .. })
        ));
        assert_eq!(5, root.consumed());
    }

    #[test]
    fn test_metered_error_discards_partial_scratch_and_releases_reservations() {
        let services = services();
        let root = ReadTreeContext::new(8);
        let file_system = AsyncFileSystem::from_spi(MemoryFileSystem::new(b"abc", true)).unwrap();
        let path = qubit_fs::Path::parse("/input").unwrap();
        let mut session = block_on(MeteredReadSession::open_sequential(
            &file_system,
            &path,
            Arc::clone(&services),
            Arc::clone(&root),
            OperationContext::new(Default::default()).unwrap(),
            8,
            4,
        ))
        .unwrap();

        let mut output = [0xCC; 4];
        assert!(block_on(session.read(&mut output)).is_err());
        assert_eq!([0xCC; 4], output, "partial bytes remain private on provider error");
        assert_eq!(0, root.consumed());
        assert_eq!(8, root.remaining());
        let scratch_permit = services
            .reserve_owned(16 * 1024 * 1024)
            .expect("scratch permit released");
        drop(scratch_permit);

        assert_eq!(3, block_on(session.read(&mut output)).unwrap());
        assert_eq!(&output[..3], b"abc");
        assert_eq!(3, root.consumed());
    }

    #[test]
    fn test_metered_session_caps_each_payload_and_cumulative_request_reads() {
        let services = services();
        let root = ReadTreeContext::new(8);
        let file_system = AsyncFileSystem::from_spi(MemoryFileSystem::new(b"abcdef", false)).unwrap();
        let path = qubit_fs::Path::parse("/input").unwrap();
        let mut session = block_on(MeteredReadSession::open_sequential(
            &file_system,
            &path,
            Arc::clone(&services),
            Arc::clone(&root),
            OperationContext::new(Default::default()).unwrap(),
            3,
            2,
        ))
        .unwrap();

        let mut output = [0xCC; 6];
        assert_eq!(2, block_on(session.read(&mut output)).unwrap());
        assert_eq!(&output[..2], b"ab");
        assert_eq!(1, block_on(session.read(&mut output)).unwrap());
        assert_eq!(b"c", &output[..1]);
        assert!(matches!(
            block_on(session.read(&mut output)),
            Err(MimeError::BudgetExceeded {
                kind: "read",
                limit: 0,
                ..
            })
        ));
        assert_eq!(3, root.consumed());
    }

    #[test]
    fn test_stopped_context_blocks_next_managed_sequential_read() {
        let services = services();
        let root = ReadTreeContext::new(8);
        let context = OperationContext::new(Default::default()).unwrap();
        let file_system = AsyncFileSystem::from_spi(MemoryFileSystem::new(b"abcdef", false)).unwrap();
        let path = qubit_fs::Path::parse("/input").unwrap();
        let mut session = block_on(MeteredReadSession::open_sequential(
            &file_system, &path, Arc::clone(&services), Arc::clone(&root), context.clone(), 8, 8,
        )).unwrap();
        let mut output = [0; 2];
        assert_eq!(2, block_on(session.read(&mut output)).unwrap());
        assert!(context.request_stop(StopReason::UserCancelled));
        assert!(block_on(session.read(&mut output)).is_err());
        assert_eq!(2, root.consumed());
    }

    #[test]
    fn test_dropping_pending_read_releases_read_and_scratch_reservations() {
        let services = services();
        let root = ReadTreeContext::new(8);
        let mut provider = MemoryFileSystem::new(b"abcdef", false);
        provider.pending_first_read = true;
        let file_system = AsyncFileSystem::from_spi(provider).unwrap();
        let path = qubit_fs::Path::parse("/input").unwrap();
        let mut session = block_on(MeteredReadSession::open_sequential(
            &file_system,
            &path,
            Arc::clone(&services),
            Arc::clone(&root),
            OperationContext::new(Default::default()).unwrap(),
            8,
            8,
        ))
        .unwrap();

        let mut output = [0xCC; 4];
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut read = Box::pin(session.read(&mut output));
        assert!(read.as_mut().poll(&mut context).is_pending());
        drop(read);
        assert_eq!([0xCC; 4], output, "pending scratch is not delivered to the backend");
        assert_eq!(0, root.consumed());
        assert_eq!(8, root.remaining());
        let scratch_permit = services
            .reserve_owned(16 * 1024 * 1024)
            .expect("scratch permit released");
        drop(scratch_permit);
    }
}
