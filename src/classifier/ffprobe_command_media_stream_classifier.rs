// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! FFprobe-backed media stream classifier.

use std::path::Path;
use std::sync::Arc;

use qubit_command::Command;
use qubit_command::CommandRunner;

use crate::AsyncMediaStreamClassifierBackend;
use crate::BorrowedMimeInput;
use crate::FileBasedMediaStreamClassifier;
use crate::InputExtent;
use crate::MediaStreamType;
use crate::MimeConfig;
use crate::MimeError;
use crate::MimeFuture;
use crate::MimeOperationOptions;
use crate::MimeResult;
use crate::command_execution::AsyncMimeCommandExecutor;
use crate::command_execution::ManagedCommandExecutor;
use crate::command_execution::ManagedMimeCommand;
use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::MimeExecutionWorker;
use crate::command_execution::SystemMimeCommandExecutor;
use crate::command_execution::require_complete_stdout;
use crate::detector::MimeExecutionServices;
use crate::detector::mime_input::OwnedMimeInput;
use crate::detector::stream_type_from_output;
use crate::managed_temp_staging::ManagedStagingLimits;
use crate::managed_temp_staging::stage_borrowed_bytes_in;
use crate::managed_temp_staging::stage_owned_in;

/// Media stream classifier backed by the `ffprobe` command.
///
/// # Examples
///
/// ```
/// use qubit_mime::FfprobeCommandMediaStreamClassifier;
/// let classifier = FfprobeCommandMediaStreamClassifier::new();
/// assert!(classifier.max_staging_size() > 0);
/// ```
#[derive(Clone)]
pub struct FfprobeCommandMediaStreamClassifier {
    /// The working directory used to execute FFprobe.
    working_directory: Option<String>,
    /// The command runner used to execute FFprobe.
    command_runner: CommandRunner,
    /// Maximum bytes staged from reader/content input.
    max_staging_size: u64,
    /// Optional operation services for explicit asynchronous classification.
    execution_services: Option<Arc<MimeExecutionServices>>,
    /// Optional test or adapter executor; production uses the managed command
    /// service.
    async_executor: Option<Arc<dyn AsyncMimeCommandExecutor>>,
}

impl std::fmt::Debug for FfprobeCommandMediaStreamClassifier {
    /// Formats classifier configuration while omitting command service
    /// internals.
    ///
    /// # Parameters
    /// - `f`: Formatter receiving the classifier configuration.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FfprobeCommandMediaStreamClassifier")
            .field("working_directory", &self.working_directory)
            .field("command_runner", &self.command_runner)
            .field("max_staging_size", &self.max_staging_size)
            .field("has_execution_services", &self.execution_services.is_some())
            .field("has_async_executor", &self.async_executor.is_some())
            .finish()
    }
}

impl FfprobeCommandMediaStreamClassifier {
    /// FFprobe executable name.
    pub const COMMAND: &'static str = "ffprobe";
    /// FFprobe stream name for video streams.
    pub const VIDEO_STREAM: &'static str = "video";
    /// FFprobe stream name for audio streams.
    pub const AUDIO_STREAM: &'static str = "audio";

    /// Creates a FFprobe-backed classifier.
    ///
    /// # Returns
    /// A classifier using the current process working directory.
    #[must_use]
    pub fn new() -> Self {
        Self::from_mime_config(MimeConfig::default())
    }

