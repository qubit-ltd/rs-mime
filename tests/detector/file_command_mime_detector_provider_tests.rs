use qubit_mime::{
    FileCommandMimeDetectorProvider,
    MimeConfig,
    MimeDetectorAvailability,
    ServiceProvider,
};

#[test]
fn test_file_command_mime_detector_provider_metadata_and_availability() {
    let provider = FileCommandMimeDetectorProvider;
    let descriptor = provider.descriptor().expect("file provider descriptor should be valid");
    let availability = provider.availability(&MimeConfig::default());

    assert_eq!("file", descriptor.id().as_str());
    assert_eq!(
        vec!["file-command", "file-command-mime-detector"],
        descriptor.aliases_as_str()
    );
    assert_eq!(10, descriptor.priority());
    assert!(matches!(
        availability,
        MimeDetectorAvailability::Available | MimeDetectorAvailability::Unavailable { .. }
    ));
}
