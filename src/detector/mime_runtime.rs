// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::sync::Arc;

use qubit_operation::OperationScope;
use qubit_spi::ProviderSelection;

use super::internal::mime_submission;
pub use super::mime_detector_context::MimeDetectorContext;
use super::mime_detector_registry::MimeDetectorRegistry;
use super::mime_execution_services::MimeExecutionServices;
use super::mime_input::OwnedMimeInput;
use super::mime_operation::AsyncMimeOperation;
use super::mime_operation::MimeOperation;
use super::mime_operation::MimeSubmitFailure;
use super::mime_operation::MimeSubmitRejection;
use super::mime_operation_options::MimeOperationOptions;
use super::owned_repository_mime_detector::OwnedRepositoryMimeDetector;
use crate::MediaStreamClassifierRegistry;
use crate::MimeConfig;
use crate::MimeDetector;
use crate::MimeRepository;
use crate::MimeResult;

/// Process-local runtime owning shared MIME configuration and repository.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeDetector;
/// use qubit_mime::detector::MimeRuntime;
/// let runtime = MimeRuntime::builtin()?;
/// let detector = runtime.create_detector()?;
/// assert_eq!(detector.detect_by_filename("document.pdf")?, Some("application/pdf".to_owned()));
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
#[must_use = "use the runtime to create detectors or submit MIME operations"]
#[derive(Clone)]
pub struct MimeRuntime {
    /// Configuration and repository shared by detectors created by this
    /// runtime.
    context: MimeDetectorContext,
    /// Optional services for managed operations.
    execution_services: Option<Arc<MimeExecutionServices>>,
    /// Selected async provider identity and detector backend.
    async_backend: Option<(String, Arc<dyn crate::AsyncMimeDetectorBackend>, bool)>,
    /// Repository selection is known to use the crate-owned async matcher.
    async_repository: Option<Arc<MimeRepository>>,
}

impl fmt::Debug for MimeRuntime {
    /// Formats the runtime's shared context and configured backend presence.
    ///
    /// # Parameters
    /// - `self`: Runtime being formatted.
    /// - `f`: Formatter receiving the debug representation.
    ///
    /// # Returns
    /// The formatter after writing the runtime representation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MimeRuntime")
            .field("context", &self.context)
            .field("has_execution_services", &self.execution_services.is_some())
            .field("has_async_backend", &self.async_backend.is_some())
            .finish()
    }
}

impl MimeRuntime {
    /// Creates a runtime that shares the supplied repository handle.
    ///
    /// The configuration is moved into a shared handle so detectors created by
    /// this runtime use the same immutable settings. Managed submissions also
    /// use this repository when the selected async backend is repository-backed
    /// or declares the bundled repository as its candidate source.
    ///
    /// # Parameters
    ///
    /// - `config`: detection settings used by detectors created by this
    ///   runtime.
    /// - `repository`: shared MIME repository used for detection.
    ///
    /// # Returns
    ///
    /// A runtime retaining the configuration and repository for later detector
    /// creation.
    pub fn new(config: MimeConfig, repository: Arc<MimeRepository>) -> Self {
        Self {
            context: MimeDetectorContext::new(Arc::new(config), repository),
            execution_services: None,
            async_backend: None,
            async_repository: None,
        }
    }

    /// Creates a runtime using the default configuration and bundled
    /// repository.
    ///
    /// This explicitly chooses bundled data. `Default` delegates to this
    /// constructor; `new` retains the repository supplied by the caller.
    ///
    /// # Returns
    ///
    /// A runtime backed by the crate's bundled MIME data.
    ///
    /// # Errors
    ///
    /// This implementation currently initializes only infallible built-in
    /// values and returns `Ok`. The result type is retained for consistency
    /// with detector creation APIs.
    pub fn builtin() -> MimeResult<Self> {
        Ok(Self::new(MimeConfig::default(), MimeRepository::bundled_shared()))
    }

