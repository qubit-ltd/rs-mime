use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

use qubit_mime::{
    FixedMimeExecutionPort, MimeConfig, MimeDetector, MimeDetectorBackend, MimeDetectorProvider,
    MimeDetectorRegistry, MimeError, MimeExecutionServices,
};
use qubit_spi::error::ProviderFailure;
use qubit_spi::{
    ProviderDescriptor, ProviderId, ProviderMetadata, ServiceProvider, provider_descriptor,
};

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
fn operation_provider_registration_is_pure() {
    let registry = MimeDetectorRegistry::default();
    let counts = Arc::new([
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ]);
    registry
        .register(CountedProvider(Arc::clone(&counts)))
        .unwrap();
    let _resolved = registry.resolve().unwrap();
    assert_eq!(registry.provider_ids().len(), 1);
    assert_eq!(counts[0].load(Ordering::SeqCst), 0, "factory calls");
    assert_eq!(
        counts[1].load(Ordering::SeqCst),
        0,
        "I/O or model initialization"
    );
    assert_eq!(counts[2].load(Ordering::SeqCst), 0, "executor startup");
}

#[derive(Debug)]
struct CountedProvider(Arc<[AtomicUsize; 3]>);

impl ServiceProvider<qubit_mime::MimeDetectorSpec> for CountedProvider {
    fn create_configured(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn qubit_mime::MimeDetector>, ProviderFailure<qubit_mime::MimeError>> {
        self.0[0].fetch_add(1, Ordering::SeqCst);
        self.0[1].fetch_add(1, Ordering::SeqCst);
        self.0[2].fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(crate::support::DirectBackendDetector::new()))
    }
}

impl ProviderMetadata for CountedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("counted-provider")
    }
}

#[test]
fn operation_provider_sync_only_is_rejected() {
    let provider = SyncPanicProvider;
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 1 })
        .unwrap();
    let services = MimeExecutionServices::new(
        manager,
        group,
        Arc::new(FixedMimeExecutionPort::new(1, 1)),
        1024,
    )
    .unwrap();
    let result = provider.create_operation_backend(&MimeConfig::default(), services);
    assert!(matches!(
        result,
        Err(qubit_mime::MimeError::AsyncUnsupported)
    ));

    let detector = crate::support::DirectBackendDetector::new();
    let result = block_on(detector.guess_from_content_async(b"hello"));
    assert!(matches!(
        result,
        Err(qubit_mime::MimeError::AsyncUnsupported)
    ));
}

#[derive(Debug)]
struct SyncPanicProvider;

impl ServiceProvider<qubit_mime::MimeDetectorSpec> for SyncPanicProvider {
    fn create_configured(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        panic!("sync-only async selection called sync factory")
    }
}

impl ProviderMetadata for SyncPanicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("sync-panic").unwrap())
    }
}
