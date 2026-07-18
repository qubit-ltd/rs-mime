// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # Qubit MIME
//!
//! MIME type detection based on filename glob rules, content magic rules, and
//! optional native command backends.
//!
//! The crate ships with two detector providers:
//!
//! - `repository`: an embedded detector that uses the bundled MIME repository.
//! - `file`: a detector that delegates content detection to the system `file
//!   --mime-type --brief` command and uses the repository for filename guesses.
//!
//! Applications may register self-described detector providers in the
//! process-wide [`MimeDetectorRegistry`] during startup. Downstream libraries
//! resolve either an explicit [`qubit_spi::ProviderSelection`] or the Registry
//! default without knowing the concrete implementation. Provider selection is
//! separate from detector creation with explicit or default [`MimeConfig`].
//!
//! # Examples
//!
//! Resolve the process default, then create a detector with default config:
//!
//! ```rust
//! use qubit_mime::{
//!     MimeDetector,
//!     MimeDetectorRegistry,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = MimeDetectorRegistry::global().resolve()?;
//! let detector = provider.create()?;
//! assert_eq!(
//!     Some("application/pdf".to_owned()),
//!     detector.detect_by_filename("document.pdf"),
//! );
//! # Ok(())
//! # }
//! ```
//!
//! Configure a preferred detector and an explicit fallback. This is useful when
//! `file` should be used on systems where it is available, while still allowing
//! deterministic repository detection on minimal CI images:
//!
//! ```rust
//! use qubit_config::Config;
//! use qubit_mime::{
//!     CONFIG_MIME_DETECTOR_DEFAULT,
//!     CONFIG_MIME_DETECTOR_FALLBACKS,
//!     MimeConfig,
//!     MimeDetector,
//!     MimeDetectorRegistry,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut source = Config::new();
//! source.set(CONFIG_MIME_DETECTOR_DEFAULT, "file")?;
//! source.set(CONFIG_MIME_DETECTOR_FALLBACKS, "repository")?;
//!
//! let config = MimeConfig::from_config(&source)?;
//! let registry = MimeDetectorRegistry::builtin();
//! let provider = registry.resolve_selected(config.mime_detector_selection())?;
//! let detector = provider.create_configured(&config)?;
//! assert_eq!(
//!     Some("image/png".to_owned()),
//!     detector.detect_by_filename("image.png"),
//! );
//! # Ok(())
//! # }
//! ```

pub mod classifier;
pub mod detector;
pub mod repository;

mod common_mime_types;
mod constants;
mod mime_config;
mod mime_error;
mod mime_result;

pub use classifier::{
    FfprobeCommandMediaStreamClassifier,
    FfprobeCommandMediaStreamClassifierProvider,
    FileBasedMediaStreamClassifier,
    MediaStreamClassifier,
    MediaStreamClassifierBackend,
    MediaStreamClassifierProvider,
    MediaStreamClassifierRegistry,
    MediaStreamClassifierSpec,
    MediaStreamType,
};
pub use common_mime_types::*;
pub use constants::*;
pub use detector::{
    DetectionSource,
    FileBasedMimeDetector,
    FileCommandMimeDetector,
    FileCommandMimeDetectorProvider,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorBackend,
    MimeDetectorCore,
    MimeDetectorProvider,
    MimeDetectorRegistry,
    MimeDetectorSpec,
    RepositoryMimeDetector,
    RepositoryMimeDetectorProvider,
    StreamBasedMimeDetector,
};
pub use mime_config::MimeConfig;
pub use mime_error::MimeError;
pub use mime_result::MimeResult;
pub use repository::{
    MagicValueType,
    MimeGlob,
    MimeMagic,
    MimeMagicMatcher,
    MimeRepository,
    MimeType,
    MimeTypeBuilder,
};