    /// Creates a detector using this runtime's configuration and repository.
    ///
    /// The detector receives a clone of the configuration value and a clone of
    /// the repository's `Arc`, so the repository remains alive independently of
    /// this runtime.
    ///
    /// # Returns
    ///
    /// A shared handle to a repository-backed MIME detector.
    ///
    /// # Errors
    ///
    /// This implementation currently constructs the repository-backed
    /// detector infallibly and returns `Ok`.
    pub fn create_detector(&self) -> MimeResult<Arc<dyn MimeDetector>> {
        Ok(Arc::new(OwnedRepositoryMimeDetector::new(
            self.context.config().clone(),
            self.context.repository_arc(),
        )))
    }

    /// Returns the immutable configuration and repository shared by this
    /// runtime.
    ///
    /// # Returns
    ///
    /// A reference to this runtime's context. The reference is valid for the
    /// lifetime of the borrow and does not clone the shared handles.
    #[must_use]
    pub fn context(&self) -> &MimeDetectorContext {
        &self.context
    }

    /// Attaches shared operation and MIME execution services without starting
    /// workers. This also selects the runtime's repository-backed async
    /// detector as the default managed backend, using the repository already
    /// retained by the runtime. An explicit provider selection can replace the
    /// managed backend afterward without changing the synchronous detector
    /// facade or its repository.
    ///
    /// # Parameters
    /// - `services`: Shared operation manager, group, worker adapter, and
    ///   limits.
    ///
    /// # Returns
    /// This runtime configured for managed operations.
    pub fn with_execution_services(mut self, services: MimeExecutionServices) -> Self {
        let backend = OwnedRepositoryMimeDetector::new(self.context.config().clone(), self.context.repository_arc());
        self.async_backend = Some(("repository".to_owned(), Arc::new(backend), false));
        self.async_repository = Some(self.context.repository_arc());
        self.execution_services = Some(Arc::new(services));
        self
    }

    /// Selects an explicitly async-capable provider for managed submissions.
    /// The synchronous detector facade continues to use the repository-backed
    /// implementation retained by this runtime. Repository filename
    /// short-circuiting is available only when the selected provider supplies
    /// a repository source; a provider such as `file` consumes complete content
    /// and its command result.
    ///
    /// # Parameters
    /// - `registry`: Registry containing async provider factories.
    /// - `selection`: Provider selection to resolve.
    ///
    /// # Returns
    /// This runtime with the selected async backend.
    ///
    /// # Errors
    /// Returns an error when execution services are absent or provider
    /// resolution/factory creation fails. A sync-only provider returns
    /// [`crate::MimeError::AsyncUnsupported`] here, before any submission.
    pub fn with_async_provider_selection(
        mut self,
        registry: &MimeDetectorRegistry,
        selection: &ProviderSelection,
    ) -> MimeResult<Self> {
        let services = self
            .execution_services
            .as_ref()
            .ok_or(crate::MimeError::ExecutionServicesRequired)?;
        let (id, backend, repository, self_metered_provider_reads) =
            registry.create_async_backend_internal(selection, self.context.config(), services, None)?;
        self.async_backend = Some((id, backend, self_metered_provider_reads));
        self.async_repository = repository;
        Ok(self)
    }

