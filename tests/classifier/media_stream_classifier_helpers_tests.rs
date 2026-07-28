use std::io::Read;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
use qubit_local_files::LocalTempFile as TempFile;
use qubit_mime::{
    MediaStreamClassifier,
    MediaStreamClassifierBackend,
    MediaStreamType,
    MimeError,
    MimeResult,
};

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
        _reader: &mut dyn Read,
    ) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::AudioOnly)
    }
}

#[test]
fn test_media_stream_classifier_helpers_validate_public_file_entrypoint() {
    let backend = Backend;

    assert_eq!(
        MediaStreamType::VideoOnly,
        backend.classify_file(Path::new("Cargo.toml")).unwrap()
    );
    assert!(matches!(
        backend.classify_file(Path::new(".")),
        Err(MimeError::InvalidClassifierInput { .. }),
    ));
    assert!(
        backend
            .classify_file(Path::new("__missing_media_file__"))
            .is_err(),
    );
}

#[cfg(unix)]
#[test]
fn test_media_stream_classifier_helpers_report_unreadable_file() {
    let backend = Backend;
    let file = TempFile::new().expect("temporary file should be created");
    std::fs::set_permissions(
        file.path(),
        std::fs::Permissions::from_mode(0o000),
    )
    .expect("temporary file should become unreadable");

    let result = backend.classify_file(file.path());

    let _ = std::fs::set_permissions(
        file.path(),
        std::fs::Permissions::from_mode(0o600),
    );
    assert!(result.is_err());
}
