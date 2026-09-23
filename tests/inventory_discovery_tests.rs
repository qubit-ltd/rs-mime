#![cfg(feature = "inventory")]

use std::sync::Arc;

use qubit_mime::MediaStreamClassifierRegistry;
use qubit_mime::MediaStreamClassifierSpec;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeDetectorSpec;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ProviderSelection;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::provider_descriptor;

struct ExternalDetector;
impl ProviderMetadata for ExternalDetector {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("external-detector")
    }
}
impl ServiceProvider<MimeDetectorSpec> for ExternalDetector {
    fn create_configured(
        &self,
        _config: &qubit_mime::MimeConfig,
    ) -> Result<Arc<dyn qubit_mime::MimeDetector>, ProviderFailure<qubit_mime::MimeError>> {
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
        _config: &qubit_mime::MimeConfig,
    ) -> Result<Arc<dyn qubit_mime::MediaStreamClassifier>, ProviderFailure<qubit_mime::MimeError>> {
        unimplemented!("discovery does not create the classifier")
    }
}

qubit_spi::submit_sync_provider! {
    inventory_entry = qubit_mime::detector::mime_detector_inventory::Entry;
    spec = MimeDetectorSpec;
    provider = ExternalDetector;
}
qubit_spi::submit_sync_provider! {
    inventory_entry = qubit_mime::classifier::media_stream_classifier_inventory::Entry;
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
