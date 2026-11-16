// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::OperationScope;
use std::{fmt, sync::Arc};

use super::internal::mime_submission;
pub use super::mime_detector_context::MimeDetectorContext;
use super::owned_repository_mime_detector::OwnedRepositoryMimeDetector;
use super::{
    mime_execution_services::MimeExecutionServices,
    mime_input::OwnedMimeInput,
    mime_operation::{AsyncMimeOperation, MimeOperation, MimeSubmitFailure, MimeSubmitRejection},
    mime_operation_options::MimeOperationOptions,
};
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
    execution_services: Option<Arc<MimeExecutionServices>>,
}

impl fmt::Debug for MimeRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MimeRuntime")
            .field("context", &self.context)
            .field("has_execution_services", &self.execution_services.is_some())
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
        Ok(Self::new(
            MimeConfig::default(),
            MimeRepository::bundled_shared(),
        ))
    }

    /// Attaches shared operation and MIME execution services without starting workers.
    #[must_use]
    pub fn with_execution_services(mut self, services: MimeExecutionServices) -> Self {
        self.execution_services = Some(Arc::new(services));
        self
    }

    /// Submits detection to the MIME root group.
    pub fn try_submit_detection(
        &self,
        input: OwnedMimeInput,
        options: MimeOperationOptions,
    ) -> Result<AsyncMimeOperation, MimeSubmitFailure> {
        self.submit_async(None, input, options)
    }

    /// Submits detection as a child of the given operation.
    pub fn try_submit_detection_in(
        &self,
        parent: &OperationScope,
        input: OwnedMimeInput,
        options: MimeOperationOptions,
    ) -> Result<AsyncMimeOperation, MimeSubmitFailure> {
        self.submit_async(Some(parent), input, options)
    }

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
        let detector = match self.create_detector() {
            Ok(detector) => detector,
            Err(_) => {
                return Err(MimeSubmitFailure {
                    reason: MimeSubmitRejection::InvalidOptions(
                        "detector construction failed".into(),
                    ),
                    input,
                });
            }
        };
        let permit = match mime_submission::preflight(&input, &options, &*detector, &services) {
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
        let body = move |scope: OperationScope| {
            mime_submission::detect(
                scope,
                mime_submission::take(&body_cell),
                body_services,
                detector,
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
            services
                .async_root_group
                .try_submit(options.operation, body)
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
                    reason: MimeSubmitRejection::InvalidOptions(
                        "detector construction failed".into(),
                    ),
                    input,
                });
            }
        };
        let permit = match mime_submission::preflight(&input, &options, &*detector, &services) {
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
                    let mime_type =
                        detector.detect(&content, filename.as_deref(), options.policy)?;
                    let inspected_extent = match extent {
                        super::mime_input::InputExtent::Prefix => {
                            super::mime_input::InspectedExtent::Prefix(content.len())
                        }
                        super::mime_input::InputExtent::Complete => {
                            super::mime_input::InspectedExtent::Complete
                        }
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
