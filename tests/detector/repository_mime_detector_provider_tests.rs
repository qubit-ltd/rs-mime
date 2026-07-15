use qubit_mime::{
    MimeConfig,
    RepositoryMimeDetectorProvider,
    repository_mime_detector_descriptor,
};
use qubit_spi::ServiceProvider;

#[test]
fn test_repository_mime_detector_provider_creates_filename_detector() {
    let provider = RepositoryMimeDetectorProvider;
    let descriptor = repository_mime_detector_descriptor();
    let detector = provider
        .create(&MimeConfig::default())
        .expect("repository provider should create detector");

    assert_eq!("repository", descriptor.id().as_str());
    assert_eq!(
        vec!["repository-mime-detector"],
        descriptor
            .aliases()
            .iter()
            .map(|alias| alias.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(0, descriptor.priority());
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf"),
    );
}
