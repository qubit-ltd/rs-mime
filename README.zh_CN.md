# Qubit MIME

[![Rust CI](https://github.com/qubit-ltd/rs-mime/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-mime/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-mime/coverage-badge.json)](https://qubit-ltd.github.io/rs-mime/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-mime.svg?color=blue)](https://crates.io/crates/qubit-mime)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

`qubit-mime` 帮助 Rust 服务根据文件名、内容字节或两者共同判断上传文件或资源的
MIME 类型。它适合文件上传路由和媒体检查场景：默认使用内置仓库，也可以接入系统
`file` 后端，并明确处理检测结果冲突、Provider 不可用和资源上限。

完整的安装、Provider、文件系统、诊断和限制说明见[中文用户手册](doc/user_guide.zh_CN.md)。
[英文 README](README.md) 与[英文用户手册](doc/user_guide.md)提供对应内容。

## 安装

```toml
[dependencies]
qubit-mime = "0.18"
```

本 crate 要求 Rust 1.94 或更高版本。默认的 `repository` 检测器使用 crate 内置的
MIME 数据库，不依赖外部命令。

## 快速开始

上传接口可以同时比较文件名和内容前缀；两者不一致时，让内容检测结果优先：

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

需要时可以分别调用 `detect_by_filename` 和 `detect_by_content`。当文件名已有明确
结果时，`MimeDetectionPolicy::PreferFilename` 不再检查内容；`VerifyContent` 仍会
检查内容。检测成功返回 `Some(mime)`，返回 `None` 表示没有匹配候选。

## 提供的能力

- 从内置仓库读取 freedesktop shared MIME-info 的名称、别名、文件名 glob、magic、
  comment 和父类型元数据。
- 通过 `RepositoryMimeDetector` 和 `MimeDetector` trait 进行文件名、内容魔数和组合检测。
- 支持本地文件 `detect_file`、可 seek reader `detect_reader`，以及与 Provider 无关的
  文件系统入口 `detect_path` 和 `detect_async_path`。
- 通过 `MimeDetectorRegistry` 与 `MimeConfig` 配置内置 Provider、应用自定义 Provider、
  Provider 选择、fallback 和资源限制。
- 可选的 `FileCommandMimeDetector` 使用系统命令 `file --mime-type --brief` 检查内容。
- 可选的 `FfprobeCommandMediaStreamClassifier` 可进一步区分 WebM、Ogg 等媒体流中的
  仅音频和仅视频结果。

检测器只负责判断 MIME 类型，不能证明文件可以安全打开或执行。Native command
Provider 依赖对应的可执行文件，也可能因命令不可用、超时或输出无效而失败。

## Provider 选择与配置

`MimeDetectorRegistry::builtin()` 提供内置的 `repository` 和 `file` Provider。Provider
解析与检测器创建分为两个步骤：

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
    let detector = provider.create_configured(&config)?;

    assert_eq!(
        detector.detect_by_filename("image.png")?.as_deref(),
        Some("image/png"),
    );
    Ok(())
}
```

应用自定义 Provider 需要实现 `ProviderMetadata` 和
`ServiceProvider<MimeDetectorSpec>`。进程级 Registry 可通过
`MimeDetectorRegistry::global()` 获取；`builtin()` 返回适合测试或局部应用的隔离
Registry。

启用可选的 `inventory` feature 后，已链接的 Provider crate 可以通过
`qubit_spi::submit_sync_provider!` 分别向
`qubit_mime::detector::mime_detector_inventory::Entry` 和
`qubit_mime::classifier::media_stream_classifier_inventory::Entry` 提交检测器和分类器。
两个 `builtin()` Registry 会发现这些 Provider，同时保留 `repository` 和 `ffprobe`
默认选择。重复 selector 会使 inventory 构建失败；未启用此 feature 时仍需显式注册。

常用配置键包括 `mime.detector.default`、`mime.detector.fallbacks`、
`mime.max.buffer.size`、`mime.command.timeout` 和 `mime.command.output.max.bytes`。
完整配置项请参阅 `MimeConfig` 和用户手册。

## API 与延伸阅读

- [中文用户手册](doc/user_guide.zh_CN.md)
- [英文用户手册](doc/user_guide.md)
- [Rust API 文档](https://docs.rs/qubit-mime)
- [Crates.io 包](https://crates.io/crates/qubit-mime)
- [English README](README.md)

## 测试

```bash
# 使用默认 feature 集运行测试
cargo test

# 使用项目声明的全部 feature 运行测试
cargo test --all-features

# 运行项目 CI 检查
./ci-check.sh

# 检查代码覆盖率
./coverage.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./align-ci.sh`格式化代码，运行`./ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-mime](https://github.com/qubit-ltd/rs-mime)