    /// Creates a FFprobe-backed classifier from MIME configuration.
    ///
    /// # Parameters
    /// - `config`: MIME configuration providing reader/content staging limits.
    ///
    /// # Returns
    /// A classifier using the current process working directory.
    #[must_use]
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self {
            working_directory: None,
            command_runner: Self::default_command_runner(&config),
            max_staging_size: config.media_stream_max_staging_size(),
            execution_services: None,
            async_executor: None,
        }
    }

    /// Associates services required to run the asynchronous command path.
    ///
    /// # Parameters
    /// - `services`: Prepared operation services that own asynchronous command
    ///   execution and staging budgets.
    pub(crate) fn with_execution_services(mut self, services: Arc<MimeExecutionServices>) -> Self {
        self.execution_services = Some(services);
        self
    }

    #[cfg(test)]
    fn with_async_executor(mut self, executor: Arc<dyn AsyncMimeCommandExecutor>) -> Self {
        self.async_executor = Some(executor);
        self
    }

    /// Gets the command runner used by this classifier.
    ///
    /// # Returns
    /// Runner used for `ffprobe` command executions.
    #[must_use = "the command runner must be inspected or configured"]
    #[inline]
    pub fn command_runner(&self) -> &CommandRunner {
        &self.command_runner
    }

    /// Gets the maximum bytes staged from reader/content input.
    ///
    /// # Returns
    /// Maximum accepted stream size before staging is rejected.
    #[must_use]
    #[inline]
    pub fn max_staging_size(&self) -> u64 {
        self.max_staging_size
    }

    /// Gets the configured working directory.
    ///
    /// # Returns
    /// Configured path when present, or `None` when FFprobe uses the process
    /// working directory.
    #[inline]
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    /// Replaces the maximum bytes staged from reader/content input.
    ///
    /// # Parameters
    /// - `max_staging_size`: Maximum accepted stream size.
    #[inline]
    pub fn set_max_staging_size(&mut self, max_staging_size: u64) {
        self.max_staging_size = max_staging_size;
    }

    /// Replaces the command runner used by this classifier.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    #[inline]
    pub fn set_command_runner(&mut self, command_runner: CommandRunner) {
        self.command_runner = command_runner;
    }

    /// Sets the working directory used to execute FFprobe, or clears the
    /// override so the process working directory is used.
    ///
    /// # Parameters
    /// - `working_directory`: Path override, or `None` to clear the override.
    #[inline]
    pub fn set_working_directory(&mut self, working_directory: Option<String>) {
        self.working_directory = working_directory;
    }

    /// Replaces the maximum staging size and returns the updated classifier.
    ///
    /// # Parameters
    /// - `max_staging_size`: Maximum accepted stream size.
    ///
    /// # Returns
    /// The updated classifier.
    #[inline]
    #[must_use]
    pub fn with_max_staging_size(mut self, max_staging_size: u64) -> Self {
        self.max_staging_size = max_staging_size;
        self
    }

    /// Replaces the command runner and returns the updated classifier.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    ///
    /// # Returns
    /// The updated classifier.
    #[inline]
    #[must_use]
    pub fn with_command_runner(mut self, command_runner: CommandRunner) -> Self {
        self.command_runner = command_runner;
        self
    }

    /// Classifies FFprobe `codec_type` output.
    ///
    /// # Parameters
    /// - `output`: Lines printed by `ffprobe -show_entries stream=codec_type`.
    ///
    /// # Returns
    /// Media stream classification.
    #[must_use]
    #[inline]
    pub fn classify_stream_listing(output: &str) -> MediaStreamType {
        let has_video = output.lines().any(|line| line.trim() == Self::VIDEO_STREAM);
        let has_audio = output.lines().any(|line| line.trim() == Self::AUDIO_STREAM);
        match (has_video, has_audio) {
            (true, true) => MediaStreamType::VideoWithAudio,
            (true, false) => MediaStreamType::VideoOnly,
            (false, true) => MediaStreamType::AudioOnly,
            (false, false) => MediaStreamType::None,
        }
    }

    /// Checks whether the `ffprobe` command is available.
    ///
    /// Availability is checked by executing `ffprobe -version` with the default
    /// quiet command runner. The result only describes whether the command can
    /// be started successfully; a particular media file may still be unreadable
    /// or unsupported.
    ///
    /// # Returns
    /// `true` when `ffprobe -version` executes successfully.
    #[must_use]
    pub fn is_available() -> bool {
        let config = MimeConfig::default();
        Self::is_available_with_config(&config)
    }

    /// Checks availability using the supplied MIME command budgets.
    ///
    /// # Parameters
    /// - `config`: MIME command budgets used for the probe.
    ///
    /// # Returns
    /// `true` when the `ffprobe` availability probe succeeds.
    pub(crate) fn is_available_with_config(config: &MimeConfig) -> bool {
        Self::is_available_with_executor(config, &SystemMimeCommandExecutor)
    }

    /// Runs the availability probe through an injectable command executor.
    ///
    /// # Parameters
    /// - `config`: MIME command budgets used for the probe.
    /// - `executor`: Command executor that runs the availability probe.
    ///
    /// # Returns
    /// `true` when the probe succeeds and its output is complete.
    pub(crate) fn is_available_with_executor(config: &MimeConfig, executor: &dyn MimeCommandExecutor) -> bool {
        executor
            .run(
                &Self::default_command_runner(config),
                Command::new(Self::COMMAND).arg("-version"),
            )
            .is_ok_and(|output| output.exit_code == Some(0) && require_complete_stdout(&output).is_ok())
    }

    /// Executes FFprobe for one local file.
    ///
    /// # Parameters
    /// - `path`: Local file path.
    ///
    /// # Returns
    /// Media stream classification. Non-zero FFprobe status is treated as
    /// [`MediaStreamType::None`] because stream refinement is best-effort.
    ///
    /// # Errors
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when process
    /// execution itself fails.
    fn classify_with_ffprobe(&self, path: &Path) -> MimeResult<MediaStreamType> {
        self.classify_with_executor(path, &SystemMimeCommandExecutor)
    }

    /// Executes a local probe and interprets its actual exit status and stdout.
    ///
    /// # Parameters
    /// - `path`: Input path passed as a sensitive structured argument.
    /// - `executor`: Private execution boundary using this classifier's runner.
    ///
    /// # Returns
    /// Classification from complete UTF-8 stdout, or None for numeric nonzero
    /// exit.
    ///
    /// # Errors
    /// Propagates execution errors. Signal termination, truncated/incomplete
    /// stdout and invalid UTF-8 produce ClassifierBackend errors without
    /// captured data.
    pub(crate) fn classify_with_executor(
        &self,
        path: &Path,
        executor: &dyn MimeCommandExecutor,
    ) -> MimeResult<MediaStreamType> {
        let absolute_path = std::path::absolute(path)?;
        let mut command = Self::command_for_path(&absolute_path);
        if let Some(working_directory) = &self.working_directory {
            command = command.working_directory(working_directory);
        }
        let output = executor.run(&self.command_runner, command)?;
        match output.exit_code {
            Some(0) => {
                let stdout = require_complete_stdout(&output).map_err(|reason| MimeError::ClassifierBackend {
                    backend: Self::COMMAND.to_owned(),
                    reason: reason.to_owned(),
                })?;
                Ok(Self::classify_stream_listing(stdout))
            }
            Some(_) => Ok(MediaStreamType::None),
            None => Err(MimeError::ClassifierBackend {
                backend: Self::COMMAND.to_owned(),
                reason: "ffprobe exited without a numeric exit code".to_owned(),
            }),
        }
    }

    /// Creates the default command runner for FFprobe classification.
    ///
    /// Timeout comes from `config.command_timeout()`.
    ///
    /// # Parameters
    /// - `config`: MIME settings that supply the timeout and output limits.
    ///
    /// # Returns
    /// Runner used by the default classifier.
    fn default_command_runner(config: &MimeConfig) -> CommandRunner {
        CommandRunner::new(config.command_timeout())
            .disable_logging(true)
            .bounded_output(config.command_output_max_bytes())
            .max_total_output_bytes(config.command_total_output_max_bytes())
    }

    /// Builds the structured `ffprobe` command for one path.
    ///
    /// # Parameters
    /// - `path`: Local file path passed as an argument without shell parsing.
    ///
    /// # Returns
    /// Structured command description.
    fn command_for_path(path: &Path) -> Command {
        Command::new(Self::COMMAND)
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("stream=codec_type")
            .arg("-of")
            .arg("csv=p=0")
            .arg("-i")
            .sensitive_arg_os(path)
    }
}

