// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Register and resolve an application-owned MIME detector provider.

use std::sync::Arc;

use qubit_mime::MimeConfig;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeDetectorSpec;
use qubit_mime::MimeError;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderMetadata;
use qubit_spi::ProviderSelection;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;

#[derive(Debug)]
struct UploadProvider;

impl ServiceProvider<MimeDetectorSpec> for UploadProvider {
    fn create_configured(&self, _config: &MimeConfig) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        let detector = qubit_mime::RepositoryMimeDetector::new().map_err(ProviderFailure::initialization_failed)?;
        Ok(Arc::new(detector))
    }
}

impl ProviderMetadata for UploadProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("upload").expect("provider ID is valid"))
    }
}

fn main() -> qubit_mime::MimeResult<()> {
    let registry = MimeDetectorRegistry::default();
    registry
        .register(UploadProvider)
        .map_err(|error| MimeError::DetectorBackend {
            backend: "registry".to_owned(),
            reason: error.to_string(),
        })?;
    let selection = ProviderSelection::named("upload").map_err(|error| MimeError::DetectorBackend {
        backend: "registry".to_owned(),
        reason: error.to_string(),
    })?;
    let detector = registry
        .resolve_selected(&selection)
        .map_err(|error| MimeError::DetectorBackend {
            backend: "registry".to_owned(),
            reason: error.to_string(),
        })?
        .create_configured(&MimeConfig::default())
        .map_err(|error| MimeError::DetectorBackend {
            backend: "registry".to_owned(),
            reason: error.to_string(),
        })?;
    assert_eq!(
        detector.detect_by_filename("report.pdf")?.as_deref(),
        Some("application/pdf")
    );
    Ok(())
}