    /// Selects async detector and classifier providers for managed operation
    /// submission. The selected classifier is attached to detector backends
    /// that implement async MIME refinement. As with detector-only selection,
    /// this changes the managed providers while the synchronous facade keeps
    /// this runtime's original repository.
    ///
    /// # Parameters
    /// - `detector_registry`: Registry containing async detector providers.
    /// - `detector_selection`: Detector selection to resolve.
    /// - `classifier_registry`: Registry containing async classifier providers.
    /// - `classifier_selection`: Classifier selection to resolve.
    ///
    /// # Returns
    /// This runtime with the selected async detector and classifier.
    ///
    /// # Errors
    /// Returns an error when services are absent or either provider selection
    /// fails. A selected detector factory can reject an unsupported classifier
    /// during this configuration call, before any operation is submitted. The
    /// built-in `file` provider rejects every supplied async classifier with
    /// [`crate::MimeError::UnsupportedCapability`].
    pub fn with_async_provider_selection_and_classifier(
        mut self,
        detector_registry: &MimeDetectorRegistry,
        detector_selection: &ProviderSelection,
        classifier_registry: &MediaStreamClassifierRegistry,
        classifier_selection: &ProviderSelection,
    ) -> MimeResult<Self> {
        let services = self
            .execution_services
            .as_ref()
            .ok_or(crate::MimeError::ExecutionServicesRequired)?;
        let (_, classifier) =
            classifier_registry.create_async_backend(classifier_selection, self.context.config(), services)?;
        let (id, backend, repository, self_metered_provider_reads) = detector_registry.create_async_backend_internal(
            detector_selection,
            self.context.config(),
            services,
            Some(classifier),
        )?;
        self.async_backend = Some((id, backend, self_metered_provider_reads));
        self.async_repository = repository;
        Ok(self)
    }

    /// Submits detection to the MIME root group.
    ///
    /// # Parameters
    /// - `input`: Owned input returned unchanged when submission is rejected.
    /// - `options`: Detection policy, resource limits, and operation context.
    ///
    /// # Returns
    /// An operation handle, or the rejected input and its reason.
    ///
    /// # Errors
    /// Returns `MimeSubmitFailure` if services or an async backend are
    /// unavailable, request preflight fails, or root-group admission is
    /// rejected. The failure retains the input for recovery.
    ///
    /// # Behavior
    /// An error means the operation was not accepted and returns its input.
    /// After acceptance, detection errors are observed through the operation
    /// outcome as `OperationOutcome::Failed`; resource release is observed
    /// separately through settlement. The async path checks deadline and stop
    /// again when the backend becomes ready, before committing its result, so
    /// an expired deadline can stop a ready success or error. A stop reason
    /// accepted before the deadline, such as `UserCancelled`, is retained.
    pub fn try_submit_detection(
        &self,
        input: OwnedMimeInput,
        options: MimeOperationOptions,
    ) -> Result<AsyncMimeOperation, MimeSubmitFailure> {
        self.submit_async(None, input, options)
    }

    /// Submits detection as a child of the given operation.
    ///
    /// # Parameters
    /// - `parent`: Parent operation that owns this child operation.
    /// - `input`: Owned input returned unchanged when submission is rejected.
    /// - `options`: Detection policy, resource limits, and operation context.
    ///
    /// # Returns
    /// An operation handle, or the rejected input and its reason.
    ///
    /// # Errors
    /// Returns `MimeSubmitFailure` if services or an async backend are
    /// unavailable, the parent cannot accept a child, preflight, parent
    /// resource reservation, group binding, or admission fails. These
    /// failures retain the input for recovery. If committing the parent
    /// read-budget anchor fails after admission, the operation returns
    /// [`crate::MimeError::ReadBudgetBinding`].
    ///
    /// # Behavior
    /// Owned byte inputs and children resolved by a unique filename do not bind
    /// an ancestor read tree or reserve its resource slot. A descendant that
    /// needs provider reads propagates the ancestor identity and applicable
    /// scope cap, then reserves against the shared tree on its first read. If
    /// that binding cannot be admitted or the parent has ended, this method
    /// returns the input in `MimeSubmitFailure`. A pending binding that fails
    /// to commit after admission is reported by the accepted operation as
    /// [`crate::MimeError::ReadBudgetBinding`].
    pub fn try_submit_detection_in(
        &self,
        parent: &OperationScope,
        input: OwnedMimeInput,
        options: MimeOperationOptions,
    ) -> Result<AsyncMimeOperation, MimeSubmitFailure> {
        self.submit_async(Some(parent), input, options)
    }

