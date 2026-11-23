# qubit-mime user guide

[中文用户手册](user_guide.zh_CN.md) · [README](../README.md) ·
[API documentation](https://docs.rs/qubit-mime)

This guide targets Rust applications using `qubit-mime` 0.22 to inspect
uploaded files, filesystem resources, or media streams. It explains the
supported detection paths and the operational boundaries that callers must
handle.

## Purpose and Audience

Use this crate when an application needs a MIME candidate for routing,
validation, or media handling. The crate matches a filename and/or content
against the bundled freedesktop MIME-info repository. It does not validate that
the file is safe to open or execute.

## Conceptual Model

The public API has three useful layers:

- `RepositoryMimeDetector` performs deterministic matching against the bundled
  `MimeRepository`.
- `FileCommandMimeDetector` uses the repository for filename guesses and the
  local `file --mime-type --brief` command for content guesses.
- `MimeDetectorRegistry` resolves a provider, while `MimeConfig` supplies
  selection, fallback, command, classifier, and buffer settings.

`MimeDetectionPolicy` controls combined detection. `PreferFilename` accepts a
unique definitive filename result without running content detection or media
classification. `VerifyContent` checks content even when the filename is
definitive and returns `None` if content has no candidate. Use `ContentFirst`
when content should win when available but the filename should remain a
fallback. When migrating code that relied on classification of a
filename-selected media type, choose `VerifyContent` if the content must match
or `ContentFirst` if the filename remains a fallback. If neither source
matches, the successful result is `None`; an error is reported separately.

`detect_bytes` and `detect_by_content` inspect complete content. When precise
media detection is enabled and a classifier is injected, that classifier may
inspect the full resource. Use `MimeDetector::detect_prefix` when only a prefix
is available; built-in detectors do not pass it to the classifier. Both the
configuration switch and an explicitly injected classifier are required for
precise detection.

The default `Freedesktop` policy also uses the selected repository's subtype
metadata when filename and content candidates differ. For example, if the
filename matches `text/html` and the content matches its parent `text/plain`,
the detector returns `text/html`, the more specific filename candidate. Alias
names resolve to their canonical type before this check. Unknown names are
considered equivalent only when their normalized names are equal; the detector
does not infer relationships absent from the repository. Other policies keep
their existing selection rules. If your application depends on the earlier
exact-name conflict result, select `ContentFirst` and validate the returned
type against the application's own allowlist.

## Scenario: Inspect an Upload

Suppose an upload is named `report.jpg`, but its leading bytes identify a PDF.
The success criterion is to return the content-backed MIME type rather than
trusting the misleading extension:

```rust
use qubit_mime::{MimeDetectionPolicy, MimeDetector, RepositoryMimeDetector};

fn main() -> qubit_mime::MimeResult<()> {
    let detector = RepositoryMimeDetector::new()?;
    let mime = detector.detect_prefix(
        b"%PDF-1.7\n", Some("report.jpg"), MimeDetectionPolicy::VerifyContent,
    )?;
    assert_eq!(mime.as_deref(), Some("application/pdf"));
    Ok(())
}
```

`Some("application/pdf")` is the observable result. `None` is a valid result
when neither the filename nor the inspected bytes match a candidate.

## Installation and Minimal Configuration

Add the crate to `Cargo.toml`:

```toml
[dependencies]
qubit-mime = "^0.22"
```

`RepositoryMimeDetector::new()` uses the embedded repository and needs no
external command. To select a provider explicitly, create a `Config`, parse it
into `MimeConfig`, resolve a provider, and then create the detector:

The example below uses `Config`; implementing application providers also uses `ServiceProvider`. Declare these direct dependencies for the complete provider setup:

```toml
[dependencies]
qubit-mime = "^0.22"
qubit-config = "0.15"
qubit-spi = "0.14"
```

<!-- mime-doc-example:provider-configuration -->
```rust
use std::sync::Arc;

use qubit_config::Config;
use qubit_mime::CONFIG_MIME_DETECTOR_DEFAULT;
use qubit_mime::CONFIG_MIME_DETECTOR_FALLBACKS;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeDetectorSpec;
use qubit_mime::MimeError;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;

fn create_configured<P: ServiceProvider<MimeDetectorSpec>>(
    provider: &P,
    config: &MimeConfig,
) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
    provider.create_configured(config)
}

fn configured_detector() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Config::new();
    source.set(CONFIG_MIME_DETECTOR_DEFAULT, "file")?;
    source.set(CONFIG_MIME_DETECTOR_FALLBACKS, "repository")?;

    let config = MimeConfig::from_config(&source)?;
    let registry = MimeDetectorRegistry::builtin();
    let provider = registry.resolve_selected(config.mime_detector_selection())?;
    let _detector = provider.create_configured(&config)?;
    Ok(())
}
```

The `file` provider requires the system `file` executable when content must be
inspected. The `repository` fallback remains usable when that provider is not
available and the selection policy permits fallback.

Provider selection chains are strict by default: if a selected provider ID is
not registered, resolution returns a diagnostic instead of silently skipping
that entry. When a chain intentionally includes optional providers that may be
absent, use `ProviderSelection::chain_allowing_missing`; malformed selectors
and an empty chain still fail validation.

## Environment Configuration and Fallback

`MimeConfig::load()` reads environment values and returns a configuration. The process-wide default's
lazy initializer calls this method. If any value is invalid, it discards the entire environment
configuration and uses all built-in defaults; it does not return the parsing error. For example, a
valid `QUBIT_MIME_MAX_BUFFER_SIZE=33554432` is also ignored if
`QUBIT_MIME_ENABLE_PRECISE_DETECTION=maybe` is invalid, so the buffer limit returns to its built-in
default too. This whole-configuration fallback keeps default construction available, but can hide a
mistyped setting.

The process-wide default is initialized from the environment when it is first needed. Later changes
to environment variables do not update that snapshot automatically. For strict startup validation,
call `MimeConfig::from_env()?` and let the error stop startup. If the parsed value should become the
process-wide default, call `MimeConfig::set_default(config)` only after parsing succeeds.
`MimeConfig::reload_default_from_env()?` is the explicit reload path: it replaces the current
default on success and returns an error while keeping the previous default on failure. Use
`load()` when silent fallback is desired; use `from_env()` or explicit reload when configuration
errors need to be visible.

Precise media detection is opt-in: set `mime.enable.precise.detection` to
`true` and explicitly inject a `MediaStreamClassifier` into the detector core.
Enabling the setting alone does not create or install an `ffprobe` classifier.

For a complete upload body that needs media refinement, this function enables the
setting and injects the built-in `ffprobe` classifier before inspecting the full
content. Add these direct dependencies using the versions already used in this
guide:

```toml
[dependencies]
qubit-mime = "^0.22"
qubit-config = "0.15"
qubit-spi = "0.14"
```

Bring `qubit_spi::ServiceProvider` into scope for the provider's
`create_configured` method.

<!-- mime-doc-example:precise-detection -->
```rust
fn detect_precisely(content: &[u8], filename: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut source = qubit_config::Config::new();
    source.set(qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)?;
    let config = qubit_mime::MimeConfig::from_config(&source)?;
    let classifier = qubit_mime::FfprobeCommandMediaStreamClassifierProvider.create_configured(&config)?;
    let mut detector = qubit_mime::RepositoryMimeDetector::from_mime_config(config);
    detector.core_mut().set_media_stream_classifier(Some(classifier));
    Ok(detector.detect_bytes(content, Some(filename), qubit_mime::MimeDetectionPolicy::VerifyContent)?)
}
```

The function returns `None` when no MIME candidate is found. Creating the
classifier checks that `ffprobe` is available; if it is missing, creation returns
an error and this function propagates it. The application can explicitly choose
to propagate that error or abandon precise classification. Do not pass a prefix
to this function: prefixes are not classified.

## Core Workflow

Use the simplest entry point that matches the input:

- `detect_by_filename("photo.JPG")` checks filename globs.
- `detect_by_content(bytes)` checks content magic rules.
- `detect_bytes(bytes, Some(filename), policy)` combines both on
  `RepositoryMimeDetector` for complete resource bytes; use `detect_prefix`
  through `MimeDetector` when only a bounded prefix is available.
- `detect_reader(reader, filename, policy)` inspects a seekable reader and
  restores its original position.
- `detect_file(path, policy)` reads a local file.
- `MimeDetector::detect_path` and `detect_async_path` read through the
  synchronous or asynchronous filesystem facade.

Filename hints use both `/` and `\` as separators, and only the final
component determines the extension. For example, `C:\cache.v1\clip` has no
extension, so precise media detection uses the existing MIME-mapping fallback.
Because this fallback can now reach refinement that a dotted directory hint
previously skipped, the classifier may run and its errors may propagate. If the
final component has an extension that is not enabled in the precise detection
patterns, refinement remains skipped. This filename interpretation does not
change how local files are opened: actual file access continues to follow the
host platform's path semantics.

The top-level `MimeDetector::detect_reader` treats the reader as a whole
resource: detection starts at logical byte zero even when the caller's cursor
is elsewhere, then restores the original position. The default
`MimeContentBackend::detect_reader` has a different contract and reads only
from the current position. `Prefix(n)` reads at most `n` bytes and stops early
at EOF. `Complete` reads through EOF within the default 16 MiB buffer budget,
or an overridden budget, and probes one extra byte to detect overflow. Both
content-backend modes restore their starting position; if reading and
restoration both fail, the restoration error takes precedence.


### Local path behavior varies by provider

`detect_file` does not impose one symbolic-link policy across providers. The
repository detector opens the path as a file and follows a valid link to its
target; a dangling link or directory produces an open/read error. The default
`file` runner passes the path to the system command without `-L`, so links,
dangling links, and directories follow the installed command's path-object
semantics. This can vary by platform, `file` version, and environment; an
injected executor defines its own behavior. Applications needing consistent
content semantics should apply their own link policy while opening an ordinary
file, then use reader detection. Rejecting links is the application's path
opening policy; a separate metadata check alone does not eliminate races.

These are separate backend contracts. The default `MimeDetectorBackend`
reader hook reads the number of bytes declared by `content_requirement()` when
it is `Prefix(n)`; `max_test_bytes()` supplies that requirement only when the
method is not overridden. It returns `CompleteContentRequired` when the backend
declares `Complete`. A backend requiring the whole resource must override
`guess_from_reader`, implement bounded reading and position restoration, and
override `guess_from_file` when local file handling needs special behavior.
Managed async operations select an explicit async backend; a sync-only backend
returns `AsyncUnsupported` instead of falling back to synchronous work. The
managed path requires configured execution services and finite budgets, and
separates business outcome from resource settlement. Dropping an in-flight
future does not prove that accepted work has stopped. `MimeDetectorAdapter`
delegates to its reader hook through a view whose offset zero is the caller's
current position; a custom random-access override remains supported. The adapter uses
`MimeConfig::max_buffer_size()` for prefix requests. If no filename is supplied
and matching precise-detection mappings associate a MIME type with different
pairs, refinement preserves the original MIME type without classification.
When a filename extension exists, it must be listed in
`MimeConfig::precise_detection_patterns()`. When no filename extension exists,
the mapping lookup ignores that list and considers every configured mapping
that contains the detected MIME type; refinement runs only if all such mappings
share the same video/audio pair. This includes both a missing filename and a
filename with no extension. A conflicting mapping can therefore prevent
unnamed refinement even when its extension is not enabled. Pass a matching
enabled filename or remove the conflicting mapping to choose a pair.
MIME type and subtype comparisons ignore ASCII letter case, but configured
video/audio output spelling is preserved. For example,
`webm:Video/WebM,Audio/WebM` matches a detected `video/webm` and returns the
configured spelling selected by the classifier. Pair equivalence is positional:
video is compared with video and audio with audio. When several mappings match
without a filename and differ only in letter case, the lexicographically
smallest extension selects the output spelling, independent of map iteration
order. Genuine pair conflicts skip classification. This can enable classifier
calls, and therefore classifier errors, for inputs that previously bypassed
refinement because of case differences.

The built-in `FileCommandMimeDetector` stages reader input up to
`MimeConfig::max_buffer_size()` and stops at EOF. This limit comes from MIME
configuration; magic offsets in the selected filename repository do not set
the amount read. `detect_by_content(bytes)` continues to receive the provided
bytes, and local-file detection continues to pass the local path to the `file`
command. A reader resource longer than the configured budget may produce a
different result because the command sees only its staged prefix. Adjust the
existing `mime.max.buffer.size` setting if needed.

All these results use `MimeResult<Option<String>>`. Handle the outer error
before interpreting the optional MIME name.

## Advanced Usage

For application-wide provider registration, use `MimeDetectorRegistry::global()`.
For tests or scoped applications, use `MimeDetectorRegistry::builtin()` or a
separate registry. Application providers implement `ProviderMetadata` and
`ServiceProvider<MimeDetectorSpec>`; `examples/custom_provider.rs` shows the
complete registration and resolution path.

Register synchronous and asynchronous providers, then call `seal()` when
startup configuration is complete. Sealing is shared by registry clones and
rejects later registrations through either API while keeping existing
providers available. Async creation snapshots the selected provider before
calling its factory, so callbacks run without the async catalog lock; a
provider registered during a callback is considered by later creations.

Link-time provider discovery is opt-in. An application can enable it with:

```toml
[dependencies]
qubit-mime = { version = "^0.22", features = ["inventory"] }
qubit-magika = { version = "0.18", features = ["inventory"] }
```

Reference the provider crate in application code (`use qubit_magika as _;`) so
its submission is linked. `MimeDetectorRegistry::builtin()` then discovers the
submitted `magika` provider, while its default remains `repository`. The
classifier registry likewise discovers linked submissions and retains
`ffprobe` as its default. Provider crates submit factories through
`qubit_spi::submit_sync_provider!` using the detector or classifier inventory
`Entry` exposed under `qubit_mime::detector::mime_detector_inventory` or
`qubit_mime::classifier::media_stream_classifier_inventory`. Duplicate submitted
selectors cause inventory construction to fail. With the feature disabled,
register providers explicitly as shown above.

For richer inspection, use `MimeRepository` directly to access `MimeType`
metadata, aliases, comments, filename extensions, magic rules, and super-type
relationships. When media extensions are ambiguous, configure the media stream
classifier settings and use `FfprobeCommandMediaStreamClassifier` if `ffprobe`
is available.

When loading shared MIME-info XML, the root and standard rule elements must use
the `http://www.freedesktop.org/standards/shared-mime-info` namespace. Elements
from other namespaces are treated as extensions and ignored, including their
nested elements; malformed attributes on standard elements still produce an
error. A standard `magic` element must still contain at least one standard
`match` element.

Nested magic matchers are limited to 128 levels, counting the root matcher as
level one. XML rules deeper than this return `InvalidXmlAttribute`, and
programmatically constructed matchers return `InvalidMagicMatcher`. Reduce
redundant nesting or rewrite a rule as equivalent shallower alternatives while
preserving its matching behavior.

MIME super-type inheritance is traversed iteratively, so a long inheritance
chain does not consume one call-stack frame per type. Traversal storage grows
with the visited graph; this is separate from the 128-level nested-matcher
limit above.

## Precise mapping configuration errors

Each `mime.ambiguous.mime.mapping` entry is validated while `MimeConfig` is
parsed. Both configured MIME names must use the accepted `type/subtype` token
syntax. For example, `webm:video/webm,garbage` fails immediately because the
audio-side value has no slash; malformed values are not deferred until a
classifier reports an audio-only stream. The error is
`MimeError::InvalidConfigurationValue`, with key
`CONFIG_MIME_AMBIGUOUS_MIME_MAPPING`, the complete original entry as its value,
and a reason naming the invalid side and syntax problem. Valid custom names do
not need to appear in the embedded repository or start with `video/` or
`audio/`.

## Managed Detection Operations (0.22.0 Target)

`MimeRuntime` adds an opt-in managed submission path for applications that already
use `qubit-operation`. It accepts owned byte inputs with an explicit `Prefix` or
`Complete` extent. Construct `MimeExecutionServices` with the application's
`OperationManager`, a MIME operation group, a `MimeExecutionPort`, and a finite,
nonzero cumulative read limit; attach it with `MimeRuntime::with_execution_services`
and call `prepare()` before root submissions. Preparation is explicit.
While `prepare()` is running, root and child submissions are rejected; readiness
is published only after the execution port returns success. Wait for
`prepare()` to return `Ok(())` before submitting work.

The following function is copied from the runnable
[`managed_lifecycle` example](../examples/managed_lifecycle.rs). The example's
in-memory provider keeps the run deterministic; an application passes its own
`AsyncFileSystem` facade and retains its recovery store and retry queue.

<!-- mime-doc-example:managed-lifecycle -->
```rust
fn managed_lifecycle(
    file_system: Arc<AsyncFileSystem>,
    recovery_store: &RecoveryStore,
    retry_queue: &mut Vec<OwnedMimeInput>,
) {
    let manager = OperationManager::new(ManagerOptions::default()).expect("operation manager");
    let group = manager
        .group(GroupOptions { capacity: 8 })
        .expect("MIME operation group");
    let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 8)), 1024 * 1024)
        .expect("execution services");
    services.prepare().expect("prepare MIME services");

    let runtime = MimeRuntime::builtin()
        .expect("built-in repository")
        .with_execution_services(services.clone());
    let input = OwnedMimeInput::Provider {
        file_system,
        path: Path::parse("/upload/report.pdf").expect("provider path"),
    };
    let mut operation = match runtime.try_submit_detection(input, MimeOperationOptions::default()) {
        Ok(operation) => operation,
        Err(failure) => {
            eprintln!("submission rejected: {:?}", failure.reason);
            retry_queue.push(failure.input);
            let shutdown = services.shutdown();
            let settled = block_on(shutdown.wait_services_settled()).expect("settle service operations");
            assert!(settled.operations_settled);
            let stopped = shutdown.stop_services().expect("stop MIME services");
            assert!(stopped.services_stopped);
            let report = block_on(shutdown.wait_group_shutdown()).expect("drain MIME group");
            assert_eq!(report.snapshot.active_operations, 0);
            return;
        }
    };

    let outcome = block_on(operation.wait_outcome()).expect("observe MIME outcome");
    if let OperationOutcome::Succeeded(report) = outcome {
        println!(
            "{}: {:?} ({:?}, {} bytes read)",
            report.backend_id, report.mime_type, report.inspected_extent, report.budget.bytes_read,
        );
    }

    let settlement = block_on(operation.wait_settlement());
    assert!(settlement.all_released(), "operation resources should be settled");
    drop(operation);

    let shutdown = services.shutdown();
    let settled = block_on(shutdown.wait_services_settled()).expect("settle tracked operations");
    assert!(settled.operations_settled);
    for recovery in shutdown.recovery_handles_snapshot() {
        recovery
            .transfer_to(recovery_store)
            .expect("transfer unresolved cleanup to the application recovery store");
    }
    let stopped = shutdown
        .stop_services()
        .expect("stop MIME services from this sync context");
    assert!(stopped.services_stopped && stopped.unresolved_recoveries == 0);
    let report = block_on(shutdown.wait_group_shutdown()).expect("wait for MIME group Drain");
    assert_eq!(report.snapshot.active_operations, 0);
}
```

The submission branch returns rejected input to the caller's retry queue. For
accepted work, the example observes the MIME report and operation settlement
separately, then drops the typed handle before waiting for group Drain. Recovery
handles are transferred to the application's long-lived store before workers
stop. Run the complete example with `cargo run --example managed_lifecycle`.

Select an async-capable provider with `MimeRuntime::with_async_provider_selection`
when the runtime needs an async implementation. The built-in `repository` and
`file` providers both expose managed async backends. `file` requires complete
content; its lack of random-access support only rules out inputs that require
random access. Provider registration itself does not start detection work. A
runtime without execution services returns a typed
`ExecutionServicesRequired` submission rejection; an unsupported async backend
returns `AsyncUnsupported`. Both rejections return the original
`OwnedMimeInput` in `MimeSubmitFailure`, so the caller can retry or choose another
path. Once accepted, detection failures are reported by the operation handle.

For the managed `repository` provider, an input marked `Prefix` is used only for
base MIME matching. It is not passed to an async media classifier, even when a
precise-detection mapping matches. Submit the complete bytes with extent
`Complete` when the result needs media-stream refinement; classifier errors for
complete inputs continue to be reported by the operation.

### Metered provider reads for extension backends

Update async extension backends that match `BorrowedMimeInput` exhaustively:
the enum now includes `ManagedProvider` and is no longer `Copy` or `Clone`. For
this variant, the runtime opens the provider object once and lends the backend a
`MeteredReadSession`. Use `session.read()` for sequential reads and
`session.read_at()` for samples from that same opened object. Do not reopen the
path or use a raw filesystem handle for a managed input.

`uses_managed_provider_reads()` opts a backend into this session. It defaults to
`true` when `random_access_capability()` returns
`RandomAccessCapability::SamplesSameObject`. A backend that needs complete
sequential provider input can override it to return `true` while declaring
`RandomAccessCapability::Unsupported`. A `Prefix(n)` backend on the regular
bounded-prefix route receives `Bytes` containing at most `n` bytes, further
limited by request and service buffer/read budgets; the provider may return
fewer bytes at EOF. No read goes past the prefix bound. A backend that opts into
managed reads controls its own reads and should keep them within its declared
extent.

Each session read is clipped to the caller's buffer, the payload limit, the
remaining request and ancestor read allowances, and an 8 KiB chunk. The runtime
reserves an 8 KiB maximum private scratch buffer before provider I/O and copies
only successfully returned bytes into the backend buffer. Provider errors do
not expose partial scratch data. EOF returns zero, and read accounting charges
actual returned bytes; repeated or overlapping `read_at` samples are charged
again. This per-read scratch bound is distinct from a backend's retained input
or command staging memory.

The handle exposes two independent observations. `wait_outcome()` returns the
MIME detection result (including `MimeDetectionReport` on success);
`wait_settlement()` observes release or recovery responsibility. An early stopped
or failed outcome does not by itself prove that staged resources or background
helpers have been released. Do not interpret dropping a handle or future as proof
of cancellation. The operation report contains the selected MIME value, backend
identifier, inspected extent, and budget summary; it does not replace operation
settlement diagnostics. `InputExtent` describes whether the caller supplied a
prefix or complete input. `InspectedExtent::Prefix(n)` describes the bounded
contiguous window made available to matching; it is not a byte-by-byte access
counter. A complete input can therefore have a prefix inspected extent.

Use `try_submit_detection_in` to submit beneath an existing
`qubit-operation::OperationScope`; the child uses the parent's manager view and
MIME group binding. The synchronous child entry point is
`try_submit_detection_sync_in`. Provider capabilities are:

All MIME children submitted beneath the same external parent share one
cumulative provider-read budget for that parent's lifetime, including
sequential children after earlier handles have settled and been dropped. The
first child fixes the tree limit to the smaller of its request limit and the
service limit; later children cannot enlarge it. MIME retains one read-budget
anchor in the parent resource scope, so the parent needs one available resource
entry. If that registration fails because the scope is full, stopped, or ended,
submission returns `ReadBudgetBinding` with the resource error kind and the
original input. Independent root operations and distinct parents have separate
budgets.

| Provider | Managed async backend | Input and refinement limits |
| --- | --- | --- |
| `repository` | Yes | Matches supplied bytes against repository rules; does not support random-access input. |
| `file` | Yes, after explicit selection | Requires complete content and does not support random-access input. Attaching a classifier that needs same-object refinement is rejected with `UnsupportedCapability`. |
| Sync-only custom provider | No | Async selection returns `AsyncUnsupported`; it is not run synchronously as a fallback. |

Selecting a command provider does not guarantee the external command is
available or will run successfully. Those are runtime conditions reported by
the accepted operation. A classifier also needs explicit async selection and a
compatible same-object capability; do not assume `ffprobe` is available
through every detector selection.

`MimeExecutionServices::shutdown()` closes new root submissions and returns a
repeatable observer. Call `wait_services_settled()` to observe tracked
operations. If recovery is required, use `recovery_handles_snapshot()` to
retrieve the unresolved group handles, then resolve or transfer them before
stopping services. Call `stop_services()` from an external synchronous context,
then observe group shutdown with `wait_group_shutdown()`. A pending settlement
is unresolved work, not a release confirmation; group Drain can remain pending
while callers retain typed handles. `stop_services()` returns
`OperationShutdownPending` while operations or recovery remain unresolved and
does not retry recovery automatically. The 0.22.0 target remains under acceptance; see the
[acceptance record](acceptance-0.22.0-operation-0.2.zh_CN.md) for tested behavior
and outstanding lifecycle checks.

`FixedMimeExecutionPort::shutdown()` must be called from a synchronous context
outside its worker pool. Concurrent shutdown calls wait for the same workers to
exit and observe the same final result.

## Errors and Diagnostics

Important `MimeError` cases include:

- `Io` or `FileSystem` when an input cannot be read;
- `BufferLimitExceeded` when a path prefix request exceeds
  `MimeConfig::max_buffer_size()`;
- `CompleteContentRequired` when a backend requires the complete resource;
- `AsyncUnsupported` when a backend has no asynchronous content hook;
- `Command`, `DetectorUnavailable`, or `DetectorBackend` for native provider
  availability and execution failures;
- `Config` or `InvalidConfigurationValue` for invalid settings.
- `InvalidXmlAttribute` when a filename glob cannot be compiled, including
  patterns rejected by the regex size limit.

Log or inspect the concrete error before switching to a fallback. A successful
`None` is not an operational failure; it means that no candidate matched.

## Troubleshooting

If detection returns `None`, call the filename-only and content-only entry
points separately to see which source lacks a candidate. If `file` or `ffprobe`
fails, verify that the executable is on `PATH`, then inspect the returned
`MimeError` and command configuration. If a filesystem call fails, verify the
provider-neutral path and filesystem facade before changing detection policy.

## Limitations and Best Practices

`detect_path` and `detect_async_path` accept a `max_bytes` prefix bound that
must not exceed `max_buffer_size`. `ContentRequirement::Prefix` describes the
input limit available to a backend. For built-in detectors and backends using
the crate's blanket `MimeDetector` implementation, prefix detection does not
pass that prefix to a complete-media classifier; direct `MimeDetector`
implementations define their own behavior. The async path entry point reads up
to the requested prefix by default. The legacy `RepositoryMimeDetector` path
is an exception: a unique filename candidate still short-circuits under
`PreferFilename`, while content-dependent policies return `AsyncUnsupported`
before opening the provider path. Custom `MimeDetectorBackend` implementations
retain their existing behavior by default. Override
`supports_async_content_detection` to reject before IO, or provide an
asynchronous content hook when they need to inspect content. A prefix backend can inspect large
resources without loading the entire resource. A backend whose
`ContentRequirement` is `Complete` cannot use a prefix entry point and must
override `guess_from_async_provider_path` when it supports complete-resource
inspection through an asynchronous provider path. Native command detection may stage
input and is subject to command timeout, retained output, and media staging
limits. The default `file` and `ffprobe` runners also enforce a total
stdout-plus-stderr budget equal to four times
`MimeConfig::command_output_max_bytes()`. A zero per-stream limit therefore
produces a zero total budget, and any output can stop the command early with
`OutputLimitExceeded`; partial capture remains available on the command error.
`MimeDetectionReport::budget.staging_bytes` records the peak staging bytes
simultaneously charged to that request. It is not cumulative I/O or a snapshot
of storage still alive at report time, and concurrent requests have separate
peaks. The value does not replace cleanup or settlement observation. A custom
backend that manages staging outside the built-in staging budget must report
its own staging usage through its `MimeCandidates` result.
This policy belongs to these `qubit-mime` defaults and does not change the
default behavior of `rs-command` or an injected runner. Applications that
inject a `CommandRunner` can preserve an unlimited total budget with
`without_total_output_limit()` or choose a larger explicit budget with
`max_total_output_bytes(...)`.

Treat MIME detection as one input to a broader upload policy. Do not use the
returned MIME name alone as permission to open, execute, or trust content.

## Further Reading

- [README](../README.md) and [Chinese README](../README.zh_CN.md)
- [Chinese user guide](user_guide.zh_CN.md)
- [API documentation](https://docs.rs/qubit-mime)
- [Basic example](../examples/basic.rs)
- [Custom provider example](../examples/custom_provider.rs)
