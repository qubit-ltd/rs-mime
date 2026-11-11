// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::RawWaker;
use std::task::RawWakerVTable;
use std::task::Waker;

use qubit_config::Config;
use qubit_fs::AsyncFileSystem;
use qubit_fs::FileSystem;
use qubit_fs::FsResult;
use qubit_fs::Path as FsPath;
use qubit_fs::metadata::FileSystemCapabilities;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::metadata::FileSystemInfo;
use qubit_fs::metadata::FileSystemLimits;
use qubit_fs::metadata::OpenedFileInfo;
use qubit_fs::metadata::SymlinkPolicy;
use qubit_fs::path::PathConstraints;
use qubit_fs::path::PathSemantics;
use qubit_fs::spi::AsyncFileSystemSpi;
use qubit_fs::spi::OpenReaderRequest;
use qubit_fs::spi::OpenedAsyncReader;
use qubit_fs::spi::ProviderOperation;
use qubit_fs::spi::ProviderOperations;
use qubit_fs::spi::ProviderProperties;
use qubit_fs::spi::SpiFuture;
use qubit_fs::spi::StatRequest;
use qubit_fs::spi::StatResponse;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
use qubit_io::AsyncInput;
use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_mime::CONFIG_MIME_AMBIGUOUS_MIME_MAPPING;
use qubit_mime::CONFIG_MIME_DETECTOR_DEFAULT;
use qubit_mime::CONFIG_MIME_DETECTOR_FALLBACKS;
use qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION;
use qubit_mime::CONFIG_MIME_PRECISE_DETECTION_PATTERNS;
use qubit_mime::ContentRequirement;
use qubit_mime::DEFAULT_MIME_MAX_BUFFER_SIZE;
use qubit_mime::FileCommandMimeDetector;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorBackend;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeError;
use qubit_mime::MimeResult;
use qubit_mime::RepositoryMimeDetector;
use qubit_spi::ProviderSelection;

use crate::support::DirectBackendDetector;
#[cfg(unix)]
use crate::support::PathEnvGuard;
use crate::support::PrefixFileSystemSpi;
use crate::support::StaticEntryPointMimeDetector;

#[derive(Debug)]
struct EmptyAsyncFileSystemSpi {
    properties: ProviderProperties,
}

impl EmptyAsyncFileSystemSpi {
    fn new() -> Self {
        Self {
            properties: ProviderProperties::new(
                FileSystemInfo::new(
                    FileSystemId::new("mime-detector-test").expect("valid fixture id"),
                    "mime-detector-test",
                    PathSemantics::Hierarchical,
                ),
                ProviderOperations::new(),
                FileSystemCapabilities::new(),
                FileSystemLimits::unknown(),
                PathConstraints::absolute(),
                SymlinkPolicy::Reject,
            )
            .expect("valid fixture properties"),
        }
    }
}

impl AsyncFileSystemSpi for EmptyAsyncFileSystemSpi {
    fn properties(&self) -> ProviderProperties {
        self.properties.clone()
    }

    fn stat<'a>(&'a self, _request: StatRequest<'a>) -> SpiFuture<'a, FsResult<StatResponse>> {
        Box::pin(async { panic!("stat should not be called") })
    }
}

#[derive(Debug)]
struct PrefixAsyncFileSystemSpi {
    properties: ProviderProperties,
    content: Vec<u8>,
    opens: Arc<AtomicUsize>,
    reads: Arc<AtomicUsize>,
    fail_open: bool,
}

impl PrefixAsyncFileSystemSpi {
    fn new(content: &[u8]) -> Self {
        Self {
            properties: ProviderProperties::new(
                FileSystemInfo::new(
                    FileSystemId::new("mime-async-prefix-test").expect("valid fixture id"),
                    "mime-async-prefix-test",
                    PathSemantics::Hierarchical,
                ),
                qubit_fs::spi::ProviderOperations::new().with(ProviderOperation::OpenReader),
                FileSystemCapabilities::new().with_guaranteed(qubit_fs::metadata::FileSystemCapability::Read),
                FileSystemLimits::unknown(),
                PathConstraints::absolute(),
                SymlinkPolicy::Reject,
            )
            .expect("valid fixture properties"),
            content: content.to_vec(),
            opens: Arc::new(AtomicUsize::new(0)),
            reads: Arc::new(AtomicUsize::new(0)),
            fail_open: false,
        }
    }

    fn failing_open(mut self) -> Self {
        self.fail_open = true;
        self
    }
}

impl AsyncFileSystemSpi for PrefixAsyncFileSystemSpi {
    fn properties(&self) -> ProviderProperties {
        self.properties.clone()
    }

    fn stat<'a>(&'a self, _request: StatRequest<'a>) -> SpiFuture<'a, FsResult<StatResponse>> {
        Box::pin(async { panic!("prefix test must not stat") })
    }

