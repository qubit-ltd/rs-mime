use std::path::Path;

use qubit_mime::FileBasedMediaStreamClassifier;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeResult;

#[derive(Debug)]
struct FileOnlyClassifier;

impl FileBasedMediaStreamClassifier for FileOnlyClassifier {
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        Ok(if file.is_file() {
            MediaStreamType::VideoWithAudio
        } else {
            MediaStreamType::None
        })
    }
}

#[test]
fn test_file_based_media_stream_classifier_stages_content_to_temp_file() {
    let classifier = FileOnlyClassifier;

    assert_eq!(
        MediaStreamType::VideoWithAudio,
        classifier.classify_content(b"media").unwrap()
    );
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        classifier.classify_file(Path::new("Cargo.toml")).unwrap()
    );
}
