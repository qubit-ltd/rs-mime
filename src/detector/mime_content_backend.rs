use std::fmt::Debug;
use std::io::Read;

use qubit_io::std_io::ReadSeek;

use crate::MimeDetectorBackend;
use crate::MimeResult;

/// Amount of input a content backend must inspect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRequirement {
    /// Inspect only the leading prefix of the stream.
    Prefix(usize),
    /// Inspect the complete stream.
    Complete,
}

/// Object-safe backend contract for MIME content providers.
pub trait MimeContentBackend: Debug + Send + Sync {
    /// Declares the amount of input required by this backend.
    fn content_requirement(&self) -> ContentRequirement;

    /// Detects MIME candidates from bytes.
    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>>;

    /// Detects MIME candidates from a seekable reader.
    fn detect_reader(&self, reader: &mut dyn ReadSeek) -> MimeResult<Vec<String>> {
        let position = reader.stream_position()?;
        let mut bytes = Vec::new();
        match self.content_requirement() {
            ContentRequirement::Prefix(limit) => {
                reader.take(limit as u64).read_to_end(&mut bytes)?;
            }
            ContentRequirement::Complete => {
                reader.read_to_end(&mut bytes)?;
            }
        }
        reader.seek(std::io::SeekFrom::Start(position))?;
        self.detect_bytes(&bytes)
    }
}

/// Adapter exposing an existing detector backend through the content contract.
#[derive(Debug)]
pub struct MimeDetectorAdapter<B> {
    backend: B,
}

impl<B> MimeDetectorAdapter<B> {
    /// Wraps a detector backend.
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Returns the wrapped backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }
}

impl<B> MimeContentBackend for MimeDetectorAdapter<B>
where
    B: MimeDetectorBackend,
{
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(self.backend.max_test_bytes())
    }

    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
        self.backend.guess_from_content(bytes)
    }
}
// qubit-style: allow multiple-public-types
