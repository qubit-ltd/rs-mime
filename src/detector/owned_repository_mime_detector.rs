// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Repository MIME detector with shared repository ownership.

use std::sync::Arc;

use qubit_operation::OperationScope;

use super::mime_detector_core::MimeDetectorCore;
use super::stream_based_mime_detector::StreamBasedMimeDetector;
use crate::AsyncMimeDetectorBackend;
use crate::BorrowedMimeInput;
use crate::ContentRequirement;
use crate::InputExtent;
use crate::InspectedExtent;
use crate::MimeCandidates;
use crate::MimeConfig;
use crate::MimeError;
use crate::MimeOperationOptions;
use crate::MimeRepository;
use crate::MimeResult;

/// MIME detector that shares a repository without copying its detection data.
///
/// The shared handle keeps repository rules available for the detector's
/// lifetime, including filename matching, content matching, and read sizing.
#[derive(Debug)]
pub(crate) struct OwnedRepositoryMimeDetector {
    /// Shared selection and refinement behavior.
    core: MimeDetectorCore,
    /// Repository retained for the detector's lifetime.
    repository: Arc<MimeRepository>,
}

impl OwnedRepositoryMimeDetector {
    /// Creates a detector using the supplied configuration and shared
    /// repository.
    ///
    /// # Parameters
    /// - `config`: Settings for candidate selection and content inspection.
    /// - `repository`: Shared filename and content detection rules.
    ///
    /// # Returns
    /// A detector that retains a shared reference to `repository`.
    #[must_use]
    pub(crate) fn new(config: MimeConfig, repository: Arc<MimeRepository>) -> Self {
        let mut core = MimeDetectorCore::from_mime_config(config);
        core.set_mime_relations(repository.relations());
        Self { core, repository }
    }
}

impl StreamBasedMimeDetector for OwnedRepositoryMimeDetector {
    /// Returns configuration and candidate-selection behavior shared by
    /// detectors.
    #[inline]
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Returns the largest content prefix needed by any repository rule.
    #[inline]
    fn max_test_bytes(&self) -> usize {
        self.repository.max_test_bytes()
    }

    /// Returns repository candidate names matched against a filename.
    ///
    /// # Parameters
    /// - `filename`: Path or basename whose extension is matched.
    ///
    /// # Returns
    /// Candidate MIME names in repository detection order.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        self.repository
            .detect_by_filename(filename)
            .into_iter()
            .map(|mime| mime.name().to_owned())
            .collect()
    }

    /// Returns repository candidate names matched against content bytes.
    ///
    /// # Parameters
    /// - `content`: Bytes supplied to repository content detection.
    ///
    /// # Returns
    /// Candidate MIME names in repository detection order.
    ///
    /// # Errors
    /// Returns an error if repository content detection fails.
    fn guess_from_content_bytes(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(self
            .repository
            .detect_by_content(content)
            .into_iter()
            .map(|mime| mime.name().to_owned())
            .collect())
    }
}

