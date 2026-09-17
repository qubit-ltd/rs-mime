# qubit-mime user guide

This guide describes `qubit-mime` 0.16 for applications that inspect uploaded
files, object-store resources, or media streams.

## The upload scenario

Create a `RepositoryMimeDetector` when the application needs deterministic
filename and content matching:

```rust
use qubit_mime::{MimeDetectionPolicy, MimeDetector, RepositoryMimeDetector};

let detector = RepositoryMimeDetector::new()?;
let mime = detector.detect_bytes(
    b"%PDF-1.7\n",
    Some("report.pdf"),
    MimeDetectionPolicy::VerifyContent,
)?;
```

Filename rules are useful for routing, while `VerifyContent` checks the bytes
when a filename can be misleading. A detector returns `MimeResult<Option<String>>`:
the `Result` reports operational errors and `None` means no candidate matched.

## Providers and configuration

`MimeDetectorRegistry::builtin()` contains the repository provider and the
optional native `file` provider. Resolve a provider and create it with
`create_configured(&MimeConfig)`. Application providers implement
`ProviderMetadata` and `ServiceProvider<MimeDetectorSpec>`; see
`examples/custom_provider.rs` for a complete isolated registry example.

## Filesystem paths

Use `detect_file` for a local path, `detect_path` for the synchronous provider
filesystem facade, and `detect_async_path` for the asynchronous facade. The
`max_bytes` argument bounds the prefix read and must not exceed the detector's
configured buffer limit. A backend with `ContentRequirement::Complete` cannot
use a prefix entry point.

## Limits and diagnostics

The bundled repository is parsed from the embedded shared MIME-info XML. Native
command providers depend on the `file` or `ffprobe` executable and their
configured timeout/output/staging limits. Check the concrete `MimeError` before
falling back to another provider. The detector identifies MIME types; it does
not prove that a file is safe to open or execute.

## Verification

Run `cargo test --all-features --locked`, `./style-check.sh`, and `./ci-check.sh`
before publishing. The Chinese guide is available at
[`user_guide.zh_CN.md`](user_guide.zh_CN.md).
