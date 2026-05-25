use qubit_mime::{
    CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE,
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_MAX_BUFFER_SIZE,
    DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE,
    DEFAULT_MIME_DETECTOR,
    DEFAULT_MIME_MAX_BUFFER_SIZE,
    DEFAULT_PRECISE_DETECTION_PATTERNS,
    ENV_MEDIA_STREAM_MAX_STAGING_SIZE,
    ENV_MIME_DETECTOR_DEFAULT,
    ENV_MIME_MAX_BUFFER_SIZE,
};

#[test]
fn test_mime_configuration_constants_expose_expected_keys_and_defaults() {
    assert_eq!("QUBIT_MIME_DETECTOR_DEFAULT", ENV_MIME_DETECTOR_DEFAULT);
    assert_eq!("mime.detector.default", CONFIG_MIME_DETECTOR_DEFAULT);
    assert_eq!("QUBIT_MIME_MAX_BUFFER_SIZE", ENV_MIME_MAX_BUFFER_SIZE);
    assert_eq!("mime.max.buffer.size", CONFIG_MIME_MAX_BUFFER_SIZE);
    assert_eq!(
        "QUBIT_MEDIA_STREAM_MAX_STAGING_SIZE",
        ENV_MEDIA_STREAM_MAX_STAGING_SIZE
    );
    assert_eq!(
        "mime.media.stream.max.staging.size",
        CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE
    );
    assert_eq!("repository", DEFAULT_MIME_DETECTOR);
    assert_eq!(16 * 1024 * 1024, DEFAULT_MIME_MAX_BUFFER_SIZE);
    assert_eq!(64 * 1024 * 1024, DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE);
    assert!(
        DEFAULT_PRECISE_DETECTION_PATTERNS
            .split(',')
            .any(|pattern| pattern == "ogg")
    );
}