impl Default for FfprobeCommandMediaStreamClassifier {
    /// Creates the default classifier.
    fn default() -> Self {
        Self::new()
    }
}

impl FileBasedMediaStreamClassifier for FfprobeCommandMediaStreamClassifier {
    /// Gets the maximum bytes staged from reader/content input.
    ///
    /// # Returns
    /// Maximum accepted stream size before staging is rejected.
    fn max_staging_size(&self) -> u64 {
        self.max_staging_size
    }

    /// Classifies a readable local media file using FFprobe.
    ///
    /// # Parameters
    /// - `file`: Readable local media file to inspect.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when process
    /// execution itself fails.
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.classify_with_ffprobe(file)
    }
}

impl AsyncMediaStreamClassifierBackend for FfprobeCommandMediaStreamClassifier {
    /// Classifies media through the managed FFprobe path within an operation.
    ///
    /// # Parameters
    /// - `scope`: Operation scope that owns the staged input and command.
    /// - `input`: Borrowed complete input or provider reference to stage.
    /// - `options`: Per-operation staging limits and detection options.
    ///
    /// # Returns
    /// A future for the media stream classification.
    ///
    /// # Errors
    /// Returns an error when services are unavailable, the input is incomplete,
    /// staging exceeds its budget, or command execution fails.
    fn classify_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MediaStreamType> {
        Box::pin(async move {
            let services = self
                .execution_services
                .as_ref()
                .ok_or(MimeError::ExecutionServicesRequired)?;
            let max_staging_size = options
                .max_staging_bytes
                .unwrap_or(self.max_staging_size)
                .min(self.max_staging_size);
            if max_staging_size == 0 {
                return Err(MimeError::InvalidOperationBudget);
            }
            let (borrowed_bytes, owned_input, read_tree) = match input {
                BorrowedMimeInput::Bytes {
                    content,
                    extent,
                    filename,
                } => {
                    if extent != InputExtent::Complete {
                        return Err(MimeError::CompleteContentRequired);
                    }
                    let _filename = filename;
                    (Some((content, extent)), None, services.new_read_tree(0))
                }
                BorrowedMimeInput::Provider { file_system, path } => {
                    let read_tree = services.read_tree_for_operation(scope.id())?;
                    (
                        None,
                        Some(OwnedMimeInput::Provider {
                            file_system: Arc::new(file_system.clone()),
                            path: path.clone(),
                        }),
                        read_tree,
                    )
                }
            };
            let worker = MimeExecutionWorker::new(Arc::clone(&services.execution));
            let limits = ManagedStagingLimits {
                max_bytes: max_staging_size,
                budget: services.staging_budget(),
                worker: worker.clone(),
            };
            let staged = if let Some((content, extent)) = borrowed_bytes {
                stage_borrowed_bytes_in(scope, content, extent, limits).await?
            } else {
                stage_owned_in(
                    scope,
                    owned_input.expect("provider inputs produce an owned handle"),
                    limits,
                    read_tree,
                )
                .await?
            };
            let mut command = Self::command_for_path(&staged.path()?);
            if let Some(working_directory) = &self.working_directory {
                command = command.working_directory(working_directory);
            }
            let managed_executor = ManagedCommandExecutor::new(worker);
            let executor: &dyn AsyncMimeCommandExecutor = self.async_executor.as_deref().unwrap_or(&managed_executor);
            let output = executor
                .run_in(
                    scope,
                    ManagedMimeCommand::new(self.command_runner.clone(), command, services.root_group.clone()),
                    staged,
                )
                .await?;
            stream_type_from_output(&output)
        })
    }
}

