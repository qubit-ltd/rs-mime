# Qubit MIME

[![Rust CI](https://github.com/qubit-ltd/rs-mime/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-mime/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-mime/coverage-badge.json)](https://qubit-ltd.github.io/rs-mime/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-mime.svg?color=blue)](https://crates.io/crates/qubit-mime)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

MIME type detection utilities for Rust services based on filename glob rules
and content magic rules.

## Overview

Qubit MIME is a repository-backed MIME type detector for Rust. It uses the
freedesktop shared MIME-info data model: canonical MIME type names, aliases,
localized comments, filename globs, content magic rules, and super-type
relationships.

The public surface is organized into three layers:

- `MimeDetector`: the top-level detector trait. Use it when code should work
  with any detector implementation.
- `detector`: detector implementations and shared detector logic,
  including `MimeDetectorCore`, `MimeDetectorBackend`,
  `RepositoryMimeDetector`, and `FileCommandMimeDetector`.
- `MediaStreamClassifier`: the top-level media stream classifier trait. The
  `classifier` module provides `FfprobeCommandMediaStreamClassifier`,
  `MediaStreamClassifierBackend`, and `FileBasedMediaStreamClassifier` for
  implementing stream-backed or file-backed classifiers with less duplicated
  entry-point code.
- `MimeRepository`: the lower-level repository returning `MimeType` metadata
  and all matching candidates when callers need richer inspection.

## Design Goals

- **Freedesktop data model**: follow shared MIME-info names, aliases, glob
  rules, and magic rules.
- **Practical defaults**: ship with an embedded freedesktop MIME database.
- **Filename and content detection**: support glob-based and magic-based
  detection, independently or together.
- **Detector and classifier hierarchy**: keep MIME detection and media stream
  refinement separate with Rust ownership and error handling.
- **Predictable conflict resolution**: prefer higher glob weights, longer glob
  patterns, and higher magic priorities.
- **Rust-friendly API**: use borrowed repositories, concrete errors, and
  standard `Read + Seek` based detection.
- **Small dependency surface**: keep runtime dependencies focused and stable.

## Features

### Filename Detection

- Literal, extension, and general glob matching.
- Case-insensitive matching by default, with support for case-sensitive globs.
- Conflict resolution by glob weight and pattern length.
- Path-safe detection that only uses the final filename component.

### Content Magic Detection

- Freedesktop magic value types: `string`, `byte`, `host16`, `host32`,
  `big16`, `big32`, `little16`, and `little32`.
- Offset ranges such as `0` and `0:1024`.
- Optional masks for binary magic values.
- Nested magic matchers.
- Conflict resolution by magic priority.

### Repository Metadata

- Canonical names and aliases.
- Localized comments and descriptions.
- Preferred and complete filename extension lookup.
- Super-type metadata parsed from `sub-class-of` entries.
- Maximum byte count required by magic rules.

### Detection Entrypoints

- Top-level trait: `MimeDetector`.
- Repository detector: `RepositoryMimeDetector`.
- System command detector: `FileCommandMimeDetector`.
- Filename only: `detect_by_filename`.
- Content only: `detect_by_content`.
- Combined filename and bytes: `detect` or `detect_bytes`.
- Combined filename and reader: `detect_reader`.
- Local file path: `detect_file`.

### Media Stream Classification

- Top-level trait: `MediaStreamClassifier`.
- Stream result enum: `MediaStreamType`.
- FFprobe-backed implementation: `FfprobeCommandMediaStreamClassifier`.
- Precise refinement for ambiguous media types such as WebM and Ogg when a
  classifier is configured on `MimeDetectorCore`.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
qubit-mime = "0.9"
```

## Quick Start

### Detect from filename and content

```rust
use qubit_mime::{
    MimeError,
    MimeDetectionPolicy,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let detector = RepositoryMimeDetector::new()?;

    let by_name = detector.detect_by_filename("photo.JPG");
    assert_eq!(Some("image/jpeg".to_owned()), by_name);

    let by_content = detector.detect_by_content(b"%PDF-1.7\n");
    assert_eq!(Some("application/pdf".to_owned()), by_content);

    let combined = detector.detect_bytes(
        b"%PDF-1.7\n",
        Some("report.pdf"),
        MimeDetectionPolicy::VerifyContent,
    );
    assert_eq!(Some("application/pdf".to_owned()), combined);

    Ok(())
}
```

### Use the Rust-style `MimeDetector` trait

`MimeDetectorRegistry` first resolves a provider and that provider then creates
a shared `MimeDetector` trait object. Provider selection and `MimeConfig` are
independent, so code with neither can use both Registry and configuration
defaults.

```rust
use qubit_mime::{
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorRegistry,
};
use qubit_spi::ServiceProvider;

fn detect_upload(detector: &dyn MimeDetector, filename: &str, content: &[u8]) -> Option<String> {
    detector.detect(content, Some(filename), MimeDetectionPolicy::VerifyContent)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = MimeDetectorRegistry::global().resolve()?;
    let detector = provider.create()?;

    assert_eq!(
        Some("application/pdf".to_owned()),
        detect_upload(detector.as_ref(), "upload.bin", b"%PDF-1.7\n"),
    );
    Ok(())
}
```

### Configure global defaults with `Config`

`MimeConfig::default()` returns a snapshot of the current global default MIME
configuration. Use `MimeConfig::reload_default()` to replace it from an
`rs-config` `Config`, or `MimeConfig::reload_default_from_env()` to load from
`Config::from_env()`.

```rust
use qubit_config::Config;
use qubit_mime::{
    CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE,
    CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_DETECTOR_FALLBACKS,
    CONFIG_MIME_ENABLE_PRECISE_DETECTION,
    CONFIG_MIME_MAX_BUFFER_SIZE,
    CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
    DEFAULT_AMBIGUOUS_MIME_MAPPING,
    DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE,
    DEFAULT_MIME_MAX_BUFFER_SIZE,
    DEFAULT_PRECISE_DETECTION_PATTERNS,
    MimeConfig,
    MimeDetector,
    MimeDetectorRegistry,
};
use qubit_spi::ServiceProvider;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let original = MimeConfig::default();
    let mut config = Config::new();
    config.set(CONFIG_MIME_DETECTOR_DEFAULT, "repository")?;
    config.set(CONFIG_MIME_DETECTOR_FALLBACKS, "")?;
    config.set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")?;
    config.set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)?;
    config.set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, DEFAULT_PRECISE_DETECTION_PATTERNS)?;
    config.set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, DEFAULT_AMBIGUOUS_MIME_MAPPING)?;
    config.set(CONFIG_MIME_MAX_BUFFER_SIZE, DEFAULT_MIME_MAX_BUFFER_SIZE)?;
    config.set(CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE, DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE)?;

    MimeConfig::reload_default(&config)?;
    let registry = MimeDetectorRegistry::builtin();
    let mime_config = MimeConfig::default();
    let provider = registry.resolve_selected(mime_config.mime_detector_selection())?;
    let detector = provider.create_configured(&mime_config)?;

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf"),
    );

    MimeConfig::set_default(original);
    Ok(())
}
```

### Select detectors with registry and fallbacks

`MimeDetectorRegistry::global()` is the process-wide domain Registry. An App
can register self-described third-party providers during startup and replace
the Registry's default `ProviderSelection`. Any downstream library that later
calls `global().resolve()` observes that same App-configured state
without knowing the selected implementation.

Selection and creation are intentionally separate. `resolve_selected(selection)` and
`resolve()` return `ResolvingServiceProvider<MimeDetectorSpec>` and
report only `ProviderSelectionError`. The returned provider then supports both
`create(&MimeConfig)` and `create()`, whose failures are represented by
`ProviderCreationError`. `MimeConfig::mime_detector_selection()` remains one
optional source of an explicit selection; the Registry does not require it.

`ProviderSelection` owns the fallback policy. Unknown entries in a chain are
skipped; providers reporting unsupported or unavailable status fall through
under the default `OnAbsence` policy. `auto` orders candidates by descending
priority and then canonical provider ID. `MimeDetectorRegistry::builtin()` is
an isolated Registry useful for tests and scoped applications.

SPI types remain owned by `qubit-spi` and are not re-exported by `qubit-mime`.
Third-party providers implement both `ServiceProvider<MimeDetectorSpec>` and
`ProviderDefinition<MimeDetectorSpec>` and return their descriptor themselves.

Built-in detector selectors:

| Selector | Aliases | Behavior |
|----------|---------|----------|
| `repository` | `repository-mime-detector` | Uses the embedded freedesktop MIME repository |
| `file` | `file-command`, `file-command-mime-detector` | Uses the repository for filenames and `file --mime-type --brief` for local content |
| `auto` | - | Chooses available providers by priority, then provider id |

```rust
use qubit_config::Config;
use qubit_mime::{
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_DETECTOR_FALLBACKS,
    MimeConfig,
    MimeDetector,
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
    let detector = provider.create_configured(&config)?;

    assert_eq!(
        Some("image/png".to_owned()),
        detector.detect_by_filename("image.png"),
    );
    Ok(())
}
```

The complete App-startup/library-X scenario looks like this. The App owns
registration and default selection; library X owns only service use:

```rust
use std::sync::Arc;

