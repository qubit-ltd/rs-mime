use qubit_mime::MimeError;

#[test]
fn test_mime_error_display_includes_variant_context() {
    let duplicate = MimeError::DuplicateDetectorName {
        name: "repository".to_owned(),
    };
    let backend = MimeError::detector_backend("file", "command failed");

    assert_eq!(
        "duplicate MIME detector name or alias: repository",
        duplicate.to_string()
    );
    assert_eq!(
        "MIME detector backend 'file' failed: command failed",
        backend.to_string(),
    );
}
