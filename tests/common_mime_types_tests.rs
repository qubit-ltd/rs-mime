use qubit_mime::CSV_MIME_TYPES;
use qubit_mime::EXCEL_MIME_TYPE;
use qubit_mime::EXCEL_MIME_TYPES;
use qubit_mime::JSON_MIME_TYPE;
use qubit_mime::PDF_MIME_TYPE;
use qubit_mime::PDF_MIME_TYPES;
use qubit_mime::PNG_MIME_TYPE;
use qubit_mime::POWERPOINT_MIME_TYPES;
use qubit_mime::WORD_MIME_TYPES;

#[test]
fn test_common_mime_type_constants_group_default_values() {
    assert_eq!("application/pdf", PDF_MIME_TYPE);
    assert_eq!(&[PDF_MIME_TYPE], PDF_MIME_TYPES);
    assert!(EXCEL_MIME_TYPES.contains(&EXCEL_MIME_TYPE));
    assert!(WORD_MIME_TYPES.contains(&"application/msword"));
    assert!(POWERPOINT_MIME_TYPES.contains(&"application/vnd.ms-powerpoint"));
    assert_eq!("application/json", JSON_MIME_TYPE);
    assert_eq!(&["text/csv"], CSV_MIME_TYPES);
    assert_eq!("image/png", PNG_MIME_TYPE);
}
