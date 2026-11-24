// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Shared resource control for a staged MIME input.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use qubit_local_files::LocalTempFile;
use qubit_local_files::LocalTempRecovery;
use qubit_operation::ResourceId;

use crate::MimeError;
use crate::MimeResult;
use crate::command_execution::MimeExecutionWorker;
use crate::detector::StagingPermit;

/// Shared control port for a staged file. Its resource owner is registered
/// exactly once by operation; consumers receive only `ResourceRef`s.
#[must_use = "the staged MIME input owns a temporary file and staging permits"]
pub(crate) struct StagedMimeInput {
    /// Native temporary-file owner retained until cleanup succeeds.
    pub(super) owner: Mutex<StagedTempOwner>,
    /// Permits held until the native file is released.
    pub(super) permits: Mutex<Vec<StagingPermit>>,
    /// Worker that performs writes and cleanup.
    pub(super) worker: MimeExecutionWorker,
    /// Total bytes successfully written into this resource.
    pub(super) staged_bytes: AtomicU64,
    /// Resource identity shared with its cleanup adapter.
    pub(super) cleanup_id: Arc<Mutex<Option<ResourceId>>>,
}

/// Cleanup responsibility retained by an operation-owned staging resource.
pub(super) enum StagedTempOwner {
    /// A temporary file successfully created for staging.
    File(LocalTempFile),
    /// Native cleanup authority returned with a failed temp creation.
    Recovery(LocalTempRecovery),
}

impl StagedTempOwner {
    /// Returns the path associated with the owned resource.
    pub(super) fn path(&self) -> &std::path::Path {
        match self {
            Self::File(owner) => owner.path(),
            Self::Recovery(owner) => owner.path(),
        }
    }

    /// Writes to a staged file, rejecting recovery-only owners.
    pub(super) fn as_file_mut(&mut self) -> std::io::Result<&mut std::fs::File> {
        match self {
            Self::File(owner) => owner.as_file_mut(),
            Self::Recovery(_) => Err(std::io::Error::other("temporary recovery owner is not a staged file")),
        }
    }

    /// Attempts cleanup while retaining this owner on failure.
    pub(super) fn cleanup(&mut self) -> std::io::Result<()> {
        match self {
            Self::File(owner) => owner.cleanup().map_err(|error| error.into_io_error()),
            Self::Recovery(owner) => owner.cleanup().map_err(|error| error.into_io_error()),
        }
    }

    /// Closes the active file handle when this owner is a staged file.
    #[cfg(test)]
    pub(super) fn close(&mut self) {
        if let Self::File(owner) = self {
            owner.close();
        }
    }
}

