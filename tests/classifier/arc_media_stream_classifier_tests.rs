use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use qubit_mime::{
    ArcMediaStreamClassifier,
    MediaStreamClassifier,
    MediaStreamType,
    MimeResult,
};

#[derive(Debug)]
struct StaticClassifier;

impl MediaStreamClassifier for StaticClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::VideoOnly)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::AudioOnly)
    }
}

#[test]
fn test_arc_media_stream_classifier_delegates_and_converts() {
    let wrapper = ArcMediaStreamClassifier::new(Arc::new(StaticClassifier));
    let cloned = wrapper.clone();

    assert_eq!(
        MediaStreamType::AudioOnly,
        wrapper.classify_content(b"x").unwrap()
    );
    assert_eq!(
        MediaStreamType::VideoOnly,
        cloned.classify_file(Path::new("Cargo.toml")).unwrap()
    );

    let inner: Arc<dyn MediaStreamClassifier> = wrapper.into();
    assert_eq!(
        MediaStreamType::AudioOnly,
        inner.classify_content(b"x").unwrap()
    );

    let round_trip = ArcMediaStreamClassifier::from(inner);
    assert_eq!(
        MediaStreamType::VideoOnly,
        round_trip
            .as_ref()
            .classify_file(Path::new("Cargo.toml"))
            .unwrap()
    );
}
