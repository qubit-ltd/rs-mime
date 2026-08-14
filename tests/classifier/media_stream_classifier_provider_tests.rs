// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
use qubit_mime::MediaStreamClassifierProvider;

fn assert_provider<T: MediaStreamClassifierProvider>() {}

#[test]
fn ffprobe_provider_implements_the_marker_contract() {
    assert_provider::<FfprobeCommandMediaStreamClassifierProvider>();
}
