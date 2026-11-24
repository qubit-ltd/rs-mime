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
qubit-mime = "^0.22"
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
classifier。直接实现 `MimeDetector` 的类型自行决定其行为。受管异步操作使用异步 backend：附加
执行服务后默认采用 crate 内置的 `repository`；改用其他 backend 时需显式选择。为受管异步
backend 解析仅支持同步的 Provider 工厂时，会在 runtime 配置阶段以 `AsyncUnsupported` 失败，
早于任何操作提交；不会回退到同步执行。
受管操作的生命周期和 Provider 能力限制见用户手册。

旧入口 `detect_async_path` 在 `PreferFilename` 下仍会对唯一文件名候选提前返回。内置
`RepositoryMimeDetector` 没有异步内容 hook，因此按内容检测时会在打开 Provider 路径前返回
`AsyncUnsupported`。自定义 backend 保留原有异步 hook 行为；需要读取内容时，应实现异步内容或
Provider 路径 hook。若自定义 backend 希望在读取前拒绝，还可覆盖
`supports_async_content_detection` 并返回 `false`。

## 提供的能力

- 从内置仓库读取 freedesktop shared MIME-info 的名称、别名、文件名 glob、magic、
  comment 和父类型元数据。
- 通过 `RepositoryMimeDetector` 和 `MimeDetector` trait 进行文件名、内容魔数和组合检测。
- 支持本地文件 `detect_file`、可 seek reader `detect_reader`，以及与 Provider 无关的
  文件系统入口 `detect_path` 和 `detect_async_path`。
- 通过 `MimeDetectorRegistry` 与 `MimeConfig` 配置内置 Provider、应用自定义 Provider、
  Provider 选择、fallback 和资源限制。
