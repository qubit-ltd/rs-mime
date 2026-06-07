use qubit_mime::{
    MimeConfig,
    RepositoryMimeDetectorProvider,
    ServiceProvider,
};

#[test]
fn test_repository_mime_detector_provider_creates_filename_detector() {
    let provider = RepositoryMimeDetectorProvider;
    let descriptor = provider
        .descriptor()
        .expect("repository provider descriptor should be valid");
    let detector = provider
        .create_box(&MimeConfig::default())
        .expect("repository provider should create detector");

    assert_eq!("repository", descriptor.id().as_str());
    assert_eq!(
        vec!["repository-mime-detector"],
        descriptor.aliases_as_str()
    );
    assert_eq!(0, descriptor.priority());
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf"),
    );
}
