use qubit_mime::{
    FileCommandMimeDetectorProvider,
    MimeConfig,
    MimeDetectorAvailability,
    MimeDetectorProvider,
};

#[test]
fn test_file_command_mime_detector_provider_metadata_and_availability() {
    let provider = FileCommandMimeDetectorProvider;
    let availability = provider.availability(&MimeConfig::default());

    assert_eq!("file", provider.id());
    assert_eq!(
        &["file-command", "file-command-mime-detector"],
        provider.aliases()
    );
    assert_eq!(10, provider.priority());
    assert!(matches!(
        availability,
        MimeDetectorAvailability::Available | MimeDetectorAvailability::Unavailable { .. }
    ));
}
