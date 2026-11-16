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
#[derive(Clone)]
pub struct MimeRuntime {
    /// Configuration and repository shared by detectors created by this
    /// runtime.
    context: MimeDetectorContext,
    /// Optional services for managed operations.
    execution_services: Option<Arc<MimeExecutionServices>>,
    /// Selected async provider identity and detector backend.
    async_backend: Option<(String, Arc<dyn crate::AsyncMimeDetectorBackend>)>,
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
    /// this runtime use the same immutable settings.
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
    #[must_use]
    pub fn new(config: MimeConfig, repository: Arc<MimeRepository>) -> Self {
        Self {
            context: MimeDetectorContext::new(Arc::new(config), repository),
            execution_services: None,
            async_backend: None,
        }
    }

    /// Creates a runtime using the default configuration and bundled
    /// repository.
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

    /// Attaches shared operation and MIME execution services without starting
    /// workers.
    ///
    /// # Parameters
    /// - `services`: Shared operation manager, group, worker adapter, and
    ///   limits.
    ///
    /// # Returns
    /// This runtime configured for managed operations.
    #[must_use]
    pub fn with_execution_services(mut self, services: MimeExecutionServices) -> Self {
        let backend = OwnedRepositoryMimeDetector::new(self.context.config().clone(), self.context.repository_arc());
        self.async_backend = Some(("repository".to_owned(), Arc::new(backend)));
        self.execution_services = Some(Arc::new(services));
        self
    }

    /// Selects an explicitly async-capable provider for managed submissions.
    /// The synchronous detector facade continues to use the repository-backed
    /// implementation retained by this runtime.
    ///
    /// # Parameters
    /// - `registry`: Registry containing async provider factories.
    /// - `selection`: Provider selection to resolve.
    ///
    /// # Returns
    /// This runtime with the selected async backend.
    ///
    /// # Errors
    /// Returns an error when execution services are absent or resolution fails.
    pub fn with_async_provider_selection(
        mut self,
        registry: &MimeDetectorRegistry,
        selection: &ProviderSelection,
    ) -> MimeResult<Self> {
        let services = self
            .execution_services
            .as_ref()
            .ok_or(crate::MimeError::ExecutionServicesRequired)?;
        self.async_backend = Some(registry.create_async_backend(selection, self.context.config(), services)?);
        Ok(self)
    }

    /// Selects async detector and classifier providers for managed operation
    /// submission. The selected classifier is attached to detector backends
    /// that implement async MIME refinement.
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
    /// Returns an error when services are absent or either selection fails.
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
        self.async_backend = Some(detector_registry.create_async_backend_with_classifier(
            detector_selection,
            self.context.config(),
            services,
            Some(classifier),
        )?);
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
    pub fn try_submit_detection_in(
        &self,
        parent: &OperationScope,
        input: OwnedMimeInput,
        options: MimeOperationOptions,
    ) -> Result<AsyncMimeOperation, MimeSubmitFailure> {
        self.submit_async(Some(parent), input, options)
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
        let _submission = services.submission_guard();
        let can_submit = if parent.is_some() {
            services.accepts_children()
        } else {
            services.is_prepared()
        };
        if !can_submit {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::ExecutionServicesRequired,
                input,
            });
        }
        let Some((backend_id, backend)) = self.async_backend.as_ref().cloned() else {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::AsyncUnsupported,
                input,
            });
        };
        let max_buffer_size = self.context.config().max_buffer_size();
        let permit = match mime_submission::preflight(
            &input,
            &options,
            backend.content_requirement(),
            backend.random_access_capability(),
            max_buffer_size,
            &services,
        ) {
            Ok(permit) => permit,
            Err(reason) => return Err(MimeSubmitFailure { reason, input }),
        };
        let tree_limit = options
            .max_read_bytes
            .unwrap_or(services.max_read_bytes)
            .min(services.max_read_bytes);
        let read_tree = parent.map_or_else(
            || services.new_read_tree(tree_limit),
            |parent| services.read_tree_for_parent(parent.id(), tree_limit, tree_limit),
        );
        let cell = mime_submission::cell(input);
        let body_cell = Arc::clone(&cell);
        let body_services = Arc::clone(&services);
        let body_options = options.clone();
        let body_tree = Arc::clone(&read_tree);
        let body_backend_id = backend_id.clone();
        let body_max_buffer_size = max_buffer_size;
        let body = move |scope: OperationScope| {
            mime_submission::detect(
                scope,
                mime_submission::take(&body_cell),
                body_services,
                backend,
                body_backend_id,
                body_max_buffer_size,
                body_options,
                Some(permit),
                body_tree,
            )
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

    /// Submits a synchronous child operation using the parent's manager view.
    ///
    /// # Parameters
    /// - `parent`: Parent operation that owns this child operation.
    /// - `input`: Owned input returned unchanged when submission is rejected.
    /// - `options`: Detection policy, resource limits, and operation context.
    ///
    /// # Returns
    /// An operation handle, or the rejected input and its reason.
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
        let _submission = services.submission_guard();
        if !services.accepts_children() {
            return Err(MimeSubmitFailure {
                reason: MimeSubmitRejection::ExecutionServicesRequired,
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
        ) {
            Ok(permit) => permit,
            Err(reason) => return Err(MimeSubmitFailure { reason, input }),
        };
        let tree_limit = options
            .max_read_bytes
            .unwrap_or(services.max_read_bytes)
            .min(services.max_read_bytes);
        let read_tree = services.read_tree_for_parent(parent.id(), tree_limit, tree_limit);
        let cell = mime_submission::cell(input);
        let body_cell = Arc::clone(&cell);
        let body_services = Arc::clone(&services);
        let body_tree = Arc::clone(&read_tree);
        let body = move |scope: OperationScope| {
            body_services.bind_read_tree(scope.id(), &body_tree);
            let input = mime_submission::take(&body_cell);
            let result = match input {
                OwnedMimeInput::Bytes {
                    content,
                    extent,
                    filename,
                } => {
                    let mime_type = detector.detect(&content, filename.as_deref(), options.policy)?;
                    let inspected_extent = match extent {
                        super::mime_input::InputExtent::Prefix => {
                            super::mime_input::InspectedExtent::Prefix(content.len())
                        }
                        super::mime_input::InputExtent::Complete => super::mime_input::InspectedExtent::Complete,
                    };
                    Ok(crate::MimeDetectionReport {
                        mime_type,
                        backend_id: "repository".into(),
                        inspected_extent,
                        budget: Default::default(),
                    })
                }
                OwnedMimeInput::Provider { .. } => Err(crate::MimeError::AsyncUnsupported),
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
}

impl Default for MimeRuntime {
    /// Creates a runtime containing the built-in providers and repository.
    ///
    /// # Returns
    ///
    /// A runtime initialized with the default MIME configuration and bundled
    /// repository.
    fn default() -> Self {
        Self::builtin().expect("built-in MIME runtime should initialize")
    }
}
