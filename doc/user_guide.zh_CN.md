# qubit-mime 用户手册

本手册面向需要检查上传文件、对象存储资源或媒体流的 Rust 应用，适用版本为
`qubit-mime` 0.16。

## 上传文件场景

需要稳定的文件名和内容匹配时，创建 `RepositoryMimeDetector`：

```rust
use qubit_mime::{MimeDetectionPolicy, MimeDetector, RepositoryMimeDetector};

let detector = RepositoryMimeDetector::new()?;
let mime = detector.detect_bytes(
    b"%PDF-1.7\n",
    Some("report.pdf"),
    MimeDetectionPolicy::VerifyContent,
)?;
```

文件名适合路由，`VerifyContent` 会在文件名可能被伪造时检查内容。检测器返回
`MimeResult<Option<String>>`：`Result` 表示运行错误，`None` 表示没有匹配候选。

## Provider 与配置

`MimeDetectorRegistry::builtin()` 提供 repository 和可选的本地 `file` provider。
解析 provider 后使用 `create_configured(&MimeConfig)` 创建检测器。应用自己的 provider
实现 `ProviderMetadata` 与 `ServiceProvider<MimeDetectorSpec>`；完整的隔离 registry
示例见 `examples/custom_provider.rs`。

## 文件系统路径

本地路径使用 `detect_file`，同步 provider filesystem 使用 `detect_path`，异步 facade
使用 `detect_async_path`。`max_bytes` 限制读取的前缀长度，不能超过检测器缓冲区上限。
声明 `ContentRequirement::Complete` 的后端不能使用前缀入口。

## 限制与诊断

内置仓库来自嵌入的 shared MIME-info XML。Native command provider 依赖 `file` 或
`ffprobe` 可执行文件，并受超时、输出和暂存大小配置限制。发生错误时先检查具体的
`MimeError`，再决定是否切换 provider。检测 MIME 类型不能证明文件可以安全打开或执行。

## 验证

发布前运行 `cargo test --all-features --locked`、`./style-check.sh` 和 `./ci-check.sh`。
英文手册见 [`user_guide.md`](user_guide.md)。