impl AsyncMimeDetectorBackend for OwnedRepositoryMimeDetector {
    /// Reports the maximum prefix needed by repository magic rules.
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(self.repository.max_test_bytes())
    }

    /// Reports that the repository backend does not perform random access.
    fn random_access_capability(&self) -> super::RandomAccessCapability {
        super::RandomAccessCapability::Unsupported
    }

    /// Matches repository content and optionally refines the selected result.
    ///
    /// # Parameters
    /// - `scope`: Operation scope used for asynchronous refinement.
    /// - `input`: Borrowed input available to repository matching.
    /// - `options`: Per-operation MIME policy and limits.
    ///
    /// # Returns
    /// Candidate MIME names and the extent inspected by the backend.
    ///
    /// # Errors
    /// Returns an error when matching or async refinement fails.
    fn guess_content_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> crate::MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            let (content, extent, filename) = match input {
                BorrowedMimeInput::Bytes {
                    content,
                    extent,
                    filename,
                } => (content, extent, filename),
                BorrowedMimeInput::Provider { .. } => {
                    return Err(MimeError::detector_backend(
                        "repository",
                        "provider-backed input must be resolved by the managed execution service",
                    ));
                }
            };
            let matched =
                super::internal::async_repository_matcher::match_content(scope, &self.repository, content).await?;
            let candidates = super::internal::async_repository_matcher::select_candidates(
                &self.repository,
                matched,
                filename,
                options.policy,
            );
            let inspected = content.len().min(self.repository.max_test_bytes());
            let inspected_extent = match extent {
                InputExtent::Prefix | InputExtent::Complete => InspectedExtent::Prefix(inspected),
            };
            Ok(MimeCandidates {
                candidates,
                inspected_extent,
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::io::Cursor;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_fs_local::LocalFileSystems;
    use qubit_fs_local::LocalResourcePolicy;
    use qubit_fs_local::host_path_to_logical;

    use super::OwnedRepositoryMimeDetector;
    use crate::AsyncMimeDetectorBackend;
    use crate::ContentRequirement;
    use crate::MimeConfig;
    use crate::MimeDetectionPolicy;
    use crate::MimeDetector;
    use crate::MimeDetectorBackend;
    use crate::MimeRepository;
    use crate::RandomAccessCapability;
    use crate::StreamBasedMimeDetector;

    struct ThreadWake(std::thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    #[test]
    fn test_owned_repository_detector_exercises_stream_and_backend_entry_points() {
        let detector = OwnedRepositoryMimeDetector::new(MimeConfig::default(), MimeRepository::bundled_shared());
        let bytes = b"%PDF-1.7\n";
        assert!(matches!(
            MimeDetectorBackend::content_requirement(&detector),
            ContentRequirement::Prefix(n) if n > 0
        ));
        assert!(matches!(
            MimeDetector::content_requirement(&detector),
            ContentRequirement::Prefix(n) if n > 0
        ));
        assert_eq!(
            RandomAccessCapability::Unsupported,
            AsyncMimeDetectorBackend::random_access_capability(&detector)
        );
        assert_eq!(
            vec!["application/pdf"],
            StreamBasedMimeDetector::guess_from_content_bytes(&detector, bytes)
                .expect("repository should identify the PDF signature")
        );
        assert_eq!(
            vec!["application/pdf"],
            MimeDetectorBackend::guess_from_content(&detector, bytes)
                .expect("backend delegation should identify the PDF signature")
        );
        assert_eq!(
            vec!["application/pdf"],
            StreamBasedMimeDetector::guess_from_filename(&detector, "report.pdf")
        );
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector
                .detect_by_filename("report.pdf")
                .expect("filename lookup should succeed")
        );
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector
                .detect_by_content(bytes)
                .expect("content lookup should succeed")
        );
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector
                .detect(bytes, Some("report.pdf"), MimeDetectionPolicy::VerifyContent)
                .expect("combined lookup should verify the PDF content")
        );
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector
                .detect_prefix(bytes, Some("report.pdf"), MimeDetectionPolicy::VerifyContent)
                .expect("bounded prefix lookup should verify the PDF content")
        );

        let (reader_candidates, reader_prefix) =
            MimeDetectorBackend::guess_from_reader(&detector, &mut Cursor::new(bytes.to_vec()))
                .expect("default backend reader path should use the repository prefix");
        assert_eq!(vec!["application/pdf"], reader_candidates);
        assert_eq!(bytes, reader_prefix.as_slice());
        let reader_result = detector
            .detect_reader(
                &mut Cursor::new(bytes.to_vec()),
                Some("report.pdf"),
                MimeDetectionPolicy::VerifyContent,
            )
            .expect("top-level reader path should use the owned repository");
        assert_eq!(Some("application/pdf".to_owned()), reader_result);

        let path = std::env::temp_dir().join(format!("owned-repository-backend-{}.pdf", std::process::id()));
        std::fs::write(&path, bytes).expect("test PDF should be written");
        let (file_candidates, file_prefix) = MimeDetectorBackend::guess_from_file(&detector, &path)
            .expect("default backend file path should read the repository prefix");
        assert_eq!(vec!["application/pdf"], file_candidates);
        assert_eq!(bytes, file_prefix.as_slice());
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector
                .detect_file(&path, MimeDetectionPolicy::VerifyContent)
                .expect("top-level file path should use the owned repository")
        );

        let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded()).unwrap();
        let logical_path = host_path_to_logical(&path).expect("host path should convert losslessly");
        let (path_candidates, content) = MimeDetectorBackend::guess_from_provider_path(
            &detector,
            &filesystem,
            &logical_path,
            detector.max_buffer_size(),
        )
        .expect("default provider path should read a bounded prefix");
        assert_eq!(vec!["application/pdf"], path_candidates);
        assert_eq!(Some(bytes.to_vec()), content);
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector
                .detect_path(
                    &filesystem,
                    &logical_path,
                    detector.max_buffer_size(),
                    MimeDetectionPolicy::VerifyContent,
                )
                .expect("top-level provider path should use the owned repository")
        );
        std::fs::remove_file(path).unwrap();

        assert!(matches!(
            block_on(MimeDetectorBackend::guess_from_content_async(&detector, bytes)),
            Err(crate::MimeError::AsyncUnsupported)
        ));
    }
}