    fn open_reader<'a>(&'a self, request: OpenReaderRequest<'a>) -> SpiFuture<'a, FsResult<OpenedAsyncReader>> {
        self.opens.fetch_add(1, Ordering::Relaxed);
        if self.fail_open {
            return Box::pin(async move {
                Err(qubit_fs::FsError::new(
                    qubit_fs::error::FsErrorKind::Io,
                    qubit_fs::error::FsOperation::OpenReader,
                    "sentinel async open failure",
                )
                .with_path(request.path().clone()))
            });
        }
        let limit = usize::try_from(request.options().options().length().unwrap_or(u64::MAX))
            .unwrap_or(usize::MAX)
            .min(self.content.len());
        let bytes = self.content[..limit].to_vec();
        let reads = Arc::clone(&self.reads);
        let info = OpenedFileInfo::new(
            FileSystemId::new("mime-async-prefix-test").expect("valid fixture id"),
            request.path().clone(),
        );
        Box::pin(async move {
            Ok(OpenedAsyncReader::new(
                info,
                Box::new(MemoryAsyncInput {
                    bytes,
                    position: 0,
                    reads,
                }),
            ))
        })
    }
}

struct MemoryAsyncInput {
    bytes: Vec<u8>,
    position: usize,
    reads: Arc<AtomicUsize>,
}

impl AsyncInput for MemoryAsyncInput {
    type Item = u8;
    unsafe fn poll_read_unchecked(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        let available = self.bytes.len().saturating_sub(self.position);
        let read = count.min(available);
        output[index..index + read].copy_from_slice(&self.bytes[self.position..self.position + read]);
        self.position += read;
        self.reads.fetch_add(read, Ordering::Relaxed);
        Poll::Ready(Ok(read))
    }
}

#[derive(Debug)]
struct CountedClassifier(Arc<AtomicUsize>);

impl MediaStreamClassifier for CountedClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(MediaStreamType::AudioOnly)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(MediaStreamType::AudioOnly)
    }
}

#[derive(Debug)]
struct BackendErrorDetector;

impl MimeDetectorBackend for BackendErrorDetector {
    fn core(&self) -> &MimeDetectorCore {
        static CORE: std::sync::OnceLock<MimeDetectorCore> = std::sync::OnceLock::new();
        CORE.get_or_init(MimeDetectorCore::default)
    }
    fn max_test_bytes(&self) -> usize {
        5
    }
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }
    fn guess_from_content(&self, _content: &[u8]) -> MimeResult<Vec<String>> {
        Err(MimeError::detector_backend("test", "sentinel"))
    }
}

#[derive(Debug)]
struct CompleteBackend;

impl MimeDetectorBackend for CompleteBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Complete
    }
    fn core(&self) -> &MimeDetectorCore {
        static CORE: std::sync::OnceLock<MimeDetectorCore> = std::sync::OnceLock::new();
        CORE.get_or_init(MimeDetectorCore::default)
    }
    fn max_test_bytes(&self) -> usize {
        5
    }
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }
    fn guess_from_content(&self, _content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(Vec::new())
    }
}

#[derive(Debug)]
struct CustomCompleteBackend;

impl MimeDetectorBackend for CustomCompleteBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Complete
    }
    fn core(&self) -> &MimeDetectorCore {
        static CORE: std::sync::OnceLock<MimeDetectorCore> = std::sync::OnceLock::new();
        CORE.get_or_init(MimeDetectorCore::default)
    }
    fn max_test_bytes(&self) -> usize {
        5
    }
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }
    fn guess_from_content(&self, _content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(Vec::new())
    }
    fn guess_from_async_provider_path<'a>(
        &'a self,
        _file_system: &'a AsyncFileSystem,
        _path: &'a FsPath,
        _max_bytes: usize,
    ) -> Pin<Box<dyn Future<Output = MimeResult<(Vec<String>, Option<Vec<u8>>)>> + Send + 'a>> {
        Box::pin(async { Ok((vec!["application/x-custom-complete".to_owned()], None)) })
    }
}

fn run_ready<F: Future>(future: F) -> F::Output {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);

    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

#[test]
fn test_mime_detector_trait_supports_repository_detector() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let detector: &dyn MimeDetector = &detector;
    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector
            .detect_by_filename("photo.JPG")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_by_content(b"%PDF-1.7\n")
            .expect("content detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect(b"%PDF-1.7\n", Some("photo.jpg"), MimeDetectionPolicy::VerifyContent,)
            .expect("combined detection should succeed")
    );
}

#[test]
fn test_mime_detector_trait_supports_reader_and_file_detection() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let detector: &dyn MimeDetector = &detector;
    let mut reader = std::io::Cursor::new(b"%PDF-1.7\n".to_vec());

    let from_reader = detector
        .detect_reader(&mut reader, Some("document.pdf"), MimeDetectionPolicy::VerifyContent)
        .expect("trait-object reader detection should succeed");

    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".pdf"))
        .expect("temp file should be created");
    Output::write_fully(&mut file, b"%PDF-1.7\n").expect("temp file should be writable");
    let from_file = detector
        .detect_file(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("trait-object file detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), from_reader);
    assert_eq!(Some("application/pdf".to_owned()), from_file);
    assert_eq!(0, reader.position());
}

#[test]
fn test_mime_detector_trait_supports_filesystem_path_detection() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let detector: &dyn MimeDetector = &detector;
    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_prefix("report%literal-")
                .with_suffix(".pdf"),
        )
        .expect("temp file should be created");
    Output::write_fully(&mut file, b"%PDF-1.7\n").expect("temp file should be writable");
    // MIME probe only reads explicit files and performs no recursive
    // list/copy/delete work.
    let filesystem =
        LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem facade should be created");
    let path = host_path_to_logical(file.path()).expect("native temporary path should convert without lossy text");

    let detected = detector
        .detect_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent)
        .expect("filesystem-path detection should succeed");

    let native = detector
        .detect_file(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("native detection should agree with facade detection");
    assert_eq!(native, detected);
    assert_eq!(Some("application/pdf".to_owned()), detected);
}

