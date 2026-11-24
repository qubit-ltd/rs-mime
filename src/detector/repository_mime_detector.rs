// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Repository-backed MIME detector.

use std::path::Path;
use std::sync::Arc;

use qubit_io::std_io::ReadSeek;
use qubit_operation::OperationScope;

use super::RandomAccessCapability;
use crate::AsyncMimeDetectorBackend;
use crate::BorrowedMimeInput;
use crate::ContentRequirement;
use crate::InputExtent;
use crate::InspectedExtent;
use crate::MimeCandidate;
use crate::MimeCandidates;
use crate::MimeConfig;
use crate::MimeDetectionPolicy;
use crate::MimeDetector;
use crate::MimeDetectorCore;
use crate::MimeError;
use crate::MimeOperationOptions;
use crate::MimeRepository;
use crate::MimeResult;
use crate::StreamBasedMimeDetector;

/// MIME detector backed by a [`MimeRepository`].
///
/// # Type Parameters
///
/// * `'a` - Lifetime for which the detector borrows its repository.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeDetector;
/// use qubit_mime::RepositoryMimeDetector;
/// let detector = RepositoryMimeDetector::new()?;
/// assert_eq!(detector.detect_by_filename("document.pdf")?, Some("application/pdf".to_owned()));
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct RepositoryMimeDetector<'a> {
    /// The shared detector core.
    core: MimeDetectorCore,
    /// The repository used for all detections.
    repository: &'a MimeRepository,
}

impl RepositoryMimeDetector<'static> {
    /// Creates a detector using the embedded freedesktop MIME repository.
    ///
    /// # Returns
    /// A repository-backed detector.
    ///
    /// # Errors
    /// The embedded database is parsed from crate resources and is expected to
    /// be valid; this method keeps a `Result` return type for API consistency.
    #[inline]
    pub fn new() -> MimeResult<Self> {
        Ok(Self::with_repository(default_repository()))
    }

    /// Creates a detector using the embedded repository and explicit config.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// A repository-backed detector.
    #[inline]
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self::with_repository_and_config(default_repository(), config)
    }
}

impl Default for RepositoryMimeDetector<'static> {
    /// Creates a detector backed by the embedded repository.
    #[inline]
    fn default() -> Self {
        Self::new().expect("embedded MIME repository should parse")
    }
}

impl<'a> RepositoryMimeDetector<'a> {
    /// Creates a detector using an explicit repository.
    ///
    /// # Parameters
    /// - `repository`: Repository used for all detections.
    ///
    /// # Returns
    /// A detector borrowing `repository`.
    #[inline]
    pub fn with_repository(repository: &'a MimeRepository) -> Self {
        Self::with_repository_and_config(repository, MimeConfig::default())
    }

    /// Creates a detector using an explicit repository and config.
    ///
    /// # Parameters
    /// - `repository`: Repository used for all detections.
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// A detector borrowing `repository`.
    pub fn with_repository_and_config(repository: &'a MimeRepository, config: MimeConfig) -> Self {
        let mut core = MimeDetectorCore::from_mime_config(config);
        core.set_mime_relations(repository.relations());
        Self { core, repository }
    }

    /// Gets the shared detector core.
    ///
    /// # Returns
    /// Shared detector core.
    #[must_use]
    #[inline]
    pub fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets mutable shared detector core.
    ///
    /// # Returns
    /// Mutable shared detector core.
    #[must_use]
    #[inline]
    pub fn core_mut(&mut self) -> &mut MimeDetectorCore {
        &mut self.core
    }

