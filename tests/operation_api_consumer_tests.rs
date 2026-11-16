// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;

use qubit_mime::AsyncMediaStreamClassifierBackend;
use qubit_mime::AsyncMimeDetectorBackend;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::ContentRequirement;
use qubit_mime::InputExtent;
use qubit_mime::InspectedExtent;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeCandidates;
use qubit_mime::MimeCapability;
use qubit_mime::MimeDetectionReport;
use qubit_mime::MimeError;
use qubit_mime::MimeFuture;
use qubit_mime::MimeOperationOptions;
use qubit_mime::OwnedMimeInput;
use qubit_mime::RandomAccessCapability;
use qubit_operation::OperationScope;

#[derive(Debug)]
struct Detector;

impl AsyncMimeDetectorBackend for Detector {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(8)
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a OperationScope,
        _input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async {
            Ok(MimeCandidates {
                candidates: vec![],
                inspected_extent: InspectedExtent::Prefix(0),
                retained_prefix: vec![],
            })
        })
    }
}

#[derive(Debug)]
struct Classifier;

impl AsyncMediaStreamClassifierBackend for Classifier {
    fn classify_in<'a>(
        &'a self,
        _scope: &'a OperationScope,
        _input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MediaStreamType> {
        Box::pin(async { Ok(MediaStreamType::None) })
    }
}

fn assert_send_static<T: Send + 'static>() {}
fn assert_future_send<'a, T: Send + 'a>(_: MimeFuture<'a, T>) {}

#[test]
fn test_public_operation_api_is_send_and_owned() {
    assert_send_static::<MimeDetectionReport>();
    assert_send_static::<MimeError>();
    let _unsupported = MimeError::UnsupportedCapability {
        capability: MimeCapability::RandomAccessSampling,
    };
    let _detector: Arc<dyn AsyncMimeDetectorBackend> = Arc::new(Detector);
    assert_eq!(
        _detector.random_access_capability(),
        RandomAccessCapability::Unsupported
    );
    let _classifier: Arc<dyn AsyncMediaStreamClassifierBackend> = Arc::new(Classifier);
    let _prefix = OwnedMimeInput::bytes(Arc::from(&b"head"[..]), InputExtent::Prefix);
    let _complete = OwnedMimeInput::bytes(Arc::from(&b"whole"[..]), InputExtent::Complete);
    assert_future_send(Box::pin(async {
        Ok(MimeDetectionReport {
            mime_type: None,
            backend_id: "consumer-fixture".to_owned(),
            inspected_extent: InspectedExtent::Complete,
            budget: Default::default(),
        })
    }));
}