#[test]
fn test_mime_detector_path_reads_small_prefix_from_large_resource() {
    let mut content = vec![0_u8; 1024 * 1024];
    content[..9].copy_from_slice(b"%PDF-1.7\n");
    let spi = PrefixFileSystemSpi::hierarchical(content);
    let filesystem = FileSystem::from_spi(spi.clone()).expect("prefix filesystem should be created");
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let path = FsPath::parse("/large/report.bin").expect("test path should parse");

    let detected = detector
        .detect_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent)
        .expect("small prefix should be sufficient for a large resource");

    assert_eq!(Some("application/pdf".to_owned()), detected);
    assert_eq!(1, spi.opened());
    assert!(spi.requested_read_bytes() <= 16);
    assert_eq!(0, spi.stat_calls());
}

#[test]
fn test_mime_detector_path_uses_object_key_filename_without_stat() {
    let spi = PrefixFileSystemSpi::object_key(vec![0_u8; 1024 * 1024]);
    let filesystem = FileSystem::from_spi(spi.clone()).expect("prefix filesystem should be created");
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let path = FsPath::parse_literal("reports/photo.jpg").expect("object-key path should parse");

    let detected = detector
        .detect_path(&filesystem, &path, 8, MimeDetectionPolicy::PreferFilename)
        .expect("object-key filename detection should succeed");

    assert_eq!(Some("image/jpeg".to_owned()), detected);
    assert_eq!(0, spi.opened());
    assert_eq!(0, spi.requested_read_bytes());
    assert_eq!(0, spi.stat_calls());
}

#[cfg(unix)]
#[test]
fn test_mime_detector_path_accepts_non_utf8_native_filename() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    use qubit_local_files::options::LocalTempDirectoryOptions;

    let native = LocalFileSystem::host().expect("native host");
    let mut directory = native
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&std::env::temp_dir()))
        .expect("temporary directory");
    let file = directory.path().join(OsString::from_vec(b"report\xff.pdf".to_vec()));
    let native_creation = std::fs::write(&file, b"%PDF-1.7\n");

    let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host facade");
    let path = host_path_to_logical(&file).expect("lossless conversion");
    let detector = RepositoryMimeDetector::new().expect("repository detector");

    #[cfg(target_os = "macos")]
    if let Err(error) = native_creation {
        const EILSEQ: i32 = 92;
        assert_eq!(error.raw_os_error(), Some(EILSEQ), "unexpected native filename error");
        assert_eq!(
            detector
                .detect_path(&filesystem, &path, 16, MimeDetectionPolicy::PreferFilename)
                .expect("filename classification does not require native creation"),
            Some("application/pdf".to_owned()),
        );
        assert!(
            detector
                .detect_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent)
                .is_err()
        );
        directory.cleanup().expect("cleanup rejected filename fixture");
        return;
    }
    #[cfg(not(target_os = "macos"))]
    native_creation.expect("raw filename fixture");

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent)
            .expect("detect raw filename"),
    );
    directory.cleanup().expect("explicit cleanup");
}

#[test]
fn test_mime_detector_path_rejects_requests_above_detector_buffer_limit() {
    let detector = StaticEntryPointMimeDetector;
    let filesystem =
        LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem facade should be created");
    let path = FsPath::parse("/unused").expect("test path should be a valid filesystem path");

    let error = detector
        .detect_path(&filesystem, &path, 1, MimeDetectionPolicy::VerifyContent)
        .expect_err("path reads above the detector buffer limit must fail");

    assert!(matches!(
        error,
        MimeError::BufferLimitExceeded { requested: 1, limit: 0 }
    ));
}

#[test]
fn test_mime_detector_backend_defaults_read_reader_and_file_prefix() {
    let detector = DirectBackendDetector::new();
    let mut reader = std::io::Cursor::new(b"hello world".to_vec());

    let (reader_candidates, reader_content) = MimeDetectorBackend::guess_from_reader(&detector, &mut reader)
        .expect("backend reader default should read content prefix");

    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("temp file should be created");
    Output::write_fully(&mut file, b"hello world").expect("temp file should be writable");
    let (file_candidates, file_content) = MimeDetectorBackend::guess_from_file(&detector, file.path())
        .expect("backend file default should read content prefix");

    assert_eq!(vec!["text/plain".to_owned()], reader_candidates);
    assert_eq!(b"hello".to_vec(), reader_content);
    assert_eq!(vec!["text/plain".to_owned()], file_candidates);
    assert_eq!(b"hello".to_vec(), file_content);
    assert_eq!(0, reader.position());
}

