# qubit-mime user guide

[中文用户手册](user_guide.zh_CN.md) · [README](../README.md) ·
[API documentation](https://docs.rs/qubit-mime)

This guide targets Rust applications using `qubit-mime` 0.16 to inspect
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
definitive filename result without checking content. `VerifyContent` checks
content even when the filename is definitive. Other policies are available for
applications that need different filename/content precedence; see the API
documentation for their exact semantics.

## Scenario: Inspect an Upload

Suppose an upload is named `report.jpg`, but its leading bytes identify a PDF.
The success criterion is to return the content-backed MIME type rather than
trusting the misleading extension:

```rust
use qubit_mime::{MimeDetectionPolicy, RepositoryMimeDetector};

fn inspect_upload(
    filename: &str,
    content_prefix: &[u8],
) -> qubit_mime::MimeResult<Option<String>> {
    let detector = RepositoryMimeDetector::new()?;
    detector.detect_bytes(
        content_prefix,
        Some(filename),
        MimeDetectionPolicy::VerifyContent,
    )
}

fn main() -> qubit_mime::MimeResult<()> {
    assert_eq!(
        inspect_upload("report.jpg", b"%PDF-1.7\n")?.as_deref(),
        Some("application/pdf"),
    );
    Ok(())
}
```

`Some("application/pdf")` is the observable result. `None` is a valid result
when neither the filename nor the inspected bytes match a candidate.

## Installation and Minimal Configuration

Add the crate to `Cargo.toml`:

```toml
[dependencies]
qubit-mime = "0.16"
```

`RepositoryMimeDetector::new()` uses the embedded repository and needs no
external command. To select a provider explicitly, create a `Config`, parse it
into `MimeConfig`, resolve a provider, and then create the detector:

```rust
use qubit_config::Config;
use qubit_mime::{
    CONFIG_MIME_DETECTOR_DEFAULT, CONFIG_MIME_DETECTOR_FALLBACKS, MimeConfig,
    MimeDetectorRegistry,
};
use qubit_spi::ServiceProvider;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

## Core Workflow

Use the simplest entry point that matches the input:

- `detect_by_filename("photo.JPG")` checks filename globs.
- `detect_by_content(bytes)` checks content magic rules.
- `detect_bytes(bytes, Some(filename), policy)` combines both on
  `RepositoryMimeDetector`.
- `detect_reader(reader, filename, policy)` inspects a seekable reader and
  restores its original position.
- `detect_file(path, policy)` reads a local file.
- `MimeDetector::detect_path` and `detect_async_path` read through the
  synchronous or asynchronous filesystem facade.

All these results use `MimeResult<Option<String>>`. Handle the outer error
before interpreting the optional MIME name.

## Advanced Usage

For application-wide provider registration, use `MimeDetectorRegistry::global()`.
For tests or scoped applications, use `MimeDetectorRegistry::builtin()` or a
separate registry. Application providers implement `ProviderMetadata` and
`ServiceProvider<MimeDetectorSpec>`; `examples/custom_provider.rs` shows the
complete registration and resolution path.

For richer inspection, use `MimeRepository` directly to access `MimeType`
metadata, aliases, comments, filename extensions, magic rules, and super-type
relationships. When media extensions are ambiguous, configure the media stream
classifier settings and use `FfprobeCommandMediaStreamClassifier` if `ffprobe`
is available.

## Errors and Diagnostics

Important `MimeError` cases include:

- `Io` or `FileSystem` when an input cannot be read;
- `BufferLimitExceeded` when a path prefix request exceeds
  `MimeConfig::max_buffer_size()`;
- `CompleteContentRequired` when a backend requires the complete resource;
- `Command`, `DetectorUnavailable`, or `DetectorBackend` for native provider
  availability and execution failures;
- `Config` or `InvalidConfigurationValue` for invalid settings.

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
must not exceed `max_buffer_size`. A prefix backend can inspect large resources
without loading the entire resource. A backend whose `ContentRequirement` is
`Complete` cannot use a prefix entry point. Native command detection may stage
input and is subject to command timeout, retained output, and media staging
limits.

Treat MIME detection as one input to a broader upload policy. Do not use the
returned MIME name alone as permission to open, execute, or trust content.

## Further Reading

- [README](../README.md) and [Chinese README](../README.zh_CN.md)
- [Chinese user guide](user_guide.zh_CN.md)
- [API documentation](https://docs.rs/qubit-mime)
- [Basic example](../examples/basic.rs)
- [Custom provider example](../examples/custom_provider.rs)
