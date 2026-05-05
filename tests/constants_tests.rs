use qubit_mime::{
    CONFIG_MIME_DETECTOR_DEFAULT,
    DEFAULT_MIME_DETECTOR,
    DEFAULT_PRECISE_DETECTION_PATTERNS,
    ENV_MIME_DETECTOR_DEFAULT,
};

#[test]
fn test_mime_configuration_constants_expose_expected_keys_and_defaults() {
    assert_eq!("QUBIT_MIME_DETECTOR_DEFAULT", ENV_MIME_DETECTOR_DEFAULT);
    assert_eq!("mime.detector.default", CONFIG_MIME_DETECTOR_DEFAULT);
    assert_eq!("repository", DEFAULT_MIME_DETECTOR);
    assert!(
        DEFAULT_PRECISE_DETECTION_PATTERNS
            .split(',')
            .any(|pattern| pattern == "ogg")
    );
}
