// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "inventory")]

use std::sync::Arc;

use qubit_mime::classifier::media_stream_classifier_inventory::Entry as MediaStreamClassifierInventoryEntry;
use qubit_mime::detector::mime_detector_inventory::Entry as MimeDetectorInventoryEntry;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamClassifierRegistry;
use qubit_mime::MediaStreamClassifierSpec;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeDetectorSpec;
use qubit_mime::MimeError;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ProviderSelection;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::provider_descriptor;
use qubit_spi::submit_sync_provider;

struct ExternalDetector;
impl ProviderMetadata for ExternalDetector {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("external-detector")
    }
}
impl ServiceProvider<MimeDetectorSpec> for ExternalDetector {
    fn create_configured(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        unimplemented!("discovery does not create the detector")
    }
}

struct ExternalClassifier;
impl ProviderMetadata for ExternalClassifier {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("external-classifier")
    }
}
impl ServiceProvider<MediaStreamClassifierSpec> for ExternalClassifier {
    fn create_configured(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn MediaStreamClassifier>, ProviderFailure<MimeError>> {
        unimplemented!("discovery does not create the classifier")
    }
}

submit_sync_provider! {
    inventory_entry = MimeDetectorInventoryEntry;
    spec = MimeDetectorSpec;
    provider = ExternalDetector;
}
submit_sync_provider! {
    inventory_entry = MediaStreamClassifierInventoryEntry;
    spec = MediaStreamClassifierSpec;
    provider = ExternalClassifier;
}

#[test]
fn test_builtin_discovers_external_providers_without_changing_defaults() {
    let detectors = MimeDetectorRegistry::builtin();
    let classifiers = MediaStreamClassifierRegistry::builtin();
    assert!(
        detectors
            .provider_ids()
            .iter()
            .any(|id| id.as_str() == "external-detector")
    );
    assert!(
        classifiers
            .provider_ids()
            .iter()
            .any(|id| id.as_str() == "external-classifier")
    );
    assert_eq!(
        ProviderSelection::named("repository").expect("valid ID"),
        detectors.default_selection()
    );
    assert_eq!(
        ProviderSelection::named("ffprobe").expect("valid ID"),
        classifiers.default_selection()
    );
}
