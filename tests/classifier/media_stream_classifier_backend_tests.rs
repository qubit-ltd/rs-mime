use std::io::Read;
use std::path::Path;

use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamClassifierBackend;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeError;
use qubit_mime::MimeResult;

#[derive(Debug)]
struct Backend;

impl MediaStreamClassifierBackend for Backend {
    fn classify_by_local_file(
        &self,
        _file: &Path,
    ) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::VideoOnly)
    }

    fn classify_by_content(
        &self,
        reader: &mut dyn Read,
    ) -> MimeResult<MediaStreamType> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(if bytes == b"audio" {
            MediaStreamType::AudioOnly
        } else {
            MediaStreamType::None
        })
    }
}

#[test]
fn test_media_stream_classifier_backend_blanket_impl_validates_files() {
    let backend = Backend;

    assert_eq!(
        MediaStreamType::VideoOnly,
        backend.classify_file(Path::new("Cargo.toml")).unwrap()
    );
    assert_eq!(
        MediaStreamType::AudioOnly,
        backend.classify_content(b"audio").unwrap()
    );
    assert!(matches!(
        backend.classify_file(Path::new(".")),
        Err(MimeError::InvalidClassifierInput { .. })
    ));
}
