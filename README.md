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
qubit-mime = "0.21"
```

The crate requires Rust 1.94 or later. The default `repository` detector uses
the MIME database bundled in the crate and does not require an external command.

## Quick Start

An upload endpoint can compare the filename with the leading content bytes and
choose the content result when the two disagree:

```rust
use qubit_mime::{MimeDetectionPolicy, RepositoryMimeDetector};

fn detect_upload(content: &[u8], filename: &str) -> qubit_mime::MimeResult<Option<String>> {
    let detector = RepositoryMimeDetector::new()?;
    detector.detect_bytes(content, Some(filename), MimeDetectionPolicy::VerifyContent)
}

fn main() -> qubit_mime::MimeResult<()> {
    assert_eq!(
        detect_upload(b"%PDF-1.7\n", "report.jpg")?.as_deref(),
        Some("application/pdf"),
    );
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

With the default `Freedesktop` policy, a filename MIME type that is a subtype
of the content result is kept as the more specific result; aliases are resolved
through the selected repository. Unknown MIME names only match after
normalization when their names are equal. Applications that need the former
exact-name conflict behavior can select `ContentFirst` and apply their own
allowlist to the returned MIME type.

For built-in detectors and backends using the crate's blanket `MimeDetector`
implementation, `detect_prefix` matches filename and content candidates
without passing the prefix to a complete-media classifier. Types that implement
`MimeDetector` directly define their own behavior. The default asynchronous
Prefix path reads at most the requested byte limit; Complete-content backends
must provide their own asynchronous path hook.

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

The detector identifies MIME types; it does not establish that a file is safe
to open or execute. Native command providers also depend on the corresponding
executable and can fail when it is unavailable, times out, or returns invalid
output.

## Provider Selection and Configuration

`MimeDetectorRegistry::builtin()` exposes the built-in `repository` provider
and the `file` provider. Provider resolution and detector creation are separate:

The example below uses `Config`; applications that also implement custom providers use `ServiceProvider`. Declare these direct dependencies for that provider setup:

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
