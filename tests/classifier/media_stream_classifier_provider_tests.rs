// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::AsyncMediaStreamClassifierProvider;
use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::MediaStreamClassifierProvider;
use qubit_mime::MimeConfig;
use qubit_mime::MimeExecutionServices;
use qubit_spi::ProviderMetadata;

fn assert_provider<T: MediaStreamClassifierProvider>() {}

#[test]
fn test_ffprobe_provider_implements_the_marker_contract() {
    assert_provider::<FfprobeCommandMediaStreamClassifierProvider>();
}

#[test]
fn test_ffprobe_provider_exposes_stable_registration_metadata() {
    let descriptor = FfprobeCommandMediaStreamClassifierProvider.descriptor();

    assert_eq!("ffprobe", descriptor.id().as_str());
    assert!(
        descriptor
            .aliases()
            .iter()
            .any(|alias| alias.as_str() == "ffprobe-command")
    );
    assert_eq!(10, descriptor.priority());
}

#[test]
fn test_ffprobe_provider_builds_async_backend_from_operation_services() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services = MimeExecutionServices::new(
        manager,
        group,
        std::sync::Arc::new(FixedMimeExecutionPort::new(1, 1)),
        1024,
    )
    .expect("valid execution services should be created");

    let backend = FfprobeCommandMediaStreamClassifierProvider
        .create_async_backend(&MimeConfig::default(), &services)
        .expect("async backend construction should not require spawning ffprobe");
    assert!(format!("{backend:?}").contains("FfprobeCommandMediaStreamClassifier"));
}