    /// Submits a synchronous child operation using the parent's manager view.
    ///
    /// # Parameters
    /// - `parent`: Parent operation that owns this child operation.
    /// - `input`: Owned input returned unchanged when submission is rejected.
    /// - `options`: Detection policy, resource limits, and operation context.
    ///
    /// # Returns
    /// An operation handle, or the rejected input and its reason.
    ///
    /// # Errors
    /// Returns `MimeSubmitFailure` if the parent cannot accept a child, the
    /// provider-backed input is unsupported, detector creation or preflight
    /// fails, group binding, or admission fails. These failures retain the
    /// input for recovery.
    pub fn try_submit_detection_sync_in(
        &self,
        parent: &OperationScope,
        input: OwnedMimeInput,
        options: MimeOperationOptions,
    ) -> Result<MimeOperation, MimeSubmitFailure> {
        let Some(services) = self.execution_services.as_ref().cloned() else {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::ExecutionServicesRequired,
                input,
            });
        };
        let Some(_admission) = services.begin_admission(true) else {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::ExecutionServicesRequired,
                input,
            });
        };
        if matches!(&input, OwnedMimeInput::Provider { .. }) {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::AsyncUnsupported,
                input,
            });
        }
        let detector = match self.create_detector() {
            Ok(detector) => detector,
            Err(_) => {
                return Err(MimeSubmitFailure {
                    reason: MimeSubmitRejection::InvalidOptions("detector construction failed".into()),
                    input,
                });
            }
        };
        let permit = match mime_submission::preflight(
            &input,
            &options,
            detector.content_requirement(),
            crate::RandomAccessCapability::Unsupported,
            detector.max_buffer_size(),
            &services,
            false,
        ) {
            Ok(permit) => permit,
            Err(reason) => return Err(MimeSubmitFailure { reason, input }),
        };
        let tree_limit = options
            .max_read_bytes
            .unwrap_or(services.max_read_bytes())
            .min(services.max_read_bytes());
        // This synchronous path accepts complete byte inputs only, so it does
        // not create a parent read-tree anchor. It borrows an existing parent
        // tree when available; otherwise it uses a standalone context.
        let read_tree = services
            .existing_read_tree_for_parent(parent, tree_limit)
            .unwrap_or_else(|| services.new_read_tree(tree_limit));
        let cell = mime_submission::cell(input);
        let body_cell = Arc::clone(&cell);
        let body_services = Arc::clone(&services);
        let body_tree = read_tree;
        let repository_max_test_bytes = self.context.repository().max_test_bytes();
        let repository = self.context.repository_arc();
        let body = move |scope: OperationScope| {
            body_services.bind_read_tree(scope.id(), &body_tree);
            let input = mime_submission::take(&body_cell);
            let result = match input {
                OwnedMimeInput::Bytes {
                    content,
                    extent,
                    filename,
                } => {
                    let filename_short_circuit = options.policy == crate::MimeDetectionPolicy::PreferFilename
                        && filename
                            .as_deref()
                            .is_some_and(|name| repository.detect_by_filename(name).len() == 1);
                    let inspected_bytes = if filename_short_circuit {
                        0
                    } else {
                        content.len().min(repository_max_test_bytes)
                    };
                    let mime_type = if filename_short_circuit {
                        detector.detect_by_filename(filename.as_deref().expect("short circuit requires filename"))?
                    } else {
                        match extent {
                            super::mime_input::InputExtent::Prefix => detector.detect_prefix(
                                &content[..inspected_bytes],
                                filename.as_deref(),
                                options.policy,
                            )?,
                            super::mime_input::InputExtent::Complete => {
                                detector.detect(&content, filename.as_deref(), options.policy)?
                            }
                        }
                    };
                    Ok(crate::MimeDetectionReport {
                        mime_type,
                        backend_id: "repository".into(),
                        inspected_extent: super::mime_input::InspectedExtent::Prefix(inspected_bytes),
                        budget: super::mime_detection_report::MimeBudgetSummary {
                            owned_bytes: content.len(),
                            ..Default::default()
                        },
                    })
                }
                OwnedMimeInput::Provider { .. } => {
                    unreachable!("provider input is rejected before admission")
                }
            };
            let _scope = scope;
            let _permit = permit;
            result
        };
        let manager = match parent.manager().with_group(&services.root_group) {
            Ok(manager) => manager,
            Err(error) => {
                return Err(MimeSubmitFailure {
                    reason: MimeSubmitRejection::GroupBinding(error),
                    input: mime_submission::return_input(&cell),
                });
            }
        };
        match manager.try_submit(options.operation, body) {
            Ok(handle) => {
                services.track_observer(handle.observer().as_async());
                Ok(handle)
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                Err(MimeSubmitFailure {
                    reason: MimeSubmitRejection::Admission(reason),
                    input: mime_submission::return_input(&cell),
                })
            }
        }
    }

    /// Submits the input through the configured async backend.
    ///
    /// # Parameters
    /// - `parent`: Optional parent operation for child submission.
    /// - `input`: Owned input returned when admission is rejected.
    /// - `options`: Detection policy, limits, and operation context.
    ///
    /// # Returns
    /// An operation handle, or the rejected input and its reason.
    ///
    /// # Errors
    /// Returns `MimeSubmitFailure` if services or a backend are unavailable,
    /// the parent cannot accept a child, preflight, read-budget or group
    /// binding fails, or operation admission is rejected. The failure retains
    /// the input for recovery.
    fn submit_async(
        &self,
        parent: Option<&OperationScope>,
        input: OwnedMimeInput,
        options: MimeOperationOptions,
    ) -> Result<AsyncMimeOperation, MimeSubmitFailure> {
        let Some(services) = self.execution_services.as_ref().cloned() else {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::ExecutionServicesRequired,
                input,
            });
        };
        let Some((backend_id, backend, self_metered_provider_reads)) = self.async_backend.as_ref().cloned() else {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::AsyncUnsupported,
                input,
            });
        };
        let Some(_admission) = services.begin_admission(parent.is_some()) else {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::ExecutionServicesRequired,
                input,
            });
        };
        // Extension hooks may be reentrant; an in-flight ticket keeps this
        // admission visible to shutdown without holding the submission lock.
        let read_plan = mime_submission::ProviderReadPlan::capture(backend.as_ref(), self_metered_provider_reads);
        let repository = self.async_repository.clone();
        let filename_winner = managed_filename_winner(&backend_id, repository.as_ref(), &input, options.policy);
        let max_buffer_size = self.context.config().max_buffer_size();
        let filename_short_circuit = filename_winner.is_some();
        let requirement =
            read_plan.requirement_for(filename_short_circuit && matches!(&input, OwnedMimeInput::Provider { .. }));
        let permit = match mime_submission::preflight(
            &input,
            &options,
            requirement,
            read_plan.random_access,
            max_buffer_size,
            &services,
            read_plan.managed_provider_reads,
        ) {
            Ok(permit) => permit,
            Err(reason) => return Err(MimeSubmitFailure { reason, input }),
        };
        let tree_limit = options
            .max_read_bytes
            .unwrap_or(services.max_read_bytes())
            .min(services.max_read_bytes());
        let (pending_tree, standalone_tree) = match parent {
            Some(parent) if matches!(&input, OwnedMimeInput::Provider { .. }) && !filename_short_circuit => {
                match services.prepare_read_tree_for_parent(parent, tree_limit, tree_limit) {
                    Ok(binding) => (Some(binding), None),
                    Err(kind) => {
                        return Err(MimeSubmitFailure {
                            reason: MimeSubmitRejection::ReadBudgetBinding(kind),
                            input,
                        });
                    }
                }
            }
            Some(parent) => (
                None,
                Some(services.associate_read_tree_for_parent(parent, tree_limit, tree_limit)),
            ),
            None => (None, Some(services.new_read_tree(tree_limit))),
        };
        let cell = mime_submission::cell(input);
        let body_cell = Arc::clone(&cell);
        let body_services = Arc::clone(&services);
        let body_options = options.clone();
        let body_pending_tree = pending_tree;
        let body_standalone_tree = standalone_tree;
        let body_backend_id = backend_id.clone();
        let body_max_buffer_size = max_buffer_size;
        let body_read_plan = read_plan;
        let body_filename_winner = filename_winner.map(|mime| mime.name().to_owned());
        let body = move |scope: OperationScope| async move {
            let read_tree = if let Some(pending_tree) = body_pending_tree {
                pending_tree.commit().map_err(crate::MimeError::ReadBudgetBinding)?
            } else {
                body_standalone_tree.expect("root submissions have a standalone read tree")
            };
            mime_submission::detect(mime_submission::MimeSubmission {
                scope,
                input: mime_submission::take(&body_cell),
                services: body_services,
                backend,
                read_plan: body_read_plan,
                backend_id: body_backend_id,
                filename_winner: body_filename_winner,
                max_buffer_size: body_max_buffer_size,
                options: body_options,
                permit: Some(permit),
                read_tree,
            })
            .await
        };
        let result = if let Some(parent) = parent {
            match parent.manager().with_group(&services.root_group) {
                Ok(manager) => manager.as_async().try_submit(options.operation, body),
                Err(error) => {
                    return Err(MimeSubmitFailure {
                        reason: MimeSubmitRejection::GroupBinding(error),
                        input: mime_submission::return_input(&cell),
                    });
                }
            }
        } else {
            services.async_root_group.try_submit(options.operation, body)
        };
        match result {
            Ok(handle) => {
                services.track_observer(handle.observer());
                Ok(handle)
            }
            Err(error) => {
                let (reason, _) = error.into_parts();
                Err(MimeSubmitFailure {
                    reason: MimeSubmitRejection::Admission(reason),
                    input: mime_submission::return_input(&cell),
                })
            }
        }
    }
}