#[test]
fn test_async_prefix_reaches_filesystem_after_buffer_preflight() {
    let detector = DirectBackendDetector::new();
    let spi = PrefixAsyncFileSystemSpi::new(b"hello world");
    let opens = Arc::clone(&spi.opens);
    let reads = Arc::clone(&spi.reads);
    let filesystem = AsyncFileSystem::from_spi(spi).expect("async fixture");
    let path = FsPath::parse("/unused").expect("valid fixture path");

    assert_eq!(
        Some("text/plain".to_owned()),
        run_ready(detector.detect_async_path(&filesystem, &path, 5, MimeDetectionPolicy::VerifyContent,))
            .expect("async prefix detection should succeed")
    );
    assert_eq!(1, opens.load(Ordering::Relaxed));
    assert_eq!(5, reads.load(Ordering::Relaxed));

    let oversized = DEFAULT_MIME_MAX_BUFFER_SIZE + 1;
    let error =
        run_ready(detector.detect_async_path(&filesystem, &path, oversized, MimeDetectionPolicy::VerifyContent))
            .expect_err("oversized prefix must be rejected before I/O");
    assert!(matches!(
        error,
        MimeError::BufferLimitExceeded {
            requested,
            limit
        }
        if requested == oversized && limit == DEFAULT_MIME_MAX_BUFFER_SIZE
    ));
    assert_eq!(
        1,
        opens.load(Ordering::Relaxed),
        "oversized read must not open provider resource"
    );
    assert_eq!(
        5,
        reads.load(Ordering::Relaxed),
        "oversized read must not consume bytes"
    );

    let as_dyn: &dyn MimeDetector = &detector;
    assert_eq!(
        Some("text/plain".to_owned()),
        run_ready(as_dyn.detect_async_path(&filesystem, &path, 5, MimeDetectionPolicy::VerifyContent,))
            .expect("dyn async detection should succeed")
    );
    let boxed: Box<dyn MimeDetector> = Box::new(DirectBackendDetector::new());
    assert_eq!(
        Some("text/plain".to_owned()),
        run_ready(boxed.detect_async_path(&filesystem, &path, 5, MimeDetectionPolicy::VerifyContent,))
            .expect("boxed async detection should succeed")
    );
    let shared: Arc<dyn MimeDetector> = Arc::new(DirectBackendDetector::new());
    assert_eq!(
        Some("text/plain".to_owned()),
        run_ready(shared.detect_async_path(&filesystem, &path, 5, MimeDetectionPolicy::VerifyContent,))
            .expect("shared async detection should succeed")
    );

    let filename_path = FsPath::parse("/note.txt").expect("valid filename path");
    let before = opens.load(Ordering::Relaxed);
    assert_eq!(
        Some("text/plain".to_owned()),
        run_ready(detector.detect_async_path(&filesystem, &filename_path, 5, MimeDetectionPolicy::PreferFilename,))
            .expect("unique filename candidate should short-circuit async content")
    );
    assert_eq!(
        before,
        opens.load(Ordering::Relaxed),
        "PreferFilename must not open provider resource"
    );
}

#[test]
fn test_file_command_async_shortcuts_do_not_open_provider_or_start_worker() {
    let spi = PrefixAsyncFileSystemSpi::new(b"%PDF-1.7\n");
    let opens = Arc::clone(&spi.opens);
    let filesystem = AsyncFileSystem::from_spi(spi).expect("async fixture");
    let detector = FileCommandMimeDetector::new();
    let pdf_path = FsPath::parse("/document.pdf").expect("PDF path");

    assert_eq!(
        Some("application/pdf".to_owned()),
        run_ready(detector.detect_async_path(&filesystem, &pdf_path, 16, MimeDetectionPolicy::PreferFilename))
            .expect("unique filename candidate should short-circuit file content inspection"),
    );
    assert_eq!(
        0,
        opens.load(Ordering::Relaxed),
        "filename short-circuit must not start a file worker"
    );

    let path = FsPath::parse("/unknown").expect("fixture path");
    let oversized = DEFAULT_MIME_MAX_BUFFER_SIZE + 1;
    assert!(matches!(
        run_ready(detector.detect_async_path(&filesystem, &path, oversized, MimeDetectionPolicy::VerifyContent)),
        Err(MimeError::BufferLimitExceeded { requested, limit }) if requested == oversized && limit == DEFAULT_MIME_MAX_BUFFER_SIZE
    ));
    assert_eq!(
        0,
        opens.load(Ordering::Relaxed),
        "over-limit request must not start a file worker"
    );
}

