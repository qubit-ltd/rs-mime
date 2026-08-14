use std::path::Path;

use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorBackend;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeError;
use qubit_mime::MimeResult;

#[derive(Debug)]
struct Backend {
    core: MimeDetectorCore,
}

#[derive(Debug)]
struct FailingBackend {
    core: MimeDetectorCore,
}

impl FailingBackend {
    /// Creates a backend that fails every content inspection.
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::from_mime_config(MimeConfig::default()),
        }
    }
}

impl Backend {
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::from_mime_config(MimeConfig::default()),
        }
    }
}

impl MimeDetectorBackend for Backend {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    fn max_test_bytes(&self) -> usize {
        16
    }

    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        filename
            .ends_with(".txt")
            .then(|| "text/plain".to_owned())
            .into_iter()
            .collect()
    }

    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(content
            .starts_with(b"%PDF")
            .then(|| "application/pdf".to_owned())
            .into_iter()
            .collect())
    }
}

impl MimeDetectorBackend for FailingBackend {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    fn max_test_bytes(&self) -> usize {
        16
    }

    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }

    fn guess_from_content(&self, _content: &[u8]) -> MimeResult<Vec<String>> {
        Err(MimeError::detector_backend("failing", "forced failure"))
    }
}

#[test]
fn test_mime_detector_backend_blanket_impl_uses_policy_and_sources() {
    let backend = Backend::new();

    assert_eq!(
        Some("text/plain".to_owned()),
        backend
            .detect_by_filename("note.txt")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        backend
            .detect_by_content(b"%PDF-1.7")
            .expect("content detection should succeed")
    );
    assert_eq!(
        Some("text/plain".to_owned()),
        backend
            .detect(
                b"%PDF-1.7",
                Some("note.txt"),
                MimeDetectionPolicy::PreferFilename
            )
            .expect("combined detection should succeed"),
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        backend
            .detect_file(
                Path::new("tests/fixtures/real_files/test.pdf"),
                MimeDetectionPolicy::VerifyContent
            )
            .unwrap(),
    );
}

#[test]
fn test_mime_detector_backend_propagates_content_errors() {
    let backend = FailingBackend::new();

    let content_error = backend
        .detect_by_content(b"content")
        .expect_err("content detection must retain backend failures");
    let combined_error = backend
        .detect(
            b"content",
            Some("content.bin"),
            MimeDetectionPolicy::VerifyContent,
        )
        .expect_err("combined detection must retain backend failures");

    assert!(matches!(
        content_error,
        MimeError::DetectorBackend { ref backend, .. } if backend == "failing"
    ));
    assert!(matches!(
        combined_error,
        MimeError::DetectorBackend { ref backend, .. } if backend == "failing"
    ));
}
