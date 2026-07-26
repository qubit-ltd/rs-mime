// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
use qubit_spi::ProviderMetadata;

#[test]
fn default_provider_has_the_ffprobe_identity() {
    let provider = FfprobeCommandMediaStreamClassifierProvider;

    assert_eq!("ffprobe", provider.descriptor().id().as_str());
}