#[test]
fn test_repository_async_prefix_reads_pdf_from_local_temp_fixture() {
    let mut file = LocalFileSystem::host()
        .expect("host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".pdf"))
        .expect("temporary PDF should be created");
    Output::write_fully(&mut file, b"%PDF-1.7\n").expect("temporary PDF should be writable");
    let bytes = std::fs::read(file.path()).expect("temporary PDF should be readable");
    let filesystem = AsyncFileSystem::from_spi(PrefixAsyncFileSystemSpi::new(&bytes))
        .expect("async filesystem facade should be created");
    let path = FsPath::parse("/document.pdf").expect("logical PDF path should parse");
    let detector = RepositoryMimeDetector::new().expect("bundled repository should load");

    assert_eq!(
        Some("application/pdf".to_owned()),
        run_ready(detector.detect_async_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent))
            .expect("repository async prefix detection should succeed"),
    );
    let as_dyn: &dyn MimeDetector = &detector;
    assert_eq!(
        Some("application/pdf".to_owned()),
        run_ready(as_dyn.detect_async_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent))
            .expect("dyn repository async prefix detection should succeed"),
    );
    let boxed: Box<dyn MimeDetector> = Box::new(RepositoryMimeDetector::new().expect("repository should load"));
    assert_eq!(
        Some("application/pdf".to_owned()),
        run_ready(boxed.detect_async_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent))
            .expect("boxed repository async prefix detection should succeed"),
    );
    let shared: Arc<dyn MimeDetector> = Arc::new(RepositoryMimeDetector::new().expect("repository should load"));
    assert_eq!(
        Some("application/pdf".to_owned()),
        run_ready(shared.detect_async_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent))
            .expect("shared repository async prefix detection should succeed"),
    );
}

#[test]
fn test_async_prefix_preserves_filesystem_and_backend_errors() {
    let path = FsPath::parse("/unused").expect("valid fixture path");
    let detector = DirectBackendDetector::new();
    let spi = PrefixAsyncFileSystemSpi::new(b"hello").failing_open();
    let filesystem = AsyncFileSystem::from_spi(spi).expect("async fixture");
    let error = run_ready(detector.detect_async_path(&filesystem, &path, 5, MimeDetectionPolicy::VerifyContent))
        .expect_err("filesystem open error should be returned");
    match error {
        MimeError::FileSystem(error) => {
            assert!(error.to_string().contains("sentinel async open failure"))
        }
        other => panic!("filesystem error was changed: {other:?}"),
    }

    let filesystem = AsyncFileSystem::from_spi(PrefixAsyncFileSystemSpi::new(b"hello")).expect("async fixture");
    let error =
        run_ready(BackendErrorDetector.detect_async_path(&filesystem, &path, 5, MimeDetectionPolicy::VerifyContent))
            .expect_err("backend error should be returned");
    assert!(
        matches!(error, MimeError::DetectorBackend { backend, reason } if backend == "test" && reason == "sentinel")
    );
}

#[test]
fn test_async_prefix_complete_default_and_custom_hook_contract() {
    let path = FsPath::parse("/unused").expect("valid fixture path");
    let spi = PrefixAsyncFileSystemSpi::new(b"hello");
    let opens = Arc::clone(&spi.opens);
    let filesystem = AsyncFileSystem::from_spi(spi).expect("async fixture");
    let error = run_ready(CompleteBackend.detect_async_path(&filesystem, &path, 5, MimeDetectionPolicy::VerifyContent))
        .expect_err("complete backend default must reject bounded reads");
    assert!(matches!(error, MimeError::CompleteContentRequired));
    assert_eq!(
        0,
        opens.load(Ordering::Relaxed),
        "complete default must reject before open"
    );
    assert_eq!(
        Some("application/x-custom-complete".to_owned()),
        run_ready(CustomCompleteBackend.detect_async_path(&filesystem, &path, 5, MimeDetectionPolicy::VerifyContent,))
            .expect("custom complete hook should remain usable"),
    );
    assert_eq!(
        0,
        opens.load(Ordering::Relaxed),
        "custom hook owns its complete-content strategy"
    );
}

#[test]
fn test_detect_prefix_uses_prefix_source_and_filename_short_circuit() {
    let detector = DirectBackendDetector::new();
    assert_eq!(
        Some("text/plain".to_owned()),
        detector
            .detect_prefix(b"hello", None, MimeDetectionPolicy::VerifyContent)
            .expect("prefix detection should succeed")
    );
    assert_eq!(
        Some("text/plain".to_owned()),
        detector
            .detect_prefix(b"not recognized", Some("note.txt"), MimeDetectionPolicy::PreferFilename)
            .expect("unique filename match should skip prefix backend")
    );
}

#[test]
fn test_prefix_paths_skip_classifier_but_complete_content_uses_it() {
    let mut config = Config::default();
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
        .expect("config setting");
    config
        .set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "webm")
        .expect("config setting");
    config
        .set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, "webm:video/webm,audio/webm")
        .expect("config setting");
    let mut detector = RepositoryMimeDetector::from_mime_config(
        MimeConfig::from_config(&config).expect("precise config should parse"),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    detector
        .core_mut()
        .set_media_stream_classifier(Some(Arc::new(CountedClassifier(Arc::clone(&calls)))));

    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .detect_prefix(b"prefix", Some("movie.webm"), MimeDetectionPolicy::Freedesktop)
            .expect("bounded prefix detection should succeed"),
    );
    assert_eq!(
        0,
        calls.load(Ordering::Relaxed),
        "bounded prefix must not invoke complete-content classifier"
    );

    assert_eq!(
        Some("audio/webm".to_owned()),
        detector
            .detect(b"complete", Some("movie.webm"), MimeDetectionPolicy::Freedesktop)
            .expect("complete-content detection should succeed"),
    );
    assert_eq!(
        1,
        calls.load(Ordering::Relaxed),
        "complete-content detect should classify once"
    );

    let sync_spi = PrefixFileSystemSpi::hierarchical(b"prefix".to_vec());
    let sync_fs = FileSystem::from_spi(sync_spi).expect("sync filesystem fixture");
    let sync_path = FsPath::parse("/movie.webm").expect("sync path");
    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .detect_path(&sync_fs, &sync_path, 16, MimeDetectionPolicy::Freedesktop,)
            .expect("sync path prefix detection should succeed")
    );
    assert_eq!(
        1,
        calls.load(Ordering::Relaxed),
        "detect_path must not invoke classifier on prefix"
    );

    let async_spi = PrefixAsyncFileSystemSpi::new(b"prefix");
    let async_fs = AsyncFileSystem::from_spi(async_spi).expect("async filesystem fixture");
    let async_path = FsPath::parse("/movie.webm").expect("async path");
    assert_eq!(
        Some("video/webm".to_owned()),
        run_ready(detector.detect_async_path(&async_fs, &async_path, 16, MimeDetectionPolicy::Freedesktop,))
            .expect("async path prefix detection should succeed")
    );
    assert_eq!(
        1,
        calls.load(Ordering::Relaxed),
        "async prefix must not invoke complete-content classifier"
    );
}