impl StagedMimeInput {
    /// Builds a staged-file test resource around an existing owner.
    ///
    /// # Parameters
    /// - `owner`: Native temporary file used as the staged resource.
    /// - `permit`: Staging budget held by the resource.
    /// - `worker`: Worker used for cleanup operations.
    ///
    /// # Returns
    /// A staged input retaining the owner and budget permit.
    #[cfg(test)]
    pub(crate) fn new(owner: LocalTempFile, permit: StagingPermit, worker: MimeExecutionWorker) -> Self {
        Self {
            owner: Mutex::new(StagedTempOwner::File(owner)),
            permits: Mutex::new(vec![permit]),
            worker,
            staged_bytes: AtomicU64::new(0),
            cleanup_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Builds a cleanup-only resource around a failed-creation recovery owner.
    pub(crate) fn from_recovery(owner: LocalTempRecovery, worker: MimeExecutionWorker) -> Self {
        Self {
            owner: Mutex::new(StagedTempOwner::Recovery(owner)),
            permits: Mutex::new(Vec::new()),
            worker,
            staged_bytes: AtomicU64::new(0),
            cleanup_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns a cloned native path for command argument construction.
    ///
    /// # Returns
    /// The staged file's native path.
    #[inline]
    pub(crate) fn path(&self) -> MimeResult<std::path::PathBuf> {
        let owner = self.owner.lock().unwrap_or_else(|poison| poison.into_inner());
        Ok(owner.path().to_path_buf())
    }

    /// Returns the number of bytes written into this staged resource.
    ///
    /// # Returns
    /// The total number of successfully written bytes.
    #[must_use]
    #[inline]
    pub(crate) fn staged_bytes(&self) -> u64 {
        self.staged_bytes.load(Ordering::Acquire)
    }

    /// Writes bytes and retains their staging permit for the resource lifetime.
    ///
    /// # Parameters
    /// - `bytes`: Chunk to write to the temporary file.
    /// - `permit`: Budget permit retained until cleanup.
    ///
    /// # Returns
    /// `Ok(())` after the full chunk is written.
    ///
    /// # Errors
    /// Returns an I/O error when the underlying file write fails.
    pub(super) fn write_chunk(&self, bytes: &[u8], permit: StagingPermit) -> MimeResult<()> {
        // Store the permit before writing: if a partial write fails, the
        // registered resource continues to account for every reserved byte.
        self.permits.lock().unwrap_or_else(|p| p.into_inner()).push(permit);
        use std::io::Write;
        let mut owner = self.owner.lock().unwrap_or_else(|p| p.into_inner());
        owner.as_file_mut()?.write_all(bytes).map_err(MimeError::Io)?;
        self.staged_bytes.fetch_add(bytes.len() as u64, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use qubit_local_files::LocalFileSystem;
    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;

    use super::StagedMimeInput;
    use crate::MimeError;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::command_execution::MimeExecutionWorker;
    use crate::detector::MimeExecutionServices;

    #[test]
    fn test_failed_chunk_write_keeps_budget_until_the_temp_owner_is_dropped() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = MimeExecutionServices::new(manager, group, port.clone(), 32).unwrap();
        let budget = services.staging_budget();
        let first_permit = budget.try_reserve(5).unwrap();
        let write_permit = budget.try_reserve(3).unwrap();
        let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
        let path = owner.path().to_path_buf();
        let mut staged = StagedMimeInput::new(owner, first_permit, MimeExecutionWorker::new(port));
        staged.owner.lock().unwrap().close();

        let error = staged
            .write_chunk(b"abc", write_permit)
            .expect_err("writing after close must fail");
        assert!(matches!(error, MimeError::Io(_)));
        assert_eq!(8, budget.used_bytes(), "both permits remain owned after write failure");

        staged.owner.get_mut().unwrap().cleanup().unwrap();
        drop(staged);
        assert_eq!(
            0,
            budget.used_bytes(),
            "dropping the cleaned resource releases its permits"
        );
        assert!(!path.exists());
    }

    #[test]
    fn test_owner_and_permit_mutex_poisoning_do_not_block_staged_input_use() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = MimeExecutionServices::new(manager, group, port.clone(), 32).unwrap();
        let budget = services.staging_budget();
        let resource = Arc::new(StagedMimeInput::new(
            LocalFileSystem::host().unwrap().create_temp_file().unwrap(),
            budget.try_reserve(3).unwrap(),
            MimeExecutionWorker::new(port),
        ));
        let path = resource.path().unwrap();

        let owner_resource = Arc::clone(&resource);
        assert!(
            std::thread::spawn(move || {
                let _owner = owner_resource.owner.lock().unwrap();
                panic!("poison staged-file owner mutex");
            })
            .join()
            .is_err()
        );
        let permits_resource = Arc::clone(&resource);
        assert!(
            std::thread::spawn(move || {
                let _permits = permits_resource.permits.lock().unwrap();
                panic!("poison staging-permit mutex");
            })
            .join()
            .is_err()
        );

        assert_eq!(path, resource.path().unwrap());
        resource
            .write_chunk(b"abc", budget.try_reserve(3).unwrap())
            .expect("poisoned staging state should recover for a later write");
        assert_eq!(b"abc", std::fs::read(&path).unwrap().as_slice());
        assert_eq!(6, budget.used_bytes());

        resource
            .owner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .cleanup()
            .unwrap();
        drop(resource);
        assert_eq!(0, budget.used_bytes());
        assert!(!path.exists());
    }
}
