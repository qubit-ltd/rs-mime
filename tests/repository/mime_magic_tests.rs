use qubit_mime::{
    MagicValueType,
    MimeMagic,
    MimeMagicMatcher,
};

#[test]
fn test_mime_magic_matches_any_root_matcher_and_reports_width() {
    let pdf = MimeMagicMatcher::new(MagicValueType::String, 0, 0, b"%PDF".to_vec(), None, vec![])
        .unwrap();
    let png =
        MimeMagicMatcher::new(MagicValueType::String, 1, 2, b"PNG".to_vec(), None, vec![]).unwrap();
    let magic = MimeMagic::new(80, vec![pdf, png]);

    assert_eq!(80, magic.priority());
    assert_eq!(2, magic.matchers().len());
    assert_eq!(5, magic.max_test_bytes());
    assert!(magic.matches(b"%PDF-1.7"));
    assert!(magic.matches(b"xPNG"));
    assert!(!magic.matches(b"plain text"));
}
