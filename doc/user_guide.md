# qubit-mime user guide

[中文用户手册](user_guide.zh_CN.md) · [README](../README.md) ·
[API documentation](https://docs.rs/qubit-mime)

This guide targets Rust applications using `qubit-mime` 0.21 to inspect
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
qubit-mime = "0.21"
```

`RepositoryMimeDetector::new()` uses the embedded repository and needs no
external command. To select a provider explicitly, create a `Config`, parse it
into `MimeConfig`, resolve a provider, and then create the detector:

The example below uses `Config`; implementing application providers also uses `ServiceProvider`. Declare these direct dependencies for the complete provider setup:

```toml
[dependencies]
qubit-mime = "0.21"
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
qubit-mime = "0.21"
qubit-config = "0.15"
qubit-spi = "0.14"
```

<!-- mime-doc-example:precise-detection -->
```rust
fn detect_precisely(content: &[u8], filename: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    use qubit_spi::ServiceProvider;
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
reader hook reads only its prefix and returns `CompleteContentRequired` when the
backend declares `Complete`. A backend requiring the whole resource must
override `guess_from_reader`, implement bounded reading and position
restoration, and override `guess_from_file` when local file handling needs
special behavior. Through the asynchronous provider path, the built-in `file`
provider runs staging, command execution, and cleanup on a blocking thread pool.
A custom backend's default async content hook calls its synchronous method, so
backends that may block should override it. Dropping an in-flight future does
not guarantee cancellation of started work. Prefix reads may temporarily keep
both the provider buffer and an owned worker copy; `max_buffer_size` limits each
buffer, not their combined memory use. `MimeDetectorAdapter` delegates to that reader hook through
a view whose offset zero is the caller's current position; a custom
random-access override remains supported. The adapter uses
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

The built-in `FileCommandMimeDetector` stages reader input up to
`MimeConfig::max_buffer_size()` and stops at EOF. This limit comes from MIME
configuration; magic offsets in the selected filename repository do not set
the amount read. `detect_by_content(bytes)` continues to receive the provided
bytes, and local-file detection continues to pass the local path to the `file`
command. A reader resource longer than the configured budget may produce a
different result because the command sees only its staged prefix. This also
corrects custom repository relation merging and can increase the bytes staged
compared with earlier versions. Adjust the existing `mime.max.buffer.size`
setting if needed. No API or dependency migration is needed, and `rs-magika`
requires no migration.

All these results use `MimeResult<Option<String>>`. Handle the outer error
before interpreting the optional MIME name.

## Advanced Usage

For application-wide provider registration, use `MimeDetectorRegistry::global()`.
For tests or scoped applications, use `MimeDetectorRegistry::builtin()` or a
separate registry. Application providers implement `ProviderMetadata` and
`ServiceProvider<MimeDetectorSpec>`; `examples/custom_provider.rs` shows the
complete registration and resolution path.

Link-time provider discovery is opt-in. An application can enable it with:

```toml
[dependencies]
qubit-mime = { version = "0.21", features = ["inventory"] }
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

## Errors and Diagnostics

Important `MimeError` cases include:

- `Io` or `FileSystem` when an input cannot be read;
- `BufferLimitExceeded` when a path prefix request exceeds
  `MimeConfig::max_buffer_size()`;
- `CompleteContentRequired` when a backend requires the complete resource;
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
to the requested prefix by default. A prefix backend can inspect large resources
without loading the entire resource. A backend whose
`ContentRequirement` is `Complete` cannot use a prefix entry point and must
override `guess_from_async_provider_path` when it supports complete-resource
inspection through an asynchronous provider path. Native command detection may stage
input and is subject to command timeout, retained output, and media staging
limits. The default `file` and `ffprobe` runners also enforce a total
stdout-plus-stderr budget equal to four times
`MimeConfig::command_output_max_bytes()`. A zero per-stream limit therefore
produces a zero total budget, and any output can stop the command early with
`OutputLimitExceeded`; partial capture remains available on the command error.
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
