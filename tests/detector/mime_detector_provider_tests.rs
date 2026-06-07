use std::path::Path;

use qubit_io::ReadSeek;
use qubit_mime::{
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorAvailability,
    MimeDetectorSpec,
    MimeResult,
    ProviderCreateError,
    ProviderDescriptor,
    ProviderRegistryError,
    ServiceProvider,
};

#[derive(Debug)]
struct StaticDetector;

impl MimeDetector for StaticDetector {
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        filename
            .ends_with(".static")
            .then(|| "application/x-static".to_owned())
    }

    fn detect_by_content(&self, _content: &[u8]) -> Option<String> {
        None
    }

    fn detect(
        &self,
        _content: &[u8],
        filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> Option<String> {
        filename.and_then(|name| self.detect_by_filename(name))
    }

    fn detect_reader(
        &self,
        _reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(self.detect(&[], filename, policy))
    }

    fn detect_file(
        &self,
        file: &Path,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(self.detect(
            &[],
            file.file_name().and_then(|name| name.to_str()),
            policy,
        ))
    }
}

#[derive(Debug)]
struct StaticProvider;

impl ServiceProvider<MimeDetectorSpec> for StaticProvider {
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        ProviderDescriptor::new("static")
    }

    fn create_box(
        &self,
        _config: &MimeConfig,
    ) -> Result<Box<dyn MimeDetector>, ProviderCreateError> {
        Ok(Box::new(StaticDetector))
    }
}

#[test]
fn test_mime_detector_provider_defaults_and_factory() {
    let provider = StaticProvider;
    let descriptor = provider
        .descriptor()
        .expect("static provider descriptor should be valid");
    let detector = provider
        .create_box(&MimeConfig::default())
        .expect("static provider should create detector");

    assert_eq!("static", descriptor.id().as_str());
    assert!(descriptor.aliases().is_empty());
    assert_eq!(0, descriptor.priority());
    assert_eq!(
        MimeDetectorAvailability::Available,
        provider.availability(&MimeConfig::default())
    );
    assert_eq!(
        Some("application/x-static".to_owned()),
        detector.detect_by_filename("sample.static"),
    );
}
