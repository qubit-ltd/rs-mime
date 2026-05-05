use qubit_mime::MimeDetectorAvailability;

#[test]
fn test_mime_detector_availability_reports_state_and_reason() {
    assert!(MimeDetectorAvailability::Available.is_available());

    let unavailable = MimeDetectorAvailability::Unavailable {
        reason: "missing command".to_owned(),
    };

    assert!(!unavailable.is_available());
    assert!(format!("{unavailable:?}").contains("missing command"));
}