- `MimeRuntime` 提供受管检测操作：输入所有权明确，提交拒绝带回原输入，业务结果与资源
  settlement 分开观察。附加执行服务后默认使用内置 `repository`；使用 `file` 时需显式选择。
  提交被拒绝时，`MimeSubmitFailure` 会带回原输入；任务接纳后的检测错误由 operation outcome
  报告，资源释放情况则通过 settlement 观察。关闭时，Pending 且 `snapshot.group` 尚无值，说明组 Drain
  还未发布；服务准备、操作结算或准入可能仍在进行，也可能有另一个关闭尝试正在执行。操作结算后，
  `stop_services()` 会先发布组 Drain，再可能因组资源或 recovery 未处理而返回 `OperationShutdownPending`；
  这种未解决项导致的 Pending 期间，execution port 仍可用。成功返回的 `stop_services()` 表示端口已停止；
  若另一个关闭尝试仍在执行，应检查 `services_stopped`。调用方仍持有 typed handle 时，完整组 Drain 可能继续等待。
  处理 recovery、释放资源和 handle 后再重试或等待 Drain。库保证的服务关闭后重试路径仅适用于内建暂存 recovery；
  转交后由 `RecoveryStore` 承担重试责任，且必须在重试期间存活；内建暂存恢复的备用清理 port 由 MIME 进程持有。
  自定义清理应在端口停止前完成，或保留可独立继续工作的依赖。
  完整操作顺序见[中文用户手册](doc/user_guide.zh_CN.md#受管检测操作0220)和可运行的生命周期示例。
- 可选的 `FileCommandMimeDetector` 使用系统命令 `file --mime-type --brief` 检查内容。
- 可选的 `FfprobeCommandMediaStreamClassifier` 可进一步区分 WebM、Ogg 等媒体流中的
  仅音频和仅视频结果。

默认的 `MimeContentBackend::detect_reader` 从 reader 当前位置读取。`Prefix(n)` 最多读取
n 字节，若提前遇到 EOF 则停止；`Complete` 在默认 16 MiB 预算（或覆盖后的预算）内读到
EOF，并额外探测一个字节以检查是否超限。两者都会恢复起始位置；恢复错误优先。默认
`MimeDetectorBackend` reader hook 只支持其前缀，遇到 `Complete` 会返回
`CompleteContentRequired`，因此完整内容 backend 必须覆盖该 hook。adapter 委托给 backend
reader hook；自定义随机访问覆盖仍可使用。受管检测通过 owned input、显式 scope 和有限请求预算管理执行过程，并分别观察业务结果与资源
settlement。创建 runtime 或注册 Provider 不会启动 worker；应用需显式准备和停止 MIME 执行服务。
为受管异步 backend 解析仅支持同步的 Provider 工厂时，会在 runtime 配置阶段以
`AsyncUnsupported` 失败，早于提交；不会在异步操作中同步执行。旧的同步
`FileCommandMimeDetector` API 仍可使用。丢弃进行中的 future 不能证明已接纳工作停止；释放资源前应观察 settlement。前缀读取期间可能同时保留 Provider
缓冲区和 worker 的 owned 副本；`max_buffer_size` 限制单个缓冲区，不限制两者合计内存。无文件名时，若同一 MIME 对应的多个映射具有不同
MIME 对，细化会保留原 MIME；可传入匹配的文件名或消除映射冲突来指定细化对。

几种输入方式的边界不同：普通 backend 的 `ContentRequirement::Prefix(n)` 是 reader
读取窗口的上限；自动读取普通 Provider 时也会标记为 `InputExtent::Prefix`，即使 EOF 提前到达。
`OwnedMimeInput::Bytes` 保留调用方提供的字节和 `InputExtent`，不会隐式截短，也不计作 Provider I/O。
扩展 backend 的完整读取和同对象随机采样通过 `MeteredReadSession` 完成；普通前缀 backend
可以选择使用该 session。session 读取共享 operation 额度。内置受管 `file` Provider 则自行计量 staging 读取。旧入口
`detect_path` 和 `detect_async_path` 的 `max_bytes` 参数仍是各自调用明确指定的前缀上限。

组准入关闭后，已接纳到组内的父 operation 仍可派发必要后代；新的外部 scope 必须在关闭前接纳。
同步子操作不会被强制打断，只能在自身的协作检查点观察 stop。
暂存清理 recovery 转交给 `RecoveryStore` 后，即使原 MIME 服务已经停止，仍可由该 store 重试；应用需要独立保留并运行 recovery store。
路径输入会按 native path 语义提取 basename 再进行文件名匹配，完整路径不会重新作为文件名解析；
POSIX basename 中的反斜杠属于文件名数据，非 UTF-8 本地 basename 会以 lossy 字符串匹配。
`OwnedMimeInput::Bytes` 仍使用调用方提供的 filename。
临时文件部分创建失败时，清理 owner 会由同一 operation scope 持有；应同时检查 settlement 和 recovery，
不能只依据失败结果判断清理完成。
受管异步操作会在 backend future 等待期间观察 scope deadline 和 stop 请求；repository matcher
在协作检查点响应。deadline/stop 也会在 backend 已就绪、结果提交之前再次裁决；因此即使 backend 已
返回成功或错误，超时仍可能先结束 operation。若用户取消已被接受，竞争时保留 `UserCancelled` 原因。
已经派发给 command 或 staging worker 的工作，会一直保留资源 owner，直到 worker job 实际结算。

嵌套 MIME operation 会沿 operation ancestor 共享 Provider 读取计量。由唯一文件名短路完成的输入和 owned bytes
输入不会绑定祖先读取额度，也不占用祖先的资源槽。需要读取 Provider 的后代会传播祖先身份及适用的 scope cap；
首次读取时从共享祖先树预留额度。若绑定无法在准入前完成，或祖先已结束，提交会带回原输入并拒绝；任务接纳后
无法提交待处理绑定，则 operation 以 `ReadBudgetBinding` 失败。

读取额度按成功交付的字节计费。Provider 打开失败计零；前缀读取在错误或取消前已交付的字节继续计费，未使用的
预留额度会退还。`Prefix(n)` 是最大窗口，不是最小长度：请求或服务的 buffer/read 上限可能缩短窗口，EOF 也可能使实际字节更少。
若提交时无法预留 owned 前缀缓冲区，会拒绝并返还原输入；若任务接纳后无法预留共享祖先读取额度，operation
会报告 `MimeError::BudgetExceeded { kind: "read", .. }`。扩展 backend 可选择 `MeteredReadSession`，它按当前额度裁切
每次读取并单独预留 scratch 内存。内置 `file` Provider 在自身 staging 读取中计量。
`MimeBudgetSummary.bytes_read` 统计本请求发起读取所交付的字节，重复读取会再次计数；后代请求使用各自的报告，
祖先额度仍会限制整棵读取树的合计用量。

检测器只负责判断 MIME 类型，不能证明文件可以安全打开或执行。Native command
Provider 依赖对应的可执行文件，也可能因命令不可用、超时或输出无效而失败。

本地 `detect_file` 的路径对象行为取决于 Provider。repository 检测会跟随指向有效文件的符号链接；
悬空链接和目录会在打开或读取时出错。默认 `file` runner 不添加 `-L`，而是将路径交给系统
`file` 命令，因此链接和目录的处理取决于平台、命令版本及环境。注入的 executor 则由注入者决定行为。
需要一致内容语义时，应用应按自己的链接策略打开普通文件，再通过 reader 检测。拒绝链接应由应用控制
路径打开策略；单独检查 metadata 无法消除竞态。

同步 `FileCommandMimeDetector` 的 reader/path API 与受管 `file` Provider 是两条独立入口；旧同步调用仍可继续使用。
受管 `file` Provider 在自己的 staging 流程中计量读取，不要求扩展 backend 使用公开的 `MeteredReadSession`。
受管 `file` factory 会拒绝任何附加的异步 classifier，错误为 `UnsupportedCapability`，发生在提交前；即使输入不会触发媒体细化也一样。
旧版异步内容 hook 返回 `AsyncUnsupported`；需要跟踪 staging、worker 执行和清理时，请走受管 Provider。
这项限制针对受管 factory，不会移除旧同步 API。

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

通过 registry 使用 Magika inventory，要求 `qubit-mime` 0.22 和 `qubit-magika` 0.18
均已发布。2026-11-24 的 registry manifest 检查发现 `qubit-mime` 0.22.0 尚未发布。
[固定源码接入指南](doc/user_guide.zh_CN.md#固定源码接入-magika)列出同级仓库的确切 revision、应用配置、`cargo check` 与 `--locked` 命令，以及动态 ONNX Runtime 前置条件。
[T4 固定版本中的 consumer fixture](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/src/main.rs)在普通模式显式注册 Magika，启用 inventory 时由 registry 发现后再显式选择。
[固定版本的 workflow](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/.github/workflows/magika-consumer.yml)记录了经过校验的运行库下载方式；[fixture manifest](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/Cargo.toml) 和 [锁文件](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/Cargo.lock)也固定在同一版本。fixture 源码、锁文件和 workflow 都可在源码仓库查看，但不会进入已发布的 crate package。

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