#[cfg(test)]
mod async_operation_tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Condvar;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_fs::AsyncFileSystem;
    use qubit_fs::FsResult;
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
    use qubit_io::AsyncInput;
    use qubit_operation::ResourceRef;

    use super::FfprobeCommandMediaStreamClassifier;
    use crate::AsyncMediaStreamClassifierBackend;
    use crate::BorrowedMimeInput;
    use crate::InputExtent;
    use crate::MediaStreamType;
    use crate::MimeConfig;
    use crate::MimeError;
    use crate::MimeExecutionPort;
    use crate::MimeFuture;
    use crate::MimeOperationOptions;
    use crate::command_execution::AsyncMimeCommandExecutor;
    use crate::command_execution::ManagedMimeCommand;
    use crate::command_execution::MimeCommandOutput;
    use crate::detector::MimeExecutionServices;
    use crate::managed_temp_staging::StagedMimeInput;

    #[derive(Debug)]
    struct InlinePort;
    impl MimeExecutionPort for InlinePort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }
        fn try_dispatch(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
            job();
            Ok(())
        }
        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeCommandExecutor {
        calls: AtomicUsize,
        path: Mutex<Option<std::path::PathBuf>>,
        staged_len: Mutex<Option<u64>>,
        blocker: Mutex<Option<std::path::PathBuf>>,
        gate: Option<Arc<TestGate>>,
        fail: bool,
    }
    impl AsyncMimeCommandExecutor for FakeCommandExecutor {
        fn run_in<'a>(
            &'a self,
            _scope: &'a qubit_operation::OperationScope,
            _command: ManagedMimeCommand,
            input: ResourceRef<StagedMimeInput>,
        ) -> MimeFuture<'a, MimeCommandOutput> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let path = input.path().expect("staged path should be available");
            *self.staged_len.lock().unwrap() = std::fs::metadata(&path).ok().map(|metadata| metadata.len());
            if self.fail {
                let blocker = path.parent().unwrap().join("refinement-cleanup-blocker");
                std::fs::write(&blocker, b"retain temp owner").unwrap();
                *self.blocker.lock().unwrap() = Some(blocker);
            }
            *self.path.lock().unwrap() = Some(path);
            let gate = self.gate.clone();
            let fail = self.fail;
            Box::pin(async move {
                if let Some(gate) = gate {
                    TestGateFuture(gate).await;
                }
                let output = MimeCommandOutput {
                    exit_code: Some(0),
                    stdout: b"audio\n".to_vec(),
                    stdout_truncated: false,
                    stdout_complete: true,
                };
                drop(input);
                if fail {
                    Err(MimeError::BudgetExceeded {
                        kind: "staging",
                        requested: 9,
                        limit: 8,
                    })
                } else {
                    Ok(output)
                }
            })
        }
    }

    struct TestProvider {
        properties: ProviderProperties,
        content: Vec<u8>,
    }
    impl TestProvider {
        fn new(content: &[u8]) -> Arc<AsyncFileSystem> {
            let properties = ProviderProperties::new(
                FileSystemInfo::new(
                    FileSystemId::new("refinement-test").unwrap(),
                    "refinement-test",
                    PathSemantics::Hierarchical,
                ),
                ProviderOperations::new().with(ProviderOperation::OpenReader),
                FileSystemCapabilities::new().with_guaranteed(qubit_fs::metadata::FileSystemCapability::Read),
                FileSystemLimits::unknown(),
                PathConstraints::absolute(),
                SymlinkPolicy::Reject,
            )
            .unwrap();
            Arc::new(
                AsyncFileSystem::from_spi(Self {
                    properties,
                    content: content.to_vec(),
                })
                .unwrap(),
            )
        }
    }
    impl AsyncFileSystemSpi for TestProvider {
        fn properties(&self) -> ProviderProperties {
            self.properties.clone()
        }

        fn stat<'a>(&'a self, _request: StatRequest<'a>) -> SpiFuture<'a, FsResult<StatResponse>> {
            Box::pin(async { panic!("refinement staging must not stat provider input") })
        }

        fn open_reader<'a>(&'a self, request: OpenReaderRequest<'a>) -> SpiFuture<'a, FsResult<OpenedAsyncReader>> {
            let content = self.content.clone();
            Box::pin(async move {
                Ok(OpenedAsyncReader::new(
                    OpenedFileInfo::new(FileSystemId::new("refinement-test").unwrap(), request.path().clone()),
                    Box::new(ProviderBytes { content, position: 0 }),
                ))
            })
        }
    }

    struct ProviderBytes {
        content: Vec<u8>,
        position: usize,
    }
    impl AsyncInput for ProviderBytes {
        type Item = u8;
        unsafe fn poll_read_unchecked(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            output: &mut [u8],
            index: usize,
            count: usize,
        ) -> Poll<std::io::Result<usize>> {
            let read = count.min(self.content.len().saturating_sub(self.position));
            output[index..index + read].copy_from_slice(&self.content[self.position..self.position + read]);
            self.position += read;
            Poll::Ready(Ok(read))
        }
    }

    #[derive(Debug, Default)]
    struct GateState {
        started: bool,
        released: bool,
        waker: Option<Waker>,
    }
    #[derive(Debug, Default)]
    struct TestGate {
        state: Mutex<GateState>,
        changed: Condvar,
    }
    impl TestGate {
        fn wait_until_started(&self) {
            let mut state = self.state.lock().unwrap();
            while !state.started {
                state = self.changed.wait(state).unwrap();
            }
        }

        fn release(&self) {
            let mut state = self.state.lock().unwrap();
            state.released = true;
            let waker = state.waker.take();
            drop(state);
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
    struct TestGateFuture(Arc<TestGate>);
    impl Future for TestGateFuture {
        type Output = ();
        fn poll(self: std::pin::Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            let mut state = self.0.state.lock().unwrap();
            state.started = true;
            self.0.changed.notify_all();
            if state.released {
                Poll::Ready(())
            } else {
                state.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }

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
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    #[test]
    fn test_refinement_operation_ffprobe_stages_and_cleans_input() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let services = Arc::new(MimeExecutionServices::new(manager.clone(), group, Arc::new(InlinePort), 64).unwrap());
        services.prepare().unwrap();
        let command_executor = Arc::new(FakeCommandExecutor::default());
        let classifier = Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(MimeConfig::default())
                .with_execution_services(Arc::clone(&services))
                .with_async_executor(command_executor.clone()),
        );
        let content = vec![0x45; 40 * 1024];
        let mut operation = manager
            .as_async()
            .try_submit(Default::default(), move |scope| {
                let classifier = Arc::clone(&classifier);
                let content: Arc<[u8]> = content.into();
                async move {
                    classifier
                        .classify_in(
                            &scope,
                            BorrowedMimeInput::Bytes {
                                content: &content,
                                extent: InputExtent::Complete,
                                filename: None,
                            },
                            &MimeOperationOptions::default(),
                        )
                        .await
                }
            })
            .unwrap();
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Succeeded(MediaStreamType::AudioOnly)
        ));
        assert_eq!(command_executor.calls.load(Ordering::SeqCst), 1);
        assert_eq!(*command_executor.staged_len.lock().unwrap(), Some(40 * 1024));
        let _settlement = block_on(operation.wait_settlement());
        let path = command_executor.path.lock().unwrap().clone().unwrap();
        assert!(!path.exists(), "staged owner should be cleaned at settlement");
        let shutdown = services.shutdown();
        block_on(shutdown.wait_services_settled()).unwrap();
        shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_refinement_operation_zero_staging_budget_does_not_launch_command() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let services = Arc::new(MimeExecutionServices::new(manager.clone(), group, Arc::new(InlinePort), 64).unwrap());
        services.prepare().unwrap();
        let command_executor = Arc::new(FakeCommandExecutor::default());
        let classifier = Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(MimeConfig::default())
                .with_execution_services(Arc::clone(&services))
                .with_async_executor(command_executor.clone()),
        );
        let mut operation = manager
            .as_async()
            .try_submit(Default::default(), move |scope| {
                let classifier = Arc::clone(&classifier);
                async move {
                    classifier
                        .classify_in(
                            &scope,
                            BorrowedMimeInput::Bytes {
                                content: b"complete media payload",
                                extent: InputExtent::Complete,
                                filename: None,
                            },
                            &MimeOperationOptions {
                                max_staging_bytes: Some(0),
                                ..Default::default()
                            },
                        )
                        .await
                }
            })
            .unwrap();
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Failed(MimeError::InvalidOperationBudget)
        ));
        assert_eq!(command_executor.calls.load(Ordering::SeqCst), 0);
        let _settlement = block_on(operation.wait_settlement());
        let shutdown = services.shutdown();
        block_on(shutdown.wait_services_settled()).unwrap();
        shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_refinement_operation_staging_overlimit_does_not_launch_command() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let services = Arc::new(MimeExecutionServices::new(manager.clone(), group, Arc::new(InlinePort), 64).unwrap());
        services.prepare().unwrap();
        let command_executor = Arc::new(FakeCommandExecutor::default());
        let classifier = Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(MimeConfig::default())
                .with_execution_services(Arc::clone(&services))
                .with_async_executor(command_executor.clone()),
        );
        let mut operation = manager
            .as_async()
            .try_submit(Default::default(), move |scope| {
                let classifier = Arc::clone(&classifier);
                async move {
                    classifier
                        .classify_in(
                            &scope,
                            BorrowedMimeInput::Bytes {
                                content: b"complete media payload",
                                extent: InputExtent::Complete,
                                filename: None,
                            },
                            &MimeOperationOptions {
                                max_staging_bytes: Some(8),
                                ..Default::default()
                            },
                        )
                        .await
                }
            })
            .unwrap();
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Failed(MimeError::BudgetExceeded { kind: "staging", .. })
        ));
        assert_eq!(command_executor.calls.load(Ordering::SeqCst), 0);
        let _settlement = block_on(operation.wait_settlement());
        let shutdown = services.shutdown();
        block_on(shutdown.wait_services_settled()).unwrap();
        shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_refinement_operation_stop_retains_staged_input_until_command_returns() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let services = Arc::new(MimeExecutionServices::new(manager.clone(), group, Arc::new(InlinePort), 64).unwrap());
        services.prepare().unwrap();
        let gate = Arc::new(TestGate::default());
        let command_executor = Arc::new(FakeCommandExecutor {
            calls: AtomicUsize::new(0),
            path: Mutex::new(None),
            staged_len: Mutex::new(None),
            blocker: Mutex::new(None),
            gate: Some(Arc::clone(&gate)),
            fail: false,
        });
        let classifier = Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(MimeConfig::default())
                .with_execution_services(Arc::clone(&services))
                .with_async_executor(command_executor.clone()),
        );
        let content = vec![0x45; 40 * 1024];
        let budget = services.staging_budget();
        let mut operation = manager
            .as_async()
            .try_submit(Default::default(), move |scope| {
                let classifier = Arc::clone(&classifier);
                let content: Arc<[u8]> = content.into();
                async move {
                    classifier
                        .classify_in(
                            &scope,
                            BorrowedMimeInput::Bytes {
                                content: &content,
                                extent: InputExtent::Complete,
                                filename: None,
                            },
                            &MimeOperationOptions::default(),
                        )
                        .await
                }
            })
            .unwrap();
        gate.wait_until_started();
        let path = command_executor.path.lock().unwrap().clone().unwrap();
        assert!(path.exists(), "active command must retain the staged input");
        assert_eq!(budget.used_bytes(), 40 * 1024);
        let _stop = operation.request_stop(qubit_operation::StopReason::UserCancelled);
        assert!(path.exists(), "stop cannot release input before command return");
        assert_eq!(budget.used_bytes(), 40 * 1024);
        gate.release();
        let _outcome = block_on(operation.wait_outcome()).unwrap();
        let _settlement = block_on(operation.wait_settlement());
        assert!(!path.exists(), "staged input should clean after command returns");
        assert_eq!(budget.used_bytes(), 0);
        let shutdown = services.shutdown();
        block_on(shutdown.wait_services_settled()).unwrap();
        shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_refinement_operation_reuses_staging_budget() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let services = Arc::new(MimeExecutionServices::new(manager.clone(), group, Arc::new(InlinePort), 64).unwrap());
        services.prepare().unwrap();
        let command_executor = Arc::new(FakeCommandExecutor::default());
        let classifier = Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(MimeConfig::default())
                .with_execution_services(Arc::clone(&services))
                .with_async_executor(command_executor.clone()),
        );
        let file_system = TestProvider::new(b"abcd");
        let path = qubit_fs::Path::parse("/sample.ogg").unwrap();
        let tree = services.new_read_tree(8);
        let body_tree = Arc::clone(&tree);
        let body_services = Arc::clone(&services);
        let mut operation = manager
            .as_async()
            .try_submit(Default::default(), move |scope| {
                let classifier = Arc::clone(&classifier);
                let file_system = Arc::clone(&file_system);
                async move {
                    body_services.bind_read_tree(scope.id(), &body_tree);
                    body_tree.reserve(4).unwrap().commit(4);
                    classifier
                        .classify_in(
                            &scope,
                            BorrowedMimeInput::Provider {
                                file_system: &file_system,
                                path: &path,
                            },
                            &MimeOperationOptions {
                                max_staging_bytes: Some(3),
                                ..Default::default()
                            },
                        )
                        .await
                }
            })
            .unwrap();
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Failed(MimeError::BudgetExceeded { kind: "staging", .. })
        ));
        assert_eq!(tree.remaining(), 0, "prior bytes and the sentinel share one read tree");
        assert_eq!(command_executor.calls.load(Ordering::SeqCst), 0);
        let _settlement = block_on(operation.wait_settlement());
        assert_eq!(services.staging_budget().used_bytes(), 0);
        let shutdown = services.shutdown();
        block_on(shutdown.wait_services_settled()).unwrap();
        shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_refinement_operation_double_error_retains_and_recovers_staging_owner() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let services = Arc::new(MimeExecutionServices::new(manager.clone(), group, Arc::new(InlinePort), 64).unwrap());
        services.prepare().unwrap();
        let command_executor = Arc::new(FakeCommandExecutor {
            calls: AtomicUsize::new(0),
            path: Mutex::new(None),
            staged_len: Mutex::new(None),
            blocker: Mutex::new(None),
            gate: None,
            fail: true,
        });
        let classifier = Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(MimeConfig::default())
                .with_execution_services(Arc::clone(&services))
                .with_async_executor(command_executor.clone()),
        );
        let mut operation = manager
            .as_async()
            .try_submit(Default::default(), move |scope| {
                let classifier = Arc::clone(&classifier);
                async move {
                    classifier
                        .classify_in(
                            &scope,
                            BorrowedMimeInput::Bytes {
                                content: b"primary error",
                                extent: InputExtent::Complete,
                                filename: None,
                            },
                            &MimeOperationOptions::default(),
                        )
                        .await
                }
            })
            .unwrap();
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Failed(MimeError::BudgetExceeded {
                kind: "staging",
                requested: 9,
                limit: 8
            })
        ));
        let report = block_on(operation.wait_settlement());
        assert_eq!(report.status, qubit_operation::SettlementStatus::RecoveryRequired);
        assert_eq!(report.recoveries.len(), 1);
        assert_eq!(services.staging_budget().used_bytes(), b"primary error".len() as u64);
        let blocker = command_executor.blocker.lock().unwrap().clone().unwrap();
        std::fs::remove_file(blocker).unwrap();
        let attempt = report.recoveries[0].try_retry().unwrap();
        assert!(block_on(attempt.wait_async()).failures.is_empty());
        assert_eq!(services.staging_budget().used_bytes(), 0);
        let shutdown = services.shutdown();
        block_on(shutdown.wait_services_settled()).unwrap();
        shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }
}
