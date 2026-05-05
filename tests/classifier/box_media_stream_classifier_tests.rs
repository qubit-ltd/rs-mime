use std::io::Read;
use std::path::Path;

use qubit_mime::{
    BoxMediaStreamClassifier,
    MediaStreamClassifier,
    MediaStreamType,
    MimeResult,
};

#[derive(Debug)]
struct StaticClassifier;

impl MediaStreamClassifier for StaticClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::VideoWithAudio)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::None)
    }
}

#[test]
fn test_box_media_stream_classifier_delegates_and_converts() {
    let wrapper = BoxMediaStreamClassifier::new(Box::new(StaticClassifier));

    assert_eq!(
        MediaStreamType::None,
        wrapper.classify_content(b"x").unwrap()
    );
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        wrapper
            .as_ref()
            .classify_file(Path::new("Cargo.toml"))
            .unwrap()
    );

    let inner: Box<dyn MediaStreamClassifier> = wrapper.into();
    assert_eq!(MediaStreamType::None, inner.classify_content(b"x").unwrap());

    let round_trip = BoxMediaStreamClassifier::from(inner);
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        round_trip.classify_file(Path::new("Cargo.toml")).unwrap()
    );
}
