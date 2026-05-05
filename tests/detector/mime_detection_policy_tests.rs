use qubit_mime::MimeDetectionPolicy;

#[test]
fn test_mime_detection_policy_is_copyable_and_comparable() {
    let policy = MimeDetectionPolicy::PreferFilename;
    let copied = policy;

    assert_eq!(MimeDetectionPolicy::PreferFilename, copied);
    assert_ne!(MimeDetectionPolicy::VerifyContent, copied);
    assert_eq!("PreferFilename", format!("{policy:?}"));
}