#[test]
fn test_prefer_filename_preserves_unique_webm_candidate_without_classification() {
    let mut config = Config::default();
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
        .expect("config setting");
    config
        .set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "webm")
        .expect("config setting");
    config
        .set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, "webm:video/webm,audio/webm")
        .expect("config setting");
    let mut detector = RepositoryMimeDetector::from_mime_config(
        MimeConfig::from_config(&config).expect("precise config should parse"),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    detector
        .core_mut()
        .set_media_stream_classifier(Some(Arc::new(CountedClassifier(Arc::clone(&calls)))));

    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .detect(b"contents", Some("movie.webm"), MimeDetectionPolicy::PreferFilename)
            .expect("bytes detection should preserve unique filename candidate")
    );
    assert_eq!(0, calls.load(Ordering::Relaxed));

    let mut reader = std::io::Cursor::new(b"contents".to_vec());
    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .detect_reader(&mut reader, Some("movie.webm"), MimeDetectionPolicy::PreferFilename)
            .expect("reader detection should preserve unique filename candidate")
    );
    assert_eq!(0, calls.load(Ordering::Relaxed));

    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".webm"))
        .expect("temp file should be created");
    Output::write_fully(&mut file, b"contents").expect("temp file should be writable");
    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .detect_file(file.path(), MimeDetectionPolicy::PreferFilename)
            .expect("file detection should preserve unique filename candidate")
    );
    assert_eq!(0, calls.load(Ordering::Relaxed));

    let sync_spi = PrefixFileSystemSpi::hierarchical(b"contents".to_vec());
    let sync_fs = FileSystem::from_spi(sync_spi).expect("sync filesystem fixture");
    let sync_path = FsPath::parse("/movie.webm").expect("sync path");
    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .detect_path(&sync_fs, &sync_path, 16, MimeDetectionPolicy::PreferFilename)
            .expect("sync provider detection should preserve unique filename candidate")
    );
    assert_eq!(0, calls.load(Ordering::Relaxed));

    let async_spi = PrefixAsyncFileSystemSpi::new(b"contents");
    let async_fs = AsyncFileSystem::from_spi(async_spi).expect("async filesystem fixture");
    let async_path = FsPath::parse("/movie.webm").expect("async path");
    assert_eq!(
        Some("video/webm".to_owned()),
        run_ready(detector.detect_async_path(&async_fs, &async_path, 16, MimeDetectionPolicy::PreferFilename))
            .expect("async provider detection should preserve unique filename candidate")
    );
    assert_eq!(0, calls.load(Ordering::Relaxed));

    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .detect_prefix(b"contents", Some("movie.webm"), MimeDetectionPolicy::PreferFilename)
            .expect("prefix detection should preserve unique filename candidate")
    );
    assert_eq!(0, calls.load(Ordering::Relaxed));
}

#[test]
fn test_mime_detector_backend_prefer_filename_skips_reader_and_file_content() {
    let detector = DirectBackendDetector::new();
    let mut reader = std::io::Cursor::new(b"xxxxx".to_vec());

    let from_reader = detector
        .detect_reader(&mut reader, Some("note.txt"), MimeDetectionPolicy::PreferFilename)
        .expect("filename-preferred reader detection should succeed");

    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".txt"))
        .expect("temp file should be created");
    Output::write_fully(&mut file, b"xxxxx").expect("temp file should be writable");
    let from_file = detector
        .detect_file(file.path(), MimeDetectionPolicy::PreferFilename)
        .expect("filename-preferred file detection should succeed");

    assert_eq!(Some("text/plain".to_owned()), from_reader);
    assert_eq!(Some("text/plain".to_owned()), from_file);
    assert_eq!(0, reader.position());
}

#[test]
fn test_default_mime_detector_returns_usable_detector() {
    let registry = MimeDetectorRegistry::builtin();
    let detector = registry
        .resolve()
        .expect("default provider selection")
        .create()
        .expect("default detector");
    assert!(
        detector
            .detect_by_filename("document.pdf")
            .expect("filename detection should succeed")
            .is_some()
    );
}

