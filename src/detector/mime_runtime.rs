use std::sync::Arc;

pub use super::mime_detector_context::MimeDetectorContext;
use super::owned_repository_mime_detector::OwnedRepositoryMimeDetector;
use crate::MimeConfig;
use crate::MimeDetector;
use crate::MimeRepository;
use crate::MimeResult;

/// Process-local runtime owning shared MIME configuration and repository.
#[derive(Debug, Clone)]
pub struct MimeRuntime {
    context: MimeDetectorContext,
}

impl MimeRuntime {
    /// Creates a runtime with explicit configuration and repository.
    pub fn new(config: MimeConfig, repository: Arc<MimeRepository>) -> Self {
        Self {
            context: MimeDetectorContext::new(Arc::new(config), repository),
        }
    }

    /// Creates a runtime using built-in configuration and repository.
    pub fn builtin() -> MimeResult<Self> {
        Ok(Self::new(MimeConfig::default(), MimeRepository::bundled_shared()))
    }

    /// Returns runtime context.
    pub fn context(&self) -> &MimeDetectorContext {
        &self.context
    }

    /// Creates the built-in repository detector.
    pub fn create_detector(&self) -> MimeResult<Arc<dyn MimeDetector>> {
        Ok(Arc::new(OwnedRepositoryMimeDetector::new(
            self.context.config().clone(),
            self.context.repository_arc(),
        )))
    }
}

impl Default for MimeRuntime {
    fn default() -> Self {
        Self::builtin().expect("built-in MIME runtime should initialize")
    }
}
