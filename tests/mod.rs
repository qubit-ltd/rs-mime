/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Integration tests for `qubit-mime`.

#[path = "detector/repository_mime_detector_tests.rs"]
mod repository_mime_detector_tests;

#[path = "repository/mime_glob_tests.rs"]
mod mime_glob_tests;
#[path = "repository/mime_magic_matcher_tests.rs"]
mod mime_magic_matcher_tests;
#[path = "repository/mime_repository_tests.rs"]
mod mime_repository_tests;
#[path = "repository/mime_type_tests.rs"]
mod mime_type_tests;

#[cfg(coverage)]
#[path = "coverage_support/coverage_support_tests.rs"]
mod coverage_support_tests;
