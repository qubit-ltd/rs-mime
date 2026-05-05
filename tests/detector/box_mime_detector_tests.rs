use std::path::Path;

use qubit_io::ReadSeek;
use qubit_mime::{
    BoxMimeDetector,
    MimeDetectionPolicy,
    MimeDetector,
    MimeResult,
};

#[derive(Debug)]
struct StaticDetector;

impl MimeDetector for StaticDetector {
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        filename
            .ends_with(".box")
            .then(|| "application/x-box".to_owned())
    }

    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        (content == b"box").then(|| "application/x-box-content".to_owned())
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
        Ok(self.detect(b"box", filename, policy))
    }

    fn detect_file(&self, file: &Path, policy: MimeDetectionPolicy) -> MimeResult<Option<String>> {
        Ok(self.detect(
            b"box",
            file.file_name().and_then(|name| name.to_str()),
            policy,
        ))
    }
}

#[test]
fn test_box_mime_detector_delegates_and_converts() {
    let wrapper = BoxMimeDetector::new(Box::new(StaticDetector));

    assert_eq!(
        Some("application/x-box".to_owned()),
        wrapper.detect_by_filename("a.box")
    );
    assert_eq!(
        Some("application/x-box-content".to_owned()),
        wrapper.detect_by_content(b"box")
    );

    let inner: Box<dyn MimeDetector> = wrapper.into();
    assert_eq!(
        Some("application/x-box".to_owned()),
        inner.detect_by_filename("a.box")
    );

    let round_trip = BoxMimeDetector::from(inner);
    assert_eq!(
        Some("application/x-box-content".to_owned()),
        round_trip.detect_by_content(b"box")
    );
}
