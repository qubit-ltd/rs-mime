use std::path::Path;

use qubit_mime::DetectionSource;

#[test]
fn test_detection_source_carries_content_or_path() {
    match DetectionSource::Content(b"abc") {
        DetectionSource::Content(bytes) => assert_eq!(b"abc", bytes),
        _ => panic!("content source expected"),
    }

    match DetectionSource::Path(Path::new("Cargo.toml")) {
        DetectionSource::Path(path) => assert_eq!(Path::new("Cargo.toml"), path),
        _ => panic!("path source expected"),
    }

    assert!(matches!(DetectionSource::None, DetectionSource::None));
}
