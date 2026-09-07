use std::sync::Arc;

use super::repository_mime_detector::RepositoryMimeDetector;
use crate::MimeConfig;
use crate::MimeDetector;
use crate::MimeRepository;
use crate::MimeResult;

/// Immutable context shared by detector adapters.
#[derive(Debug, Clone)]
pub struct MimeDetectorContext {
    config: Arc<MimeConfig>,
    repository: Arc<MimeRepository>,
}

impl MimeDetectorContext {
    /// Creates a context from configuration and repository.
    pub fn new(config: Arc<MimeConfig>, repository: Arc<MimeRepository>) -> Self {
        Self { config, repository }
    }
    /// Returns detector configuration.
    pub fn config(&self) -> &MimeConfig {
        &self.config
    }
    /// Returns MIME repository.
    pub fn repository(&self) -> &MimeRepository {
        &self.repository
    }
}

/// Resolves filename/content candidates using repository rules.
#[derive(Debug, Default, Clone, Copy)]
pub struct MimeDecisionEngine;

impl MimeDecisionEngine {
    /// Creates a decision engine.
    pub const fn new() -> Self {
        Self
    }
}

/// Process-local runtime owning shared MIME configuration and repository.
#[derive(Debug, Clone)]
pub struct MimeRuntime {
    context: MimeDetectorContext,
}

impl MimeRuntime {
    /// Creates a runtime with explicit configuration and repository.
    pub fn new(config: MimeConfig, repository: MimeRepository) -> Self {
        Self {
            context: MimeDetectorContext::new(Arc::new(config), Arc::new(repository)),
        }
    }

    /// Creates a runtime using built-in configuration and repository.
    pub fn builtin() -> MimeResult<Self> {
        let detector = RepositoryMimeDetector::new()?;
        Ok(Self::new(MimeConfig::default(), detector.repository().clone()))
    }

    /// Returns runtime context.
    pub fn context(&self) -> &MimeDetectorContext {
        &self.context
    }

    /// Creates the built-in repository detector.
    pub fn create_detector(&self) -> MimeResult<Arc<dyn MimeDetector>> {
        let repository = Box::leak(Box::new(self.context.repository().clone()));
        Ok(Arc::new(RepositoryMimeDetector::with_repository_and_config(
            repository,
            self.context.config().clone(),
        )))
    }
}

impl Default for MimeRuntime {
    fn default() -> Self {
        Self::builtin().expect("built-in MIME runtime should initialize")
    }
}
// qubit-style: allow multiple-public-types