    /// Gets the underlying repository.
    ///
    /// # Returns
    /// Repository used by this detector.
    #[must_use]
    #[inline]
    pub fn repository(&self) -> &'a MimeRepository {
        self.repository
    }

    /// Detects a MIME type from a filename.
    ///
    /// # Parameters
    /// - `filename`: Path or basename to inspect.
    ///
    /// # Returns
    /// First MIME type matched by filename, or `None`.
    ///
    /// # Errors
    /// Propagates errors returned by the configured MIME detection backend.
    #[inline]
    pub fn detect_by_filename(&self, filename: &str) -> MimeResult<Option<String>> {
        <Self as MimeDetector>::detect_by_filename(self, filename)
    }

    /// Detects a MIME type from content bytes.
    ///
    /// # Parameters
    /// - `bytes`: Content bytes to inspect. When precise media detection is
    ///   enabled and a classifier is configured, provide the complete resource
    ///   because the classifier may inspect the entire media stream. Use
    ///   [`MimeDetector::detect_prefix`] when only a bounded prefix is
    ///   available.
    ///
    /// # Returns
    /// First MIME type matched by magic, or `None`.
    ///
    /// # Errors
    /// Propagates errors returned while inspecting content or refining a
    /// detected MIME type, including media-classifier errors.
    #[inline]
    pub fn detect_by_content(&self, bytes: &[u8]) -> MimeResult<Option<String>> {
        <Self as MimeDetector>::detect_by_content(self, bytes)
    }

    /// Detects a MIME type from content bytes and an optional filename.
    ///
    /// # Parameters
    /// - `bytes`: Complete resource bytes to inspect. When precise media
    ///   detection is enabled and a classifier is configured, the classifier
    ///   may inspect the full resource. Use [`MimeDetector::detect_prefix`]
    ///   when only a bounded prefix is available.
    /// - `filename`: Optional path or basename used for glob detection.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Propagates errors returned while inspecting content or refining a
    /// detected MIME type, including media-classifier errors.
    #[inline]
    pub fn detect_bytes(
        &self,
        bytes: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.detect(bytes, filename, policy)
    }

    /// Detects a MIME type from the complete seekable resource without
    /// consuming its position. Detection starts at logical byte zero and
    /// restores the reader's original position.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original stream position is restored.
    /// - `filename`: Optional path or basename used for glob detection.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when reading or seeking
    /// fails, or another detector or media-classifier error when detection
    /// cannot complete.
    #[inline]
    pub fn detect_reader(
        &self,
        reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        <Self as MimeDetector>::detect_reader(self, reader, filename, policy)
    }

    /// Detects a MIME type from a local file.
    ///
    /// # Parameters
    /// - `file`: Local file path to open.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the file cannot be
    /// opened or read, or another detector or media-classifier error when
    /// detection cannot complete.
    #[inline]
    pub fn detect_file(&self, file: &Path, policy: MimeDetectionPolicy) -> MimeResult<Option<String>> {
        <Self as MimeDetector>::detect_file(self, file, policy)
    }

    /// Guesses MIME type names from filename rules.
    ///
    /// # Parameters
    /// - `filename`: Filename or path.
    ///
    /// # Returns
    /// Candidate MIME type names.
    #[must_use]
    pub fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        self.repository
            .detect_by_filename(filename)
            .into_iter()
            .map(|mime_type| mime_type.name().to_owned())
            .collect()
    }

    /// Guesses MIME type names from content magic rules.
    ///
    /// # Parameters
    /// - `bytes`: Content bytes to inspect.
    ///
    /// # Returns
    /// Candidate MIME type names.
    #[must_use]
    pub fn guess_from_content(&self, bytes: &[u8]) -> Vec<String> {
        self.repository
            .detect_by_content(bytes)
            .into_iter()
            .map(|mime_type| mime_type.name().to_owned())
            .collect()
    }

    /// Adds explicit async stream refinement to this detector's selection core.
    ///
    /// # Parameters
    /// - `classifier`: Optional operation-backed stream classifier.
    ///
    /// # Returns
    /// This detector with its async classifier configuration replaced.
    #[inline]
    pub(crate) fn with_async_media_stream_classifier(
        mut self,
        classifier: Option<Arc<dyn crate::AsyncMediaStreamClassifierBackend>>,
    ) -> Self {
        self.core.set_async_media_stream_classifier(classifier);
        self
    }
}

