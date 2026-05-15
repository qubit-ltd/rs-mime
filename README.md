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
qubit-mime = "0.5.2"
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

`MimeDetectorRegistry` creates boxed or shared `MimeDetector` trait objects
from `MimeConfig`. Code that only needs MIME names can depend on the trait
instead of a concrete detector.

```rust
use qubit_mime::{
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorRegistry,
    MimeError,
};

fn detect_upload(detector: &dyn MimeDetector, filename: &str, content: &[u8]) -> Option<String> {
    detector.detect(content, Some(filename), MimeDetectionPolicy::VerifyContent)
}

fn main() -> Result<(), MimeError> {
    let detector =
        MimeDetectorRegistry::default_registry()?.create_default_box(&MimeConfig::default())?;

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
    CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_DETECTOR_FALLBACKS,
    CONFIG_MIME_ENABLE_PRECISE_DETECTION,
    CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
    DEFAULT_AMBIGUOUS_MIME_MAPPING,
    DEFAULT_PRECISE_DETECTION_PATTERNS,
    MimeConfig,
    MimeDetector,
    MimeDetectorRegistry,
    MimeError,
};

fn main() -> Result<(), MimeError> {
    let original = MimeConfig::default();
    let mut config = Config::new();
    config.set(CONFIG_MIME_DETECTOR_DEFAULT, "repository")?;
    config.set(CONFIG_MIME_DETECTOR_FALLBACKS, "")?;
    config.set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")?;
    config.set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)?;
    config.set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, DEFAULT_PRECISE_DETECTION_PATTERNS)?;
    config.set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, DEFAULT_AMBIGUOUS_MIME_MAPPING)?;

    MimeConfig::reload_default(&config)?;
    let detector =
        MimeDetectorRegistry::default_registry()?.create_default_box(&MimeConfig::default())?;

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf"),
    );

    MimeConfig::set_default(original);
    Ok(())
}
```

### Select detectors with registry and fallbacks

`MimeDetectorRegistry::create_default_box()` and
`MimeDetectorRegistry::create_default_arc()` use the configured default
detector first. If that provider is unknown, unavailable, or fails to
initialize, the configured fallback chain is tried in order. Set the default
selector to `auto` to choose the highest-priority available provider from the
registry.

The default registry starts with the built-in providers returned by
`MimeDetectorRegistry::builtin()`. Extension crates can make their providers
available to all default registry snapshots by calling
`MimeDetectorRegistry::register_default(provider)` during application startup.
After registration succeeds, later `MimeDetectorRegistry::default_registry()`
snapshots see the provider through the same process-wide registry.

`MimeDetectorRegistry::default_registry()` returns a snapshot clone of the
current process-wide registry. Mutating that snapshot does not update the global
registry. Use `register_default()` when a provider should become globally
visible, and use an explicit `MimeDetectorRegistry::builtin()` or
`MimeDetectorRegistry::new()` when a caller needs isolation, tests a custom
provider, or wants to restrict which providers can be selected.

Global registration is process-local and should normally happen once, before
creating detectors from configuration. Duplicate provider ids or aliases are
reported as `MimeError::DuplicateDetectorName`; poisoned global registry locks
are reported as `MimeError::DetectorBackend`.

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
    MimeError,
};

fn main() -> Result<(), MimeError> {
    let mut source = Config::new();
    source.set(CONFIG_MIME_DETECTOR_DEFAULT, "file")?;
    source.set(CONFIG_MIME_DETECTOR_FALLBACKS, "repository")?;

    let config = MimeConfig::from_config(&source)?;
    let detector = MimeDetectorRegistry::default_registry()?.create_default_box(&config)?;

    assert_eq!(
        Some("image/png".to_owned()),
        detector.detect_by_filename("image.png"),
    );
    Ok(())
}
```

Use an explicit registry when you need custom providers:

```rust
use qubit_mime::{
    MimeConfig,
    MimeDetector,
    MimeDetectorRegistry,
    MimeError,
};

