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
    ///
    /// # Returns
    ///
    /// The core that supplies MIME policy and candidate-selection behavior.
    #[inline]
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Returns the largest content prefix needed by any repository rule.
    ///
    /// # Returns
    ///
    /// The required prefix length in bytes.
    #[inline]
    fn max_test_bytes(&self) -> usize {
        self.repository.max_test_bytes()
    }

    /// Reports that repository content matching has no asynchronous hook.
    #[inline]
    fn supports_async_content_detection(&self) -> bool {
        false
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
    ///
    /// # Returns
    ///
    /// A prefix requirement sized to the repository's largest magic rule.
    #[inline]
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(self.repository.max_test_bytes())
    }

    /// Reports that the repository backend does not perform random access.
    ///
    /// # Returns
    ///
    /// `Unsupported`, because the backend matches contiguous content prefixes.
    #[inline]
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
                BorrowedMimeInput::ManagedProvider { .. } => {
                    return Err(MimeError::detector_backend(
                        "repository",
                        "managed provider input must be resolved by the selected third-party backend",
                    ));
                }
            };
            if let Some(mime) = super::internal::async_repository_matcher::preferred_filename(
                &self.repository,
                filename,
                options.policy,
            ) {
                if let Some(reason) = scope.context().stop_reason() {
                    return Err(MimeError::DetectorBackend {
                        backend: "repository".to_owned(),
                        reason: format!("operation stopped: {reason:?}"),
                    });
                }
                return Ok(MimeCandidates {
                    candidates: vec![crate::MimeCandidate {
                        mime_type: mime.name().to_owned(),
                        rank: 0,
                    }],
                    inspected_extent: InspectedExtent::Prefix(0),
                    retained_prefix: Vec::new(),
                    staging_bytes: 0,
                });
            }
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
