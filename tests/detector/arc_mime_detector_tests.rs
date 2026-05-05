use std::path::Path;
use std::sync::Arc;

use qubit_io::ReadSeek;
use qubit_mime::{
    ArcMimeDetector,
    MimeDetectionPolicy,
    MimeDetector,
    MimeResult,
};

#[derive(Debug)]
struct StaticDetector;

impl MimeDetector for StaticDetector {
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        filename
            .ends_with(".arc")
            .then(|| "application/x-arc".to_owned())
    }

    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        (content == b"arc").then(|| "application/x-arc-content".to_owned())
    }

    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        match policy {
            MimeDetectionPolicy::PreferFilename => {
                filename.and_then(|name| self.detect_by_filename(name))
            }
            MimeDetectionPolicy::VerifyContent => self.detect_by_content(content),
        }
    }

    fn detect_reader(
        &self,
        _reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(self.detect(b"arc", filename, policy))
    }

    fn detect_file(&self, file: &Path, policy: MimeDetectionPolicy) -> MimeResult<Option<String>> {
        Ok(self.detect(
            b"arc",
            file.file_name().and_then(|name| name.to_str()),
            policy,
        ))
    }
}

#[test]
fn test_arc_mime_detector_delegates_clones_and_converts() {
    let wrapper = ArcMimeDetector::new(Arc::new(StaticDetector));
    let cloned = wrapper.clone();

    assert_eq!(
        Some("application/x-arc".to_owned()),
        cloned.detect_by_filename("a.arc")
    );
    assert_eq!(
        Some("application/x-arc-content".to_owned()),
        wrapper.detect_by_content(b"arc")
    );

    let inner: Arc<dyn MimeDetector> = wrapper.into();
    assert_eq!(
        Some("application/x-arc".to_owned()),
        inner.detect_by_filename("a.arc")
    );

    let round_trip = ArcMimeDetector::from(inner);
    assert_eq!(
        Some("application/x-arc-content".to_owned()),
        round_trip.detect_by_content(b"arc")
    );
}
