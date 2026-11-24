// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! MIME detector implementations.

/// Asynchronous MIME backend contracts and random-access capability.
pub mod async_mime_detector_backend;
pub mod content_requirement;
pub(crate) mod detection_source;
pub(crate) mod file_based_mime_detector;
pub(crate) mod file_command_mime_detector;
pub(crate) mod file_command_mime_detector_provider;
mod internal;
/// Metered same-object reads for managed provider backends.
pub mod metered_read_session;
/// Owned results and byte-accounting summaries produced by managed detection.
pub mod mime_detection_report;
pub mod mime_execution_services;
pub mod mime_input;
pub mod mime_operation;
pub mod mime_operation_options;
pub(crate) use internal::mime_budget::OwnedMemoryPermit;
pub(crate) use internal::mime_budget::ReadPermit;
pub(crate) use internal::mime_budget::ReadTreeContext;
pub(crate) use internal::mime_budget::StagingBudget;
pub(crate) use internal::mime_budget::StagingPermit;
/// Content backend contracts and adapters.
pub mod mime_content_backend;
pub(crate) mod mime_detection_policy;
pub(crate) mod mime_detector;
pub mod mime_detector_adapter;
pub(crate) mod mime_detector_backend;
pub(crate) mod mime_detector_context;
pub(crate) mod mime_detector_core;
#[cfg(feature = "inventory")]
pub mod mime_detector_inventory;
pub(crate) mod mime_detector_provider;
pub(crate) mod mime_detector_registry;
pub(crate) mod mime_detector_spec;
/// Runtime context and detector lifecycle helpers.
pub mod mime_runtime;
pub(crate) mod owned_repository_mime_detector;
pub(crate) mod repository_mime_detector;
pub(crate) mod repository_mime_detector_provider;
pub(crate) mod stream_based_mime_detector;

pub use async_mime_detector_backend::AsyncMimeDetectorBackend;
pub use async_mime_detector_backend::MimeFuture;
pub use async_mime_detector_backend::RandomAccessCapability;
pub use content_requirement::ContentRequirement;
pub use detection_source::DetectionSource;
pub use file_based_mime_detector::FileBasedMimeDetector;
pub use file_command_mime_detector::FileCommandMimeDetector;
pub use file_command_mime_detector_provider::FileCommandMimeDetectorProvider;
pub(crate) use internal::async_mime_refinement::stream_type_from_output;
pub use mime_content_backend::MimeContentBackend;
pub use mime_content_backend::MimeDetectorAdapter;
pub use mime_detection_policy::MimeDetectionPolicy;
pub use mime_detection_report::MimeBudgetSummary;
pub use mime_detection_report::MimeCandidate;
pub use mime_detection_report::MimeCandidates;
pub use mime_detection_report::MimeDetectionReport;
pub use mime_detector::MimeDetector;
pub use mime_detector_backend::MimeDetectorBackend;
pub use mime_detector_context::MimeDetectorContext;
pub use mime_detector_core::MimeDetectorCore;
pub use mime_detector_provider::AsyncMimeDetectorProvider;
pub use mime_detector_provider::MimeDetectorProvider;
pub use mime_detector_registry::MimeDetectorRegistry;
pub use mime_detector_spec::MimeDetectorSpec;
pub use mime_execution_services::MimeExecutionDispatchFailure;
pub use mime_execution_services::MimeExecutionDispatchFailureKind;
pub use mime_execution_services::MimeExecutionPort;
pub use mime_execution_services::MimeExecutionServices;
pub use mime_execution_services::MimeServiceLifecycle;
pub use mime_input::BorrowedMimeInput;
pub use mime_input::InputExtent;
pub use mime_input::InspectedExtent;
pub use mime_input::OwnedMimeInput;
pub use mime_operation::AsyncMimeOperation;
pub use mime_operation::MimeOperation;
pub use mime_operation::MimeSubmitFailure;
pub use mime_operation::MimeSubmitRejection;
pub use mime_operation_options::MimeOperationOptions;
pub use mime_runtime::MimeRuntime;
pub use repository_mime_detector::RepositoryMimeDetector;
pub use repository_mime_detector_provider::RepositoryMimeDetectorProvider;
pub use stream_based_mime_detector::StreamBasedMimeDetector;
