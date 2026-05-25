use std::path::Path;

use qubit_mime::{
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorBackend,
    MimeDetectorCore,
    MimeResult,
};

#[derive(Debug)]
struct Backend {
    core: MimeDetectorCore,
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

#[test]
fn test_mime_detector_backend_blanket_impl_uses_policy_and_sources() {
    let backend = Backend::new();

    assert_eq!(Some("text/plain".to_owned()), backend.detect_by_filename("note.txt"));
    assert_eq!(
        Some("application/pdf".to_owned()),
        backend.detect_by_content(b"%PDF-1.7")
    );
    assert_eq!(
        Some("text/plain".to_owned()),
        backend.detect(b"%PDF-1.7", Some("note.txt"), MimeDetectionPolicy::PreferFilename),
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