use qubit_mime::{
    MimeConfig,
    MimeDetector,
    MimeDetectorRegistry,
    MimeDetectorSpec,
    RepositoryMimeDetector,
};
use qubit_spi::error::ProviderCreationError;
use qubit_spi::{
    ProviderDefinition,
    ProviderDescriptor,
    ProviderId,
    ProviderSelection,
    ServiceProvider,
};

struct AppMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for AppMimeDetectorProvider {
    fn create_configured(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderCreationError> {
        Ok(Arc::new(RepositoryMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}

impl ProviderDefinition<MimeDetectorSpec> for AppMimeDetectorProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            ProviderId::new("app-detector").expect("static ID is valid"),
        )
    }
}

// This function represents code inside independently published library X.
fn library_x_detector() -> Result<Arc<dyn MimeDetector>, Box<dyn std::error::Error>> {
    let provider = MimeDetectorRegistry::global().resolve()?;
    Ok(provider.create()?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = MimeDetectorRegistry::global();
    registry.register(AppMimeDetectorProvider)?;
    registry.set_default_selection(ProviderSelection::named("app-detector")?);

    let detector = library_x_detector()?;
    assert_eq!(
        Some("text/plain".to_owned()),
        detector.detect_by_filename("notes.txt"),
    );
    Ok(())
}
```

For an isolated provider set, create a registry directly and register providers
on it:

```rust
use qubit_mime::{
    MimeDetectorRegistry,
    RepositoryMimeDetectorProvider,
};
use qubit_spi::{ProviderSelection, ServiceProvider};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = MimeDetectorRegistry::default();
    registry.register(RepositoryMimeDetectorProvider)?;
    let selection = ProviderSelection::named("repository-mime-detector")?;
    let detector = registry.resolve_selected(&selection)?.create()?;

    assert_eq!(
        Some("text/plain".to_owned()),
        detector.detect_by_filename("notes.txt"),
    );
    Ok(())
}
```

Registry selection and service creation use separate SPI error types:

| Error | Stage | Meaning |
|-------|-------|---------|
| `RegistrationError` | Registration | A provider ID or alias conflicts with an existing provider |
| `ProviderSelectionError` | Resolution | The explicit/default selection yields no provider candidates |
| `ProviderCreationError` | Creation | Selected candidates fail or fallback policy stops traversal |

### Configuration keys

`MimeConfig::from_config()` accepts both logical keys and environment-style keys.
Environment variables use the environment-style names. List values can be arrays
or scalar strings split on `,` and `;`; empty items are ignored. Ambiguous MIME
mapping values are split on `;` as `extension:video-mime,audio-mime`.

| Setting | Logical key | Environment key | Default |
|---------|-------------|-----------------|---------|
| Default MIME detector | `mime.detector.default` | `QUBIT_MIME_DETECTOR_DEFAULT` | `repository` |
| MIME detector fallbacks | `mime.detector.fallbacks` | `QUBIT_MIME_DETECTOR_FALLBACKS` | empty |
| Media stream classifier | `mime.media.stream.classifier.default` | `QUBIT_MEDIA_STREAM_CLASSIFIER_DEFAULT` | `ffprobe` |
| Media stream staging limit | `mime.media.stream.max.staging.size` | `QUBIT_MEDIA_STREAM_MAX_STAGING_SIZE` | `67108864` |
| Precise detection enabled | `mime.enable.precise.detection` | `QUBIT_MIME_ENABLE_PRECISE_DETECTION` | `true` |
| Precise detection patterns | `mime.precise.detection.patterns` | `QUBIT_MIME_PRECISE_DETECTION_PATTERNS` | `webm,ogg` |
| Ambiguous MIME mapping | `mime.ambiguous.mime.mapping` | `QUBIT_MIME_AMBIGUOUS_MIME_MAPPING` | `webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg` |
| Maximum detector buffer size | `mime.max.buffer.size` | `QUBIT_MIME_MAX_BUFFER_SIZE` | `16777216` |

### Detect a filesystem path

```rust
use qubit_mime::{
    MimeDetectionPolicy,
    MimeError,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let detector = RepositoryMimeDetector::new()?;
    let path = std::env::temp_dir().join("qubit-mime-example.pdf");

    std::fs::write(&path, b"%PDF-1.7\n")?;
    let detected = detector.detect_file(&path, MimeDetectionPolicy::VerifyContent)?;
    std::fs::remove_file(&path).ok();

    assert_eq!(Some("application/pdf".to_owned()), detected);
    Ok(())
}
```

### Use the system `file` command detector

`FileCommandMimeDetector` uses the embedded repository for filename candidates
and `file --mime-type --brief` for content detection.

```rust,no_run
use std::time::Duration;

use qubit_command::CommandRunner;
use qubit_mime::{
    FileCommandMimeDetector,
    MimeDetectionPolicy,
    MimeDetector,
    MimeError,
};

fn main() -> Result<(), MimeError> {
    if !FileCommandMimeDetector::is_available() {
        return Ok(());
    }

    let detector = FileCommandMimeDetector::new();
    let detected = detector.detect(
        b"%PDF-1.7\n",
        Some("report.bin"),
        MimeDetectionPolicy::VerifyContent,
    );

    assert_eq!(Some("application/pdf".to_owned()), detected);

    let runner = CommandRunner::new()
        .timeout(Duration::from_secs(2))
        .disable_logging(true);
    let detector = FileCommandMimeDetector::new().with_command_runner(runner);
    assert!(detector.command_runner().configured_timeout().is_some());

    Ok(())
}
```

### Classify media streams with FFprobe

`FfprobeCommandMediaStreamClassifier` classifies a media file as no media,
audio-only, video-only, or video with audio.

```rust,no_run
use std::path::Path;

use qubit_mime::{
    FfprobeCommandMediaStreamClassifier,
    MediaStreamClassifier,
    MediaStreamType,
    MimeError,
};

fn main() -> Result<(), MimeError> {
    if !FfprobeCommandMediaStreamClassifier::is_available() {
        return Ok(());
    }

    let classifier = FfprobeCommandMediaStreamClassifier::new();
    let stream_type = classifier.classify_file(Path::new("sample.webm"))?;

    assert!(matches!(
        stream_type,
        MediaStreamType::AudioOnly
            | MediaStreamType::VideoOnly
            | MediaStreamType::VideoWithAudio
            | MediaStreamType::None,
    ));
    Ok(())
}
```

### Detect from a seekable reader

`detect_reader` reads up to the repository's required magic byte count and then
restores the original stream position.

```rust
use std::io::{
    Cursor,
    Seek,
};

use qubit_mime::{
    MimeDetectionPolicy,
    MimeError,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let detector = RepositoryMimeDetector::new()?;
    let mut reader = Cursor::new(b"%PDF-1.7\npayload".to_vec());

    let detected = detector.detect_reader(
        &mut reader,
        Some("document.bin"),
        MimeDetectionPolicy::VerifyContent,
    )?;
    assert_eq!(Some("application/pdf".to_owned()), detected);
    assert_eq!(0, reader.stream_position()?);

    Ok(())
}
```

## Combined Detection Strategy

Combined detection accepts both a filename and content bytes. The
`MimeDetectionPolicy` value makes the filename/content resolution strategy
explicit at each call site.

```rust
use qubit_mime::{
    MimeDetectionPolicy,
    MimeError,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let detector = RepositoryMimeDetector::new()?;
    let pdf_bytes = b"%PDF-1.7\n";

    // Prefer a definitive filename result and avoid extra content inspection.
    let by_filename = detector.detect_bytes(
        pdf_bytes,
        Some("photo.jpg"),
        MimeDetectionPolicy::PreferFilename,
    );
    assert_eq!(Some("image/jpeg".to_owned()), by_filename);

    // Verify content magic when content is more authoritative.
    let by_magic = detector.detect_bytes(
        pdf_bytes,
        Some("photo.jpg"),
        MimeDetectionPolicy::VerifyContent,
    );
    assert_eq!(Some("application/pdf".to_owned()), by_magic);

    Ok(())
}
```

Use `MimeDetectionPolicy::PreferFilename` when filenames come from a trusted
source and you want less I/O. Use `MimeDetectionPolicy::VerifyContent` when
uploaded or user-controlled filenames may be wrong or misleading.

## Repository Metadata

The default detector exposes the parsed repository. Use it when you need
metadata instead of just a MIME name.

```rust
use qubit_mime::{
    MimeError,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let detector = RepositoryMimeDetector::new()?;
    let repository = detector.repository();

    let png = repository
        .get("image/png")
        .expect("default repository should contain image/png");

    assert_eq!("image/png", png.name());
    assert_eq!(Some("png"), png.preferred_extension());
    assert!(png.description().is_some());
    assert!(png.matches_filename("ICON.PNG"));

    let extensions = png.all_extensions();
    assert!(extensions.contains(&"png"));

    Ok(())
}
```

`MimeRepository::detect_by_filename` and `MimeRepository::detect_by_content`
return all best candidates instead of a single string:

```rust
use qubit_mime::{
    MimeError,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let detector = RepositoryMimeDetector::new()?;
    let repository = detector.repository();

    let candidates = repository.detect_by_filename("archive.tar.gz");
    assert!(!candidates.is_empty());

    for candidate in candidates {
        println!("{} {:?}", candidate.name(), candidate.description());
    }

    Ok(())
}
```

## Custom Repository

Use `MimeRepository::from_xml` when you need a small test repository, a product
specific MIME database, or a database generated from another source.

```rust
use qubit_mime::{
    MimeError,
    MimeRepository,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let xml = r#"
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-example">
    <comment>Example bundle</comment>
    <alias type="application/example"/>
    <glob pattern="*.example" weight="80"/>
    <magic priority="90">
      <match type="string" value="EXAMPLE" offset="0"/>
    </magic>
  </mime-type>
</mime-info>
"#;

    let repository = MimeRepository::from_xml(xml)?;
    let detector = RepositoryMimeDetector::with_repository(&repository);

    assert_eq!(
        Some("application/x-example".to_owned()),
        detector.detect_by_filename("demo.example"),
    );
    assert_eq!(
        Some("application/x-example".to_owned()),
        detector.detect_by_content(b"EXAMPLE payload"),
    );

    let mime_type = repository
        .get("application/example")
        .expect("alias should resolve to the canonical MIME type");
    assert_eq!("application/x-example", mime_type.name());
    assert_eq!(Some("example"), mime_type.preferred_extension());

    Ok(())
}
```

## Low-Level Rule Types

The low-level types are useful in tests and integrations that need to inspect
or validate MIME rules directly.

```rust
use qubit_mime::{
    MagicValueType,
    MimeError,
    MimeGlob,
    MimeMagic,
    MimeMagicMatcher,
    MimeType,
};

fn main() -> Result<(), MimeError> {
    let glob = MimeGlob::new("*.png", MimeGlob::DEFAULT_WEIGHT, false)?;
    assert!(glob.matches("ICON.PNG"));

    let matcher = MimeMagicMatcher::new(
        MagicValueType::String,
        0,
        0,
        b"\x89PNG\r\n\x1a\n".to_vec(),
        None,
        vec![],
    )?;
    let magic = MimeMagic::new(80, vec![matcher]);

    let png = MimeType::builder("image/png")
        .description("en", "PNG image")
        .alias("image/x-png")
        .glob(glob)
        .magic(magic)
        .build();

    assert_eq!("image/png", png.name());
    assert_eq!(Some("png"), png.preferred_extension());
    assert!(png.matches_filename("icon.png"));
    assert!(png.magics()[0].matches(b"\x89PNG\r\n\x1a\npayload"));

    Ok(())
}
```

## API Reference

### `MimeDetector`

| Method | Description |
|--------|-------------|
| `MimeDetectorRegistry::global()` | Borrow the process-wide Registry shared by App and libraries |
| `MimeDetectorRegistry::builtin()` | Create an isolated Registry with built-in detector providers |
| `MimeDetectorRegistry::register(provider)` | Register an owned self-described provider at runtime |
| `MimeDetectorRegistry::register_shared(provider)` | Register an already shared self-described provider |
| `MimeDetectorRegistry::resolve_selected(selection)` | Resolve an explicit selection without creating a detector |
| `MimeDetectorRegistry::resolve()` | Resolve the Registry default without requiring MIME config |
| `ResolvingServiceProvider::create(config)` | Create a detector with explicit service config |
| `ResolvingServiceProvider::create()` | Create a detector with default service config |
| `MimeDetectorRegistry::provider_ids()` | List canonical provider IDs in registration order |
| `MimeDetectorProvider` | Factory trait for pluggable detector implementations |
| `detect_by_filename(filename)` | Detect one MIME name from filename |
| `detect_by_content(bytes)` | Detect one MIME name from content bytes |
| `detect(bytes, filename, policy)` | Detect from bytes and optional filename |

### `RepositoryMimeDetector`

| Method | Description |
|--------|-------------|
| `new()` | Create a detector backed by the embedded freedesktop repository |
| `with_repository(repository)` | Create a detector borrowing an explicit repository |
| `with_repository_and_config(repository, config)` | Create a detector borrowing an explicit repository and MIME configuration |
| `repository()` | Borrow the underlying repository |
| `detect_by_filename(filename)` | Return the first MIME name matched by filename |
| `detect_by_content(bytes)` | Return the first MIME name matched by content magic |
| `detect_bytes(bytes, filename, policy)` | Detect from bytes and optional filename |
| `detect_reader(reader, filename, policy)` | Detect from a `Read + Seek` reader and restore its position |
| `detect_file(file, policy)` | Open and detect a local file path |

### `FileCommandMimeDetector`

| Method | Description |
|--------|-------------|
| `new()` | Create a detector backed by the embedded repository and the system `file` command |
| `with_repository(repository)` | Create a detector borrowing an explicit repository |
| `with_repository_and_runner(repository, runner)` | Create a detector with an explicit `qubit_command::CommandRunner` |
| `with_repository_runner_and_config(repository, runner, config)` | Create a detector with explicit repository, runner, and MIME configuration |
| `command_runner()` | Borrow the runner used for command execution |
| `set_command_runner(runner)` | Replace the runner used for command execution |
| `is_available()` | Check whether the `file` command can be executed |
| `detect_file_by_content(file)` | Detect a local file using command output only |
| `detect_file(file, policy)` | Detect a local file by filename and command-backed content inspection |
| `detect_reader(reader, filename, policy)` | Detect a seekable reader through the file-backed path |

### `MediaStreamClassifier`

| Method | Description |
|--------|-------------|
| `MediaStreamClassifierRegistry::global()` | Borrow the process-wide classifier Registry |
| `MediaStreamClassifierRegistry::builtin()` | Create an isolated Registry with the built-in classifier provider |
| `MediaStreamClassifierRegistry::register(provider)` | Register an owned self-described classifier provider |
| `MediaStreamClassifierRegistry::register_shared(provider)` | Register an already shared self-described classifier provider |
| `MediaStreamClassifierRegistry::resolve_selected(selection)` | Resolve an explicit classifier selection |
| `MediaStreamClassifierRegistry::resolve()` | Resolve the Registry default independently from MIME config |
| `MediaStreamClassifierRegistry::provider_ids()` | List canonical provider IDs in registration order |
| `MediaStreamClassifierProvider` | Factory trait for pluggable classifier implementations |
| `classify_file(file)` | Classify a local media file |
| `classify_reader(reader)` | Classify media content from a reader |
| `classify_content(bytes)` | Classify in-memory media content |

### `FfprobeCommandMediaStreamClassifier`

| Method | Description |
|--------|-------------|
| `new()` | Create an FFprobe-backed classifier |
| `is_available()` | Check whether `ffprobe` can be executed |
| `classify_stream_listing(output)` | Classify parsed FFprobe `codec_type` output |
| `set_working_directory(directory)` | Set the command working directory |

### `MimeRepository`

| Method | Description |
|--------|-------------|
| `from_xml(xml)` | Parse a freedesktop shared MIME-info XML document |
| `empty()` | Create an empty repository |
| `all()` | Return all parsed MIME types in database order |
| `get(name)` | Resolve a canonical MIME name or alias |
| `max_test_bytes()` | Return the maximum byte prefix needed by magic rules |
| `detect_by_filename(filename)` | Return best filename candidates |
| `detect_by_content(bytes)` | Return best content candidates |
| `detect(filename, bytes, policy)` | Merge filename and content detection |

### Metadata and Rule Types

| Type | Purpose |
|------|---------|
| `MimeType` | Metadata and rules for one MIME type |
| `MimeTypeBuilder` | Builder for standalone `MimeType` values |
| `MimeGlob` | Filename glob rule with weight and case-sensitivity |
| `MimeMagic` | Priority-ranked collection of magic matchers |
| `MimeMagicMatcher` | One magic matcher with offset, value, mask, and children |
| `MagicValueType` | Freedesktop magic value type enum |
| `MediaStreamType` | Audio/video stream classification result |
| `MimeConfig` | Precise detection and ambiguous media mapping configuration |
| `MimeError` | Error type for XML parsing, rule validation, and I/O |

### `MimeConfig`

| Method | Description |
|--------|-------------|
| `from_config(config)` | Parse MIME configuration from a `qubit_config::Config` |
| `from_env()` | Parse MIME configuration from `Config::from_env()` |
| `default()` | Clone the current global default MIME configuration |
| `set_default(config)` | Replace the global default used by future default instances |
| `reload_default(config)` | Parse and replace the global default from a `Config` |
| `reload_default_from_env()` | Parse and replace the global default from process environment |
| `mime_detector_default()` | Read the configured detector selector |
| `mime_detector_fallbacks()` | Read the configured detector fallback chain |
| `media_stream_classifier_default()` | Read the configured media classifier selector |
| `media_stream_max_staging_size()` | Read the reader/content staging limit for media classifiers |

## Module Layout

The source layout is grouped by detector, classifier, and repository concerns:

```text
src/
  mime_detector.rs              # top-level MimeDetector trait
  mime_config.rs                # precise detection configuration
  detector/                     # detector implementations
  classifier/                   # media stream classifier interface and implementations
  repository/                   # MIME database, glob, magic, and metadata types
```

Use the root re-exports for normal application code. Use the nested modules
when you need to inspect or extend a specific detector, classifier, or
repository component.

## Detection Rules

### Filename Rules

Filename detection uses only the final path component. Matches are ranked by:

1. Higher glob weight.
2. Longer glob pattern when weights tie.
3. Database order when the repository returns multiple equal candidates.

### Content Rules

Content detection checks the provided byte prefix against each MIME type's
direct magic rules. Matches are ranked by higher magic priority. Use
`repository.max_test_bytes()` to know the largest useful prefix length for a
repository.

### Combined Rules

Combined detection first evaluates filename globs. When there is exactly one
filename match and magic checking is not forced, that filename match is used.
Otherwise, content magic is evaluated and merged with filename candidates.

## Comparison with Java `common-mime`

| Aspect | Java `common-mime` | Qubit MIME |
|--------|--------------------|------------|
| Database model | Freedesktop shared MIME-info | Same model |
| Filename detection | Glob rules | Glob rules |
| Content detection | Magic rules | Magic rules |
| Alias lookup | Supported | Supported |
| Detector interface | `MimeDetector` | `MimeDetector` trait |
| Media stream classifier | `MediaStreamClassifier` | `MediaStreamClassifier` trait |
| Repository detector | `RepositoryMimeDetector` | `RepositoryMimeDetector` |
| File command detector | `FileCommandMimeDetector` | `FileCommandMimeDetector` |
| FFprobe classifier | `FfprobeCommandMediaStreamClassifier` | `FfprobeCommandMediaStreamClassifier` |
| Repository loading | XML resource | Embedded XML or explicit XML |
| Return style | Java objects and collections | Rust `Option`, slices, and vectors |
| Errors | Java exceptions | Concrete `MimeError` |

## Testing

```bash
# Core API with the default empty feature set
cargo test --no-default-features

# Core API plus regex validation
cargo test --all-features

# Project CI checks
./ci-check.sh

# Check code coverage
./coverage.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Please follow the Rust API guidelines, keep public
API documentation and tests current, and run `./align-ci.sh` to format code and
`./ci-check.sh` to satisfy CI requirements before submitting a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-mime](https://github.com/qubit-ltd/rs-mime)
