use qubit_mime::MagicValueType;
use qubit_mime::MimeError;
use qubit_mime::MimeMagic;
use qubit_mime::MimeMagicMatcher;

#[test]
fn test_mime_magic_rejects_invalid_priority_and_empty_matchers() {
    assert!(matches!(
        MimeMagic::new(101, vec![]),
        Err(MimeError::InvalidMagicPriority { priority: 101 })
    ));
    assert!(matches!(MimeMagic::new(50, vec![]), Err(MimeError::EmptyMagicMatchers)));
}

#[test]
fn test_mime_magic_matches_any_root_matcher_and_reports_width() {
    let pdf = MimeMagicMatcher::new(MagicValueType::String, 0, 0, b"%PDF".to_vec(), None, vec![]).unwrap();
    let png = MimeMagicMatcher::new(MagicValueType::String, 1, 2, b"PNG".to_vec(), None, vec![]).unwrap();
    let magic = MimeMagic::new(80, vec![pdf, png]).unwrap();

    assert_eq!(80, magic.priority());
    assert_eq!(2, magic.matchers().len());
    assert_eq!(5, magic.max_test_bytes());
    assert!(magic.matches(b"%PDF-1.7"));
    assert!(magic.matches(b"xPNG"));
    assert!(!magic.matches(b"plain text"));
}
