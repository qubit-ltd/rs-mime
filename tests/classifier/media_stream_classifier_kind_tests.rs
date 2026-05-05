use qubit_mime::{
    BoxMediaStreamClassifier,
    MediaStreamClassifier,
    MimeConfig,
};

#[test]
fn test_media_stream_classifier_kind_is_observable_through_public_selectors() {
    assert!(BoxMediaStreamClassifier::from_name("ffprobe").is_some());
    assert!(BoxMediaStreamClassifier::from_name("FFPROBE-COMMAND").is_some());
    assert!(
        BoxMediaStreamClassifier::from_name("ffprobe-command-media-stream-classifier").is_some()
    );
    assert!(BoxMediaStreamClassifier::from_name("unknown").is_none());

    let fallback = BoxMediaStreamClassifier::from_config(&MimeConfig::default());
    let _ = fallback.classify_content(b"not a media payload");
}