/// Finds a preferred filename candidate for managed content detection.
///
/// # Parameters
///
/// - `backend_id`: The selected managed backend identifier.
/// - `repository`: The repository used by repository-backed detection.
/// - `input`: The owned content and optional filename input.
/// - `policy`: The filename matching policy.
fn managed_filename_winner<'a>(
    backend_id: &str,
    repository: Option<&'a Arc<MimeRepository>>,
    input: &OwnedMimeInput,
    policy: crate::MimeDetectionPolicy,
) -> Option<&'a crate::MimeType> {
    if backend_id == "file" {
        return None;
    }
    repository.and_then(|repository| match input {
        OwnedMimeInput::Bytes { filename, .. } => {
            super::internal::async_repository_matcher::preferred_filename(repository, filename.as_deref(), policy)
        }
        OwnedMimeInput::Provider { path, .. } => {
            super::internal::async_repository_matcher::preferred_basename(repository, path.file_name(), policy)
        }
    })
}

impl Default for MimeRuntime {
    /// Creates a runtime containing the built-in providers and repository.
    ///
    /// # Returns
    ///
    /// A runtime initialized with the default MIME configuration and bundled
    /// repository.
    ///
    /// # Panics
    /// Panics if initialization of the bundled runtime unexpectedly fails.
    fn default() -> Self {
        Self::builtin().expect("built-in MIME runtime should initialize")
    }
}

#[cfg(test)]
mod file_policy_tests {
    use std::sync::Arc;

    use super::OwnedMimeInput;
    use super::managed_filename_winner;
    use crate::MimeRepository;

    #[test]
    fn test_file_backend_does_not_use_runtime_repository_filename_shortcuts() {
        let repository = Arc::new(MimeRepository::bundled().clone());
        let input = OwnedMimeInput::Bytes {
            content: Arc::from(b"content".as_slice()),
            extent: crate::InputExtent::Complete,
            filename: Some("document.pdf".to_owned()),
        };
        assert!(
            managed_filename_winner(
                "file",
                Some(&repository),
                &input,
                crate::MimeDetectionPolicy::PreferFilename,
            )
            .is_none()
        );
    }
}
