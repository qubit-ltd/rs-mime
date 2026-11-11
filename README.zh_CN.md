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
qubit-mime = "0.21"
```

本 crate 要求 Rust 1.94 或更高版本。默认的 `repository` 检测器使用 crate 内置的
MIME 数据库，不依赖外部命令。

## 快速开始

上传接口可以同时比较文件名和内容前缀；两者不一致时，让内容检测结果优先：

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

需要时可以分别调用 `detect_by_filename` 和 `detect_by_content`。当文件名只有一个明确
结果时，`MimeDetectionPolicy::PreferFilename` 不会运行内容检测或媒体分类；`VerifyContent`
仍会检查内容并要求内容匹配。若内容没有候选时仍需保留文件名结果作为 fallback，请使用
`ContentFirst`。如果应用以前依赖对文件名候选进行媒体分类，内容必须匹配时改用
`VerifyContent`，而文件名仍可作为 fallback 时改用 `ContentFirst`。检测成功返回
`Some(mime)`，返回 `None` 表示没有匹配候选。`detect_bytes` 和 `detect_by_content`
面向完整内容；若启用精确检测并注入 classifier，classifier 可能检查整份媒体资源。只有前缀时
请调用 `MimeDetector::detect_prefix`；内置 detector 不会把该前缀交给 classifier。精确检测必须
同时启用配置并显式注入 classifier。

默认的 `Freedesktop` 策略会在文件名类型属于内容结果的子类型时保留更具体的文件名类型；
别名按当前仓库解析。未知 MIME 名称只有在规范化后相同才会匹配。若应用需要旧版按名称精确
比较时的冲突结果，可选择 `ContentFirst`，并对返回类型应用自己的允许列表。

对于内置 detector 和通过本 crate blanket `MimeDetector` 实现接入的 backend，
`detect_prefix` 会匹配文件名和内容候选，不会把前缀交给需要完整媒体内容的
classifier。直接实现 `MimeDetector` 的类型自行决定其行为。默认异步 Prefix 路径最多读取
请求的字节数；需要完整内容的 backend 必须自行提供异步路径 hook。

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

默认的 `MimeContentBackend::detect_reader` 从 reader 当前位置读取。`Prefix(n)` 最多读取
n 字节，若提前遇到 EOF 则停止；`Complete` 在默认 16 MiB 预算（或覆盖后的预算）内读到
EOF，并额外探测一个字节以检查是否超限。两者都会恢复起始位置；恢复错误优先。默认
`MimeDetectorBackend` reader hook 只支持其前缀，遇到 `Complete` 会返回
`CompleteContentRequired`，因此完整内容 backend 必须覆盖该 hook。adapter 委托给 backend
reader hook；自定义随机访问覆盖仍可使用。通过异步 Provider 路径调用时，内置 `file` provider
会在线程池执行暂存、命令和清理。自定义 backend 的默认异步内容 hook 会调用同步方法；可能阻塞的
backend 应覆盖该 hook。丢弃 future 不保证取消已启动的任务。前缀读取期间可能同时保留 Provider
缓冲区和 worker 的 owned 副本；`max_buffer_size` 限制单个缓冲区，不限制两者合计内存。无文件名时，若同一 MIME 对应的多个映射具有不同
MIME 对，细化会保留原 MIME；可传入匹配的文件名或消除映射冲突来指定细化对。

检测器只负责判断 MIME 类型，不能证明文件可以安全打开或执行。Native command
Provider 依赖对应的可执行文件，也可能因命令不可用、超时或输出无效而失败。

本地 `detect_file` 的路径对象行为取决于 Provider。repository 检测会跟随指向有效文件的符号链接；
悬空链接和目录会在打开或读取时出错。默认 `file` runner 不添加 `-L`，而是将路径交给系统
`file` 命令，因此链接和目录的处理取决于平台、命令版本及环境。注入的 executor 则由注入者决定行为。
需要一致内容语义时，应用应按自己的链接策略打开普通文件，再通过 reader 检测。拒绝链接应由应用控制
路径打开策略；单独检查 metadata 无法消除竞态。

内置 `file` Provider 检测 reader 时，最多暂存 `MimeConfig::max_buffer_size()` 字节，并在 EOF 时停止；
该预算不由所选文件名仓库中的 magic 偏移决定。字节切片检测检查调用方提供的字节；`file` Provider
则将本地路径传给系统 `file` 命令。超过 reader 预算的资源可能因只检查暂存前缀而得到不同结果。应用可通过现有
`mime.max.buffer.size` 配置调整预算。reader 契约和不同入口的行为详见
[reader 行为说明](doc/user_guide.zh_CN.md#核心工作流)。

## Provider 选择与配置

`MimeDetectorRegistry::builtin()` 提供内置的 `repository` 和 `file` Provider。Provider
解析与检测器创建分为两个步骤：

下面的示例使用 `Config`；如果应用还要实现自定义 Provider，则会用到 `ServiceProvider`。请为这类 Provider 配置声明以下直接依赖：

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
任一环境值无效时，`MimeConfig::load()` 会回退到整套内置默认值。严格启动校验和显式重载方式
见[环境配置说明](doc/user_guide.zh_CN.md#环境配置与整体回退)。

加载 MIME repository 时，非法文件名 glob 会以结构化的
`MimeError::InvalidXmlAttribute` 错误返回。

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
./.infra/bin/ci-check.sh

# 检查代码覆盖率
./.infra/bin/coverage.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./.infra/bin/align-ci.sh`格式化代码，运行`./.infra/bin/ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-mime](https://github.com/qubit-ltd/rs-mime)
