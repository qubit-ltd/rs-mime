use qubit_mime::{
    BoxMimeDetector,
    MimeConfig,
    MimeDetector,
};

#[test]
fn test_mime_detector_kind_is_observable_through_public_selectors() {
    let repository = BoxMimeDetector::from_name("repository").unwrap();
    let repository_alias = BoxMimeDetector::from_name("repository-mime-detector").unwrap();

    assert_eq!(
        Some("application/pdf".to_owned()),
        repository.detect_by_filename("document.pdf"),
    );
    assert_eq!(
        Some("application/json".to_owned()),
        repository_alias.detect_by_filename("payload.json"),
    );
    assert!(BoxMimeDetector::from_name("unknown").is_err());

    let configured = BoxMimeDetector::from_config(&MimeConfig::default()).unwrap();
    assert_eq!(
        Some("image/png".to_owned()),
        configured.detect_by_filename("image.png"),
    );
}