#[test]
fn test_mime_detector_registry_creates_boxed_and_shared_named_detectors() {
    #[cfg(unix)]
    let _path_guard = PathEnvGuard::preserve();

    let registry = MimeDetectorRegistry::builtin();
    let config = MimeConfig::default();
    let boxed = create_named_detector(&registry, "repository", &config);
    let shared = create_named_detector(&registry, "repository", &config);
    let boxed_file = create_named_detector(&registry, "file", &config);
    let shared_file = create_named_detector(&registry, "file", &config);

    assert_eq!(
        Some("application/pdf".to_owned()),
        boxed
            .detect_by_filename("document.pdf")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("image/png".to_owned()),
        shared
            .detect_by_filename("image.png")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        boxed_file
            .detect_by_filename("document.pdf")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("image/png".to_owned()),
        shared_file
            .detect_by_filename("image.png")
            .expect("filename detection should succeed")
    );
    let unknown = ProviderSelection::named("unknown").expect("unknown selector should still be syntactically valid");
    assert!(registry.resolve_selected(&unknown).is_err());
}

#[test]
fn test_mime_detector_registry_creates_from_explicit_registry() {
    let registry = MimeDetectorRegistry::builtin();
    let config = create_detector_config("repository");
    let boxed = create_named_detector(&registry, "repository", &config);
    let shared = create_named_detector(&registry, "repository", &config);
    let shared_default = registry
        .resolve_selected(config.mime_detector_selection())
        .expect("configured registry selection should resolve")
        .create_configured(&config)
        .expect("shared registry default should create detector");

    assert_eq!(
        Some("application/pdf".to_owned()),
        boxed
            .detect_by_filename("document.pdf")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        shared
            .detect_by_filename("document.pdf")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        shared_default
            .detect_by_filename("document.pdf")
            .expect("filename detection should succeed")
    );
}

