use qubit_mime::{
    MimeError,
    ProviderCreateError,
    ProviderFailure,
    ProviderName,
    ProviderRegistryError,
};

#[test]
fn test_mime_error_display_includes_variant_context() {
    let duplicate = MimeError::DuplicateDetectorName {
        name: "repository".to_owned(),
    };
    let backend = MimeError::detector_backend("file", "command failed");

    assert_eq!(
        "duplicate MIME detector name or alias: repository",
        duplicate.to_string()
    );
    assert_eq!(
        "MIME detector backend 'file' failed: command failed",
        backend.to_string(),
    );
}

#[test]
fn test_provider_registry_errors_convert_to_detector_errors() {
    assert!(matches!(
        MimeError::from(ProviderRegistryError::EmptyProviderName),
        MimeError::EmptyDetectorName
    ));
    assert!(matches!(
        MimeError::from(ProviderRegistryError::InvalidProviderName {
            name: "bad name".to_owned(),
            reason: "contains whitespace".to_owned(),
        }),
        MimeError::InvalidDetectorName { ref name, ref reason }
            if name == "bad name" && reason == "contains whitespace"
    ));
    assert!(matches!(
        MimeError::from(ProviderRegistryError::DuplicateProviderName {
            name: provider_name("duplicate"),
        }),
        MimeError::DuplicateDetectorName { ref name } if name == "duplicate"
    ));
    assert!(matches!(
        MimeError::from(ProviderRegistryError::DuplicateProviderCandidate {
            name: provider_name("duplicate"),
        }),
        MimeError::DuplicateDetectorName { ref name } if name == "duplicate"
    ));
    assert!(matches!(
        MimeError::from(ProviderRegistryError::UnknownProvider {
            name: provider_name("missing"),
        }),
        MimeError::UnknownDetector { ref name } if name == "missing"
    ));
    assert!(matches!(
        MimeError::from(ProviderRegistryError::ProviderUnavailable {
            name: provider_name("file"),
            source: ProviderCreateError::unavailable("missing command"),
        }),
        MimeError::DetectorUnavailable { ref name, ref reason }
            if name == "file" && reason == "missing command"
    ));
    assert!(matches!(
        MimeError::from(ProviderRegistryError::ProviderCreate {
            name: provider_name("file"),
            source: ProviderCreateError::failed("command failed"),
        }),
        MimeError::DetectorBackend { ref backend, ref reason }
            if backend == "file" && reason == "command failed"
    ));
    assert!(matches!(
        MimeError::from(ProviderRegistryError::NoAvailableProvider {
            failures: vec![
                ProviderFailure::unknown("missing").expect("failure name should be valid"),
                ProviderFailure::create_failed("file", "command failed")
                    .expect("failure name should be valid"),
            ],
        }),
        MimeError::NoAvailableDetector { ref reason }
            if reason.contains("missing") && reason.contains("command failed")
    ));
    assert!(matches!(
        MimeError::from(ProviderRegistryError::EmptyRegistry),
        MimeError::NoAvailableDetector { ref reason }
            if reason == "detector registry is empty"
    ));
}

/// Creates a validated provider name for error conversion tests.
fn provider_name(name: &str) -> ProviderName {
    ProviderName::new(name).expect("provider name should be valid")
}
