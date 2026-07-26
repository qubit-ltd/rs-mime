// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_mime::{
    MimeConfig,
    MimeDetector,
    MimeDetectorSpec,
};
use qubit_spi::{
    ServiceSpec,
    SyncServiceSpec,
};

#[test]
fn specification_exposes_the_expected_contract_types() {
    fn assert_config<T: ServiceSpec<Config = MimeConfig>>() {}
    fn assert_output<T: SyncServiceSpec<Output = Arc<dyn MimeDetector>>>() {}

    assert_config::<MimeDetectorSpec>();
    assert_output::<MimeDetectorSpec>();
}
