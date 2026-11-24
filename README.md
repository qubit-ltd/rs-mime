# Qubit MIME

[![Rust CI](https://github.com/qubit-ltd/rs-mime/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-mime/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-mime/coverage-badge.json)](https://qubit-ltd.github.io/rs-mime/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-mime.svg?color=blue)](https://crates.io/crates/qubit-mime)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

`qubit-mime` helps Rust services identify the MIME type of an uploaded file or
resource from its filename, content bytes, or both. It is intended for upload
routing and media inspection where callers need a deterministic embedded
repository, an optional native `file` backend, and explicit handling of
ambiguous or unavailable detections.

For the complete setup, provider, filesystem, diagnostics, and limits walkthrough,
see the [English user guide](doc/user_guide.md). The [Chinese README](README.zh_CN.md)
and [Chinese user guide](doc/user_guide.zh_CN.md) cover the same public behavior.

## Installation

```toml
[dependencies]
qubit-mime = "^0.22"
```

The crate requires Rust 1.94 or later. The default `repository` detector uses
the MIME database bundled in the crate and does not require an external command.

## Quick Start

An upload endpoint can compare the filename with the leading content bytes and
choose the content result when the two disagree:

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

`detect_by_filename` and `detect_by_content` can be used independently.
`MimeDetectionPolicy::PreferFilename` accepts a unique definitive filename
result without running content detection or media classification;
`VerifyContent` checks content even when the filename looks definitive and
requires a content match. Use `ContentFirst` to keep the filename result as a
fallback when content has no candidate. If an application previously relied on
classification of a filename-selected media type, use `VerifyContent` when the
content must match or `ContentFirst` when the filename remains a fallback. A
successful detection returns `Some(mime)`; `None` means that no candidate
matched.

`detect_bytes` and `detect_by_content` inspect complete content and may invoke
the configured media classifier when precise detection is enabled. A prefix
must go through `MimeDetector::detect_prefix`; it never reaches the classifier
for built-in detectors. Precise detection requires both the configuration
switch and an explicitly injected classifier.

With the default `Freedesktop` policy, a filename MIME type that is a subtype
of the content result is kept as the more specific result; aliases are resolved
through the selected repository. Unknown MIME names only match after
normalization when their names are equal. Applications that need the former
exact-name conflict behavior can select `ContentFirst` and apply their own
allowlist to the returned MIME type.

For built-in detectors and backends using the crate's blanket `MimeDetector`
implementation, `detect_prefix` matches filename and content candidates
without passing the prefix to a complete-media classifier. Types that implement
`MimeDetector` directly define their own behavior. Managed async operations use
an async backend: attaching execution services defaults it to the bundled
`repository`, and another backend must be selected explicitly. Resolving a
sync-only provider factory for managed async work fails with `AsyncUnsupported`
while configuring the runtime, before any operation is submitted; it never
falls back to synchronous work.
See the user guide for the managed operation lifecycle and provider capability
limits.

The legacy `detect_async_path` entry also preserves its unique-filename
short-circuit under `PreferFilename`. The built-in `RepositoryMimeDetector`
has no asynchronous content hook, so content-based calls return
`AsyncUnsupported` before opening the provider path. Custom backends keep their
existing async hook behavior; override `supports_async_content_detection`
when they want an early rejection, or implement an async content or
provider-path hook when content inspection is required.

## What It Provides

- Freedesktop shared MIME-info names, aliases, filename globs, magic rules,
  comments, and super-type metadata from the bundled repository.
- Filename detection, content-magic detection, and combined detection through
  `RepositoryMimeDetector` and the `MimeDetector` trait.
- Local-file detection with `detect_file`, seekable-reader detection with
  `detect_reader`, and provider-neutral filesystem entry points with
  `detect_path` and `detect_async_path`.
- `MimeDetectorRegistry` and `MimeConfig` for built-in providers, application
  providers, selection, fallbacks, and resource limits.
- Managed detection operations through `MimeRuntime`, with owned byte inputs,
  typed submission rejection, and separate outcome and settlement observation.
  Attaching execution services defaults managed detection to the bundled
  `repository`; select `file` explicitly. A rejected submission returns the
  original input in `MimeSubmitFailure`. Detection errors after acceptance are
  reported by the operation outcome, and resource release is observed through
  settlement. During shutdown, a Pending result with no `snapshot.group` means
  Drain is not published yet; preparation, operation settlement, or admissions
  may still be in progress, or another stop attempt may be active. After
  settlement, `stop_services()` publishes Drain before it can return Pending
  for live service resources or recovery; the execution port remains available
  when unresolved resources or recovery cause Pending. A successful
  `stop_services()` call confirms the port stopped; check `services_stopped`
  when another stop attempt may be in progress. Complete group Drain can still
  wait while callers hold typed handles. Resolve or transfer
  recovery and release retained handles and resources before retrying or
  awaiting Drain. The library's post-stop retry path is guaranteed only for
  built-in staging recovery, whose fallback cleanup port follows the MIME
  process lifetime. The `RecoveryStore` owns transferred retries and must
  outlive them. Custom cleanup must finish before the port stops or retain
  dependencies that can run independently afterward.
  The [user guide](doc/user_guide.md#managed-detection-operations-0220)
  and runnable lifecycle example show the complete sequence.
- Optional `FileCommandMimeDetector`, which uses the system
  `file --mime-type --brief` command for content detection.
- Optional `FfprobeCommandMediaStreamClassifier` for refining ambiguous media
  streams such as audio-only and video-only WebM or Ogg inputs.

The default `MimeContentBackend::detect_reader` reads from the current position.
`Prefix(n)` reads at most `n` bytes and stops early at EOF. `Complete` reads to
EOF within its default 16 MiB budget (or an overridden budget), then probes one
extra byte to detect overflow. Both restore the starting position; a restore
error takes precedence. The `MimeDetectorBackend` default reader hook only
supports its prefix and returns `CompleteContentRequired` for `Complete`, so a
complete-content backend must override that hook. The adapter delegates to the
backend reader hook; custom random-access overrides remain supported. When
multiple no-filename mappings associate one MIME type with different pairs,
refinement preserves the original MIME type; pass a matching filename or remove
the mapping conflict to select a pair.

These input routes keep separate contracts: an ordinary backend's
`ContentRequirement::Prefix(n)` is the maximum window requested from a reader,
and ordinary automatic provider reads pass it as `InputExtent::Prefix` even
when EOF shortens it. `OwnedMimeInput::Bytes` keeps the bytes and `InputExtent`
supplied by the caller and is not charged as Provider I/O. Extension backends
receive complete or same-object random-access provider input through
`MeteredReadSession`; ordinary prefix backends can opt into that session.
Session reads share the operation's read budget. The built-in managed `file`
provider meters its own staging reads. The legacy `max_bytes` arguments on
`detect_path` and `detect_async_path` remain explicit prefix limits for those
entry points.

Managed detection uses owned inputs, explicit scopes, finite request budgets,
and separate outcome and settlement observation. Construction and provider
registration do not start workers; applications prepare and stop the MIME
execution services explicitly. Resolving a sync-only provider factory for the
managed async backend fails with `AsyncUnsupported` during runtime
configuration, before submission. The legacy synchronous `FileCommandMimeDetector` API remains
available. Dropping an in-flight future does not prove that accepted work has
stopped; observe settlement before releasing resources. Once group admission
closes, already accepted in-group parents can still dispatch required
descendants; new external scopes must have entered
before closure. A partial temporary-file creation error can retain a cleanup
owner in the same operation scope, so its error outcome alone does not confirm
cleanup. Recovery transferred to a `RecoveryStore` remains retryable after the
original MIME services stop; applications must retain and operate that store
independently. Async operations observe scope deadlines and stop requests while
backend futures are pending; repository matching responds at cooperative
checkpoints. The deadline is checked again if the backend becomes ready, before
its result is committed, so a timeout can still stop a ready success or error;
an accepted user-cancel reason is preserved in a race. Synchronous children are
not preempted and observe stop only at their cooperative checks. Work already
dispatched to command or staging workers keeps its resource owner tracked until
the job settles.

Nested MIME operations share provider-read accounting through their operation
ancestors. Inputs resolved by a unique filename and owned byte inputs do not
bind an ancestor read budget or reserve its resource slot. A descendant that
needs provider reads propagates the ancestor identity and applicable scope cap;
its first read reserves against the shared ancestor tree. If that binding cannot
be admitted or the ancestor has ended, submission is rejected with the input
intact. If the accepted operation cannot commit its pending binding, it fails
with `ReadBudgetBinding`.

Provider reads charge successfully delivered bytes. `Prefix(n)` is a maximum
window, not a minimum length: request or service buffer/read caps may shorten
it, and EOF may return fewer bytes. If submission cannot reserve the owned
prefix buffer, it returns the original input. If an accepted operation later
cannot reserve shared ancestor read allowance, its outcome reports
`MimeError::BudgetExceeded { kind: "read", .. }`. Extension backends can opt
into `MeteredReadSession`, which clips each read to available allowance and
reserves scratch memory separately. The built-in `file` provider meters its
staging reads internally. `MimeBudgetSummary.bytes_read` counts bytes delivered
by reads initiated by that request, including repeats; descendants have their
own reports, while ancestor caps still constrain combined usage.

The legacy synchronous `FileCommandMimeDetector` methods remain available
alongside the managed `file` provider. The managed provider stages input within
the operation and meters provider bytes internally. Its factory rejects any
attached async classifier with `UnsupportedCapability`, even when an input
would not invoke refinement; that configuration error occurs before submission.
The legacy asynchronous content hook returns `AsyncUnsupported`; use the
managed provider when staging, worker execution, and cleanup must be tracked.
This rule applies to the managed factory and does not remove the legacy sync API.

Path-backed filename detection extracts the native path basename before
matching; on POSIX, a backslash inside the basename remains filename data.
Non-UTF-8 local basenames are converted lossily for matching.
`OwnedMimeInput::Bytes` continues to use its caller-supplied filename.

For the built-in `file` provider, reader detection stages up to
`MimeConfig::max_buffer_size()` bytes and stops at EOF. Its budget does not
depend on magic offsets in the selected filename repository. Byte-slice and
local-file detection use different inputs: byte-slice detection checks the
bytes supplied by the caller, while the `file` provider passes the local path
to the system `file` command. A reader resource longer than its budget may
differ because only the staged prefix is checked.
Applications can adjust the existing `mime.max.buffer.size` setting. See the
[reader behavior guide](doc/user_guide.md#core-workflow) for the reader contracts
and entry-point differences.

The detector identifies MIME types; it does not establish that a file is safe
to open or execute. Native command providers also depend on the corresponding
executable and can fail when it is unavailable, times out, or returns invalid
output.

Local `detect_file` path behavior depends on the selected provider. The repository
detector follows a valid symbolic link to its target and returns an open/read
error for dangling links or directories. The default `file` runner passes the
path to the system command without `-L`; its handling of links and directories
depends on the platform, `file` version, and environment. Injected executors
define their own behavior. Applications that need consistent content semantics
should apply their own link policy while opening an ordinary file, then detect
through a reader. Rejecting links is an application path-opening policy; a
separate metadata check alone does not eliminate races.

## Provider Selection and Configuration

`MimeDetectorRegistry::builtin()` exposes the built-in `repository` provider
and the `file` provider. Provider resolution and detector creation are separate:

The example below uses `Config`; applications that also implement custom providers use `ServiceProvider`. Declare these direct dependencies for that provider setup:

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

Applications can register providers implementing `ProviderMetadata` and
`ServiceProvider<MimeDetectorSpec>`. The process-wide registry is available
through `MimeDetectorRegistry::global()`; `builtin()` creates an isolated
registry suitable for tests or scoped applications.

With the opt-in `inventory` feature, linked provider crates can submit detector
factories to `qubit_mime::detector::mime_detector_inventory::Entry` and classifier
factories to `qubit_mime::classifier::media_stream_classifier_inventory::Entry`
using `qubit_spi::submit_sync_provider!`. Both `builtin()` registries discover
those providers while keeping `repository` and `ffprobe` as their defaults.
Duplicate submitted selectors fail inventory construction. Without this feature,
providers continue to require explicit registration.

Registry-based Magika inventory consumption requires published `qubit-mime`
0.22 and `qubit-magika` 0.18 releases. The registry manifest check on
2026-11-24 found `qubit-mime` 0.22.0 unpublished, so the prerequisite was not
met then. The [fixed-source consumer guide](doc/user_guide.md#fixed-source-magika-consumer)
records exact sibling revisions, application setup, `cargo check` and `--locked`
commands, and the dynamic ONNX Runtime requirement. The [consumer fixture at the tested revision](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/src/main.rs)
uses explicit registration without `inventory`, and registry discovery plus
explicit selection with `inventory`. The [workflow at the tested revision](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/.github/workflows/magika-consumer.yml)
records the verified runtime download. The fixture [manifest](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/Cargo.toml)
and [lock file](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/Cargo.lock),
its source, and the workflow are available in the source repository; none are
included in the published crate package.

Important configuration keys include `mime.detector.default`,
`mime.detector.fallbacks`, `mime.max.buffer.size`,
`mime.command.timeout`, and `mime.command.output.max.bytes`. See
`MimeConfig` and the user guide for the full configuration surface.
`MimeConfig::load()` falls back to all built-in defaults if any environment value is invalid; see
the [environment configuration guide](doc/user_guide.md#environment-configuration-and-fallback) for
strict startup validation and explicit reload behavior.

Invalid filename glob patterns are returned as structured
`MimeError::InvalidXmlAttribute` errors when the MIME repository is loaded.

## API and Further Reading

- [English user guide](doc/user_guide.md)
- [中文用户手册](doc/user_guide.zh_CN.md)
- [Rust API documentation](https://docs.rs/qubit-mime)
- [Crate package](https://crates.io/crates/qubit-mime)
- [Chinese README](README.zh_CN.md)

## Testing

```bash
# Run tests with the default feature set
cargo test

# Run tests with all declared features
cargo test --all-features

# Project CI checks
./.infra/bin/ci-check.sh

# Check code coverage
./.infra/bin/coverage.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Please follow the Rust API guidelines, keep public
API documentation and tests current, and run `./.infra/bin/align-ci.sh` to format code and
`./.infra/bin/ci-check.sh` to satisfy CI requirements before submitting a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-mime](https://github.com/qubit-ltd/rs-mime)