impl AsyncMimeDetectorBackend for RepositoryMimeDetector<'_> {
    /// Reports the maximum prefix length required by repository matching.
    #[inline]
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(self.repository.max_test_bytes())
    }

    /// Reports that repository matching does not perform random access.
    #[inline]
    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    /// Matches content against the repository and applies configured
    /// refinement.
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
            if let Some(mime) =
                super::internal::async_repository_matcher::preferred_filename(self.repository, filename, options.policy)
            {
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
                super::internal::async_repository_matcher::match_content(scope, self.repository, content).await?;
            let from_filename = filename.map_or_else(Vec::new, |filename| {
                self.repository
                    .detect_by_filename(filename)
                    .into_iter()
                    .map(|mime| mime.name().to_owned())
                    .collect()
            });
            let from_content = matched
                .into_iter()
                .map(|candidate| candidate.mime_type)
                .collect::<Vec<_>>();
            let selected = self
                .core
                .select_result_in(scope, &from_filename, &from_content, input, options)
                .await?;
            let candidates = selected
                .into_iter()
                .map(|mime_type| MimeCandidate { mime_type, rank: 0 })
                .collect();
            let inspected = content.len().min(self.repository.max_test_bytes());
            Ok(MimeCandidates {
                candidates,
                inspected_extent: match extent {
                    InputExtent::Prefix | InputExtent::Complete => InspectedExtent::Prefix(inspected),
                },
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }

    /// Matches provider-derived content while keeping its basename typed.
    ///
    /// # Parameters
    /// - `scope`: Operation scope used to observe cancellation.
    /// - `input`: Borrowed bytes or managed provider input to inspect.
    /// - `basename`: Optional original filename used for rule matching.
    /// - `options`: Limits applied to this detection request.
    ///
    /// # Returns
    /// The matching MIME candidates and content inspection metadata.
    ///
    /// # Errors
    /// Returns an error when input access, detection, or request limits fail.
    fn guess_provider_content_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        input: BorrowedMimeInput<'a>,
        basename: Option<&'a str>,
        options: &'a MimeOperationOptions,
    ) -> crate::MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            let (content, extent) = match input {
                BorrowedMimeInput::Bytes { content, extent, .. } => (content, extent),
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
            if let Some(mime) =
                super::internal::async_repository_matcher::preferred_basename(self.repository, basename, options.policy)
            {
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
                super::internal::async_repository_matcher::match_content(scope, self.repository, content).await?;
            let from_filename = basename.map_or_else(Vec::new, |basename| {
                self.repository
                    .detect_by_filename_basename(basename)
                    .into_iter()
                    .map(|mime| mime.name().to_owned())
                    .collect()
            });
            let from_content = matched
                .into_iter()
                .map(|candidate| candidate.mime_type)
                .collect::<Vec<_>>();
            let selected = self
                .core
                .select_result_in(scope, &from_filename, &from_content, input, options)
                .await?;
            let candidates = selected
                .into_iter()
                .map(|mime_type| MimeCandidate { mime_type, rank: 0 })
                .collect();
            let inspected = content.len().min(self.repository.max_test_bytes());
            Ok(MimeCandidates {
                candidates,
                inspected_extent: match extent {
                    InputExtent::Prefix | InputExtent::Complete => InspectedExtent::Prefix(inspected),
                },
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

/// Gets the embedded default repository.
///
/// # Returns
/// Shared parsed repository.
#[must_use]
#[inline]
pub(crate) fn default_repository() -> &'static MimeRepository {
    MimeRepository::bundled()
}

impl<'a> StreamBasedMimeDetector for RepositoryMimeDetector<'a> {
    /// Gets the shared detector core.
    #[inline]
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets the maximum content prefix length from the repository.
    #[inline]
    fn max_test_bytes(&self) -> usize {
        self.repository.max_test_bytes()
    }

    /// Reports that repository content matching has no asynchronous hook.
    #[inline]
    fn supports_async_content_detection(&self) -> bool {
        false
    }

    /// Guesses MIME type names from filename rules.
    ///
    /// # Parameters
    /// - `filename`: File path or basename to match against repository rules.
    #[inline]
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        RepositoryMimeDetector::guess_from_filename(self, filename)
    }

    /// Returns repository candidate names matched against a native basename.
    ///
    /// # Parameters
    /// - `basename`: Native basename matched against repository glob rules.
    ///
    /// # Returns
    /// Candidate MIME names in repository detection order.
    fn guess_from_filename_basename(&self, basename: &str) -> Vec<String> {
        self.repository
            .detect_by_filename_basename(basename)
            .into_iter()
            .map(|mime_type| mime_type.name().to_owned())
            .collect()
    }

    /// Guesses MIME type names from content magic rules.
    ///
    /// # Parameters
    /// - `content`: Content bytes to inspect against repository magic rules.
    #[inline]
    fn guess_from_content_bytes(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(RepositoryMimeDetector::guess_from_content(self, content))
    }
}