fn main() -> Result<(), MimeError> {
    let registry = MimeDetectorRegistry::builtin();
    let detector = registry.create_box("repository-mime-detector", &MimeConfig::default())?;

    assert_eq!(
        Some("text/plain".to_owned()),
        detector.detect_by_filename("notes.txt"),
    );
    Ok(())
}
```

Registry selection uses these error categories:

| Error | Meaning |
|-------|---------|
| `DuplicateDetectorName` | A provider id or alias conflicts with an existing provider |
| `UnknownDetector` | No registered provider matches the requested selector |
| `DetectorUnavailable` | The provider exists but its backend is not available in this process environment |
| `NoAvailableDetector` | Every default or fallback candidate failed |
| `DetectorBackend` | A provider backend failed while initializing or detecting |

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
| Precise detection enabled | `mime.enable.precise.detection` | `QUBIT_MIME_ENABLE_PRECISE_DETECTION` | `true` |
| Precise detection patterns | `mime.precise.detection.patterns` | `QUBIT_MIME_PRECISE_DETECTION_PATTERNS` | `webm,ogg` |
| Ambiguous MIME mapping | `mime.ambiguous.mime.mapping` | `QUBIT_MIME_AMBIGUOUS_MIME_MAPPING` | `webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg` |

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
| `MimeDetectorRegistry::builtin()` | Create an isolated registry with only built-in detector providers |
| `MimeDetectorRegistry::default_registry()` | Snapshot the process-wide default detector registry |
| `MimeDetectorRegistry::register_default(provider)` | Register an external detector provider globally |
| `MimeDetectorRegistry::register(provider)` | Register an external detector provider in an explicit registry |
| `MimeDetectorRegistry::create_box(name, config)` | Create a boxed detector by provider id or alias |
| `MimeDetectorRegistry::create_arc(name, config)` | Create a shared detector by provider id or alias |
| `MimeDetectorRegistry::create_default_box(config)` | Resolve the configured default, `auto`, and fallback chain into a boxed detector |
| `MimeDetectorRegistry::create_default_arc(config)` | Resolve the configured default, `auto`, and fallback chain into a shared detector |
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
| `MediaStreamClassifierRegistry::builtin()` | Create an isolated registry with built-in classifier providers |
| `MediaStreamClassifierRegistry::default_registry()` | Snapshot the process-wide default classifier registry |
| `MediaStreamClassifierRegistry::register_default(provider)` | Register an external classifier provider globally |
| `MediaStreamClassifierRegistry::register(provider)` | Register an external classifier provider in an explicit registry |
| `MediaStreamClassifierRegistry::create_box(name, config)` | Create a boxed classifier by provider id or alias |
| `MediaStreamClassifierRegistry::create_arc(name, config)` | Create a shared classifier by provider id or alias |
| `MediaStreamClassifierRegistry::create_default_box(config)` | Resolve the configured default or `auto` into a boxed classifier |
| `MediaStreamClassifierRegistry::create_default_arc(config)` | Resolve the configured default or `auto` into a shared classifier |
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

## Testing & Code Coverage

This project keeps tests under `tests/` and validates repository parsing,
filename matching, content magic matching, reader/path detection, and coverage
thresholds.

### Running Tests

```bash
# Run all tests
cargo test

# Generate a coverage report
./coverage.sh

# Generate a text format coverage report
./coverage.sh text

# Run CI checks (format, clippy, tests, docs, coverage, audit)
./ci-check.sh
```

## Dependencies

Runtime dependencies are intentionally small:

- `qubit-command` runs external `file` commands with timeout and output capture.
- `qubit-config` loads MIME defaults from configuration objects and environment.
- `regex` compiles and runs filename glob matchers.
- `roxmltree` parses shared MIME-info XML.
- `thiserror` provides the concrete `MimeError` implementation.

## License

Copyright (c) 2026. Haixing Hu, Qubit Co. Ltd. All rights reserved.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

<http://www.apache.org/licenses/LICENSE-2.0>

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

See [LICENSE](LICENSE) for the full license text.

## Contributing

Contributions are welcome. Please keep changes aligned with the existing Rust
project structure and run `./ci-check.sh` before opening a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

## Related Projects

More Rust libraries from Qubit are published under the
[qubit-ltd](https://github.com/qubit-ltd) GitHub organization.

---

Repository: [https://github.com/qubit-ltd/rs-mime](https://github.com/qubit-ltd/rs-mime)
