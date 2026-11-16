use std::sync::Arc;

use qubit_mime::detector::MimeRuntime;
use qubit_mime::{
    FixedMimeExecutionPort, InputExtent, MimeExecutionServices, MimeOperationOptions,
    OwnedMimeInput,
};
use qubit_operation::{OperationManager, OperationScope};

/// Compile-time consumer path for root submission, child submission, both
/// handle modes, explicit settlement observation, and service shutdown.
async fn public_operation_flow(
    runtime: &MimeRuntime,
    scope: &OperationScope,
    services: &MimeExecutionServices,
) {
    let mut root = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(&b"%PDF-1.7"[..]), InputExtent::Complete),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("root request is admitted after preparation"));
    let _root_outcome = root.wait_outcome().await;
    let _root_settlement = root.wait_settlement().await;

    let mut child = runtime
        .try_submit_detection_in(
            scope,
            OwnedMimeInput::bytes(Arc::from(&b"%PDF-1.7"[..]), InputExtent::Complete),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("child request is admitted while its parent remains active"));
    let _child_outcome = child.wait_outcome().await;
    let _child_settlement = child.wait_settlement().await;

    let mut sync_child = runtime
        .try_submit_detection_sync_in(
            scope,
            OwnedMimeInput::bytes(Arc::from(&b"%PDF-1.7"[..]), InputExtent::Complete),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("synchronous child request is admitted"));
    let _sync_outcome = sync_child.wait_outcome();
    let _sync_settlement = sync_child.wait_settlement();

    let observer = services.shutdown();
    let _settled = observer.wait_services_settled().await;
    let _stopped = observer.stop_services();
}

fn main() {
    let manager = OperationManager::new(Default::default()).expect("manager");
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 8 })
        .expect("MIME group");
    let services = MimeExecutionServices::new(
        manager,
        group,
        Arc::new(FixedMimeExecutionPort::new(1, 8)),
        1024 * 1024,
    )
    .expect("execution services");
    let _ = MimeRuntime::builtin()
        .expect("repository runtime")
        .with_execution_services(services);
    let _ = public_operation_flow;
}