#[test]
fn test_boxed_mime_detector_trait_object_delegates_all_entry_points() {
    let detector: Box<dyn MimeDetector> = Box::new(StaticEntryPointMimeDetector);
    let mut reader = std::io::Cursor::new(b"data".to_vec());
    let filesystem = AsyncFileSystem::from_spi(EmptyAsyncFileSystemSpi::new()).expect("async fixture");
    let path = FsPath::parse("/unused").expect("valid fixture path");

    assert_eq!(ContentRequirement::Complete, detector.content_requirement());
    assert!(matches!(
        detector.detect_prefix(b"data", None, MimeDetectionPolicy::PreferFilename),
        Err(MimeError::CompleteContentRequired)
    ));
    assert!(matches!(
        run_ready(detector.detect_async_path(&filesystem, &path, 0, MimeDetectionPolicy::PreferFilename,)),
        Err(MimeError::CompleteContentRequired)
    ));

    assert_eq!(
        Some("application/x-static-name".to_owned()),
        detector
            .detect_by_filename("file.bin")
            .expect("boxed filename delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        detector
            .detect_by_content(b"data")
            .expect("boxed content delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-detect".to_owned()),
        detector
            .detect(b"data", Some("file.bin"), MimeDetectionPolicy::PreferFilename)
            .expect("boxed detection delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-reader".to_owned()),
        detector
            .detect_reader(&mut reader, None, MimeDetectionPolicy::PreferFilename)
            .expect("boxed reader delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-file".to_owned()),
        detector
            .detect_file(std::path::Path::new("Cargo.toml"), MimeDetectionPolicy::PreferFilename)
            .expect("boxed file delegation should succeed")
    );
}

#[test]
fn test_shared_mime_detector_trait_object_delegates_all_entry_points() {
    let detector: std::sync::Arc<dyn MimeDetector> = std::sync::Arc::new(StaticEntryPointMimeDetector);
    let cloned = detector.clone();
    let mut reader = std::io::Cursor::new(b"data".to_vec());
    let filesystem = AsyncFileSystem::from_spi(EmptyAsyncFileSystemSpi::new()).expect("async fixture");
    let path = FsPath::parse("/unused").expect("valid fixture path");

    assert_eq!(ContentRequirement::Complete, detector.content_requirement());
    assert!(matches!(
        detector.detect_prefix(b"data", None, MimeDetectionPolicy::PreferFilename),
        Err(MimeError::CompleteContentRequired)
    ));
    assert!(matches!(
        run_ready(detector.detect_async_path(&filesystem, &path, 0, MimeDetectionPolicy::PreferFilename,)),
        Err(MimeError::CompleteContentRequired)
    ));

    assert_eq!(
        Some("application/x-static-name".to_owned()),
        cloned
            .as_ref()
            .detect_by_filename("file.bin")
            .expect("arc filename delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        detector
            .detect_by_content(b"data")
            .expect("arc content delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-detect".to_owned()),
        detector
            .detect(b"data", Some("file.bin"), MimeDetectionPolicy::PreferFilename)
            .expect("arc detection delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-reader".to_owned()),
        detector
            .detect_reader(&mut reader, None, MimeDetectionPolicy::PreferFilename)
            .expect("arc reader delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-file".to_owned()),
        detector
            .detect_file(std::path::Path::new("Cargo.toml"), MimeDetectionPolicy::PreferFilename)
            .expect("arc file delegation should succeed")
    );
}

#[test]
fn test_mime_detector_registry_builds_from_config_defaults() {
    let registry = MimeDetectorRegistry::builtin();
    let config = MimeConfig::from_config(&Config::new()).expect("default MIME config should parse");
    let boxed = create_configured_detector(&registry, &config);
    let shared = create_configured_detector(&registry, &config);

    assert!(
        boxed
            .detect_by_filename("document.pdf")
            .expect("boxed filename detection should succeed")
            .is_some()
    );
    assert!(
        shared
            .detect_by_filename("document.pdf")
            .expect("shared filename detection should succeed")
            .is_some()
    );
}

#[test]
fn test_configured_fallback_rejects_unknown_detector() {
    let registry = MimeDetectorRegistry::builtin();
    let config = create_detector_config_with_fallbacks("unknown", &["repository"]);
    let error = registry
        .resolve_selected(config.mime_detector_selection())
        .expect_err("strict configured chain should reject unknown providers");

    assert!(error.is_unknown_providers());
    assert!(
        error
            .selectors()
            .expect("unknown-provider errors should retain selectors")
            .iter()
            .any(|selector| selector.as_str() == "unknown")
    );
}

#[test]
fn test_detector_backend_error_builder_preserves_context() {
    let error = MimeError::detector_backend("custom-backend", "command failed");

    assert!(matches!(
        error,
        MimeError::DetectorBackend {
            ref backend,
            ref reason,
        } if backend == "custom-backend" && reason == "command failed"
    ));
    assert_eq!(
        "MIME detector backend 'custom-backend' failed: command failed",
        error.to_string()
    );
}

fn create_detector_config(detector: &str) -> MimeConfig {
    create_detector_config_with_fallbacks(detector, &[])
}

fn create_detector_config_with_fallbacks(detector: &str, fallbacks: &[&str]) -> MimeConfig {
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, detector)
        .expect("detector default should be configurable");
    config
        .set(CONFIG_MIME_DETECTOR_FALLBACKS, fallbacks.join(","))
        .expect("detector fallbacks should be configurable");
    MimeConfig::from_config(&config).expect("detector config should parse")
}

/// Resolves a named provider and creates its detector with explicit config.
///
/// # Parameters
///
/// * `registry` - Registry containing the named provider.
/// * `selector` - Canonical ID or alias to resolve.
/// * `config` - MIME configuration supplied only during service creation.
///
/// # Returns
///
/// The detector created by the selected provider.
fn create_named_detector(
    registry: &MimeDetectorRegistry,
    selector: &str,
    config: &MimeConfig,
) -> std::sync::Arc<dyn MimeDetector> {
    let selection = ProviderSelection::named(selector).expect("test provider selector should be valid");
    registry
        .resolve_selected(&selection)
        .expect("named detector provider should resolve")
        .create_configured(config)
        .expect("named detector provider should create its service")
}

/// Resolves the optional selection carried by a MIME configuration object.
///
/// # Parameters
///
/// * `registry` - Registry containing configured candidates.
/// * `config` - Independent source of both a selection and service settings.
///
/// # Returns
///
/// The detector created after the explicit two-stage operation.
fn create_configured_detector(
    registry: &MimeDetectorRegistry,
    config: &MimeConfig,
) -> std::sync::Arc<dyn MimeDetector> {
    registry
        .resolve_selected(config.mime_detector_selection())
        .expect("configured detector selection should resolve")
        .create_configured(config)
        .expect("configured detector provider should create its service")
}

/// MIME content inspection benefits from guaranteed ranges without metadata
/// I/O.
#[test]
fn test_detect_path_uses_guaranteed_prefix_range() {
    let mut content = vec![0_u8; 1024 * 1024];
    content[..9].copy_from_slice(b"%PDF-1.7\n");
    let spi = PrefixFileSystemSpi::object_key(content).with_guaranteed_range();
    let filesystem = FileSystem::from_spi(spi.clone()).expect("prefix filesystem should be created");
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let path = FsPath::parse_literal("large/report.bin").expect("object-key path should parse");
    assert_eq!(
        detector
            .detect_path(&filesystem, &path, 16, MimeDetectionPolicy::VerifyContent)
            .expect("guaranteed prefix detection should succeed"),
        Some("application/pdf".to_owned())
    );
    assert_eq!(spi.range_length(), Some(16));
    assert_eq!(spi.opened(), 1);
    assert_eq!(spi.stat_calls(), 0);
    assert!(spi.requested_read_bytes() <= 16);
}

/// Detector buffer admission happens before opening any provider reader.
#[test]
fn test_detect_path_rejects_buffer_limit_before_provider_open() {
    let spi = PrefixFileSystemSpi::hierarchical(b"%PDF-1.7\n".to_vec()).with_guaranteed_range();
    let filesystem = FileSystem::from_spi(spi.clone()).expect("prefix filesystem should be created");
    let error = StaticEntryPointMimeDetector
        .detect_path(
            &filesystem,
            &FsPath::parse("/payload").expect("test path should be valid"),
            1,
            MimeDetectionPolicy::VerifyContent,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        MimeError::BufferLimitExceeded { requested: 1, limit: 0 }
    ));
    assert_eq!(spi.opened(), 0);
    assert_eq!(spi.stat_calls(), 0);
}
