#![cfg(feature = "inventory")]

use std::sync::Arc;

use qubit_mime::MimeDetectorSpec;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::provider_descriptor;

struct First;
struct Second;
impl ProviderMetadata for First {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("duplicate-selector")
    }
}
impl ProviderMetadata for Second {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("duplicate-selector")
    }
}
impl ServiceProvider<MimeDetectorSpec> for First {
    fn create_configured(
        &self,
        _: &qubit_mime::MimeConfig,
    ) -> Result<Arc<dyn qubit_mime::MimeDetector>, ProviderFailure<qubit_mime::MimeError>> {
        unimplemented!()
    }
}
impl ServiceProvider<MimeDetectorSpec> for Second {
    fn create_configured(
        &self,
        _: &qubit_mime::MimeConfig,
    ) -> Result<Arc<dyn qubit_mime::MimeDetector>, ProviderFailure<qubit_mime::MimeError>> {
        unimplemented!()
    }
}
qubit_spi::submit_sync_provider! {
    inventory_entry = qubit_mime::detector::mime_detector_inventory::Entry;
    spec = MimeDetectorSpec;
    provider = First;
}
qubit_spi::submit_sync_provider! {
    inventory_entry = qubit_mime::detector::mime_detector_inventory::Entry;
    spec = MimeDetectorSpec;
    provider = Second;
}

#[test]
fn test_duplicate_selector_reports_submitted_source() {
    let error = qubit_mime::detector::mime_detector_inventory::build_registry()
        .expect_err("duplicate selector must fail inventory construction");
    let message = error.to_string();
    assert!(message.contains("duplicate-selector"), "{message}");
    assert!(message.contains("inventory_duplicate_tests.rs"), "{message}");
}
