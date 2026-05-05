use qubit_mime::{
    MimeConfig,
    MimeDetectorProvider,
    RepositoryMimeDetectorProvider,
};

#[test]
fn test_repository_mime_detector_provider_creates_filename_detector() {
    let provider = RepositoryMimeDetectorProvider;
    let detector = provider.create(&MimeConfig::default()).unwrap();

    assert_eq!("repository", provider.id());
    assert_eq!(&["repository-mime-detector"], provider.aliases());
    assert_eq!(0, provider.priority());
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf"),
    );
}
