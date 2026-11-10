# qubit-mime 用户手册

[English user guide](user_guide.md) · [README](../README.zh_CN.md) ·
[API 文档](https://docs.rs/qubit-mime)

本手册面向使用 `qubit-mime` 0.21 检查上传文件、文件系统资源或媒体流的 Rust 应用。
内容覆盖可用的检测入口，以及调用方需要自行处理的运行边界。

## 手册目标与读者

当应用需要 MIME 候选值来完成路由、校验或媒体处理时，可以使用本 crate。它会根据
内置的 freedesktop MIME-info 仓库匹配文件名和/或内容，但不会验证文件是否可以安全
打开或执行。

## 概念模型

公开 API 可以按用途分为三层：

- `RepositoryMimeDetector`：基于内置 `MimeRepository` 做确定性的匹配。
- `FileCommandMimeDetector`：使用仓库猜测文件名，并调用本机
  `file --mime-type --brief` 检查内容。
- `MimeDetectorRegistry`：负责解析 Provider；`MimeConfig` 负责提供 Provider 选择、
  fallback、命令、classifier 和缓冲区设置。

组合检测由 `MimeDetectionPolicy` 控制。`PreferFilename` 在文件名只有一个明确结果时
直接采用该结果，不运行内容检测或媒体分类；`VerifyContent` 即使文件名明确也会检查内容，
且内容没有候选时返回 `None`。如果希望内容优先，但仍保留文件名作为 fallback，请使用
`ContentFirst`。如果旧代码依赖对文件名候选进行媒体分类，内容必须匹配时改用
`VerifyContent`，而文件名仍可作为 fallback 时改用 `ContentFirst`。两种来源都没有匹配时，
成功结果为 `None`；错误会单独返回。

## 贯穿场景：检查上传文件

假设上传文件名是 `report.jpg`，但文件开头的字节表明它是 PDF。成功标准是返回内容
检测出的类型，而不是信任可能误导的扩展名：

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

可观察结果是 `Some("application/pdf")`。如果文件名和已检查的字节都没有匹配候选，
返回 `None` 也是正常结果。

## 安装与最小配置

在 `Cargo.toml` 中添加：

```toml
[dependencies]
qubit-mime = "0.21"
```

`RepositoryMimeDetector::new()` 使用内置仓库，不需要外部命令。若要显式选择 Provider，
请创建 `Config`，将其解析为 `MimeConfig`，再解析 Provider 并创建检测器：

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

`file` Provider 在检查内容时要求系统中存在 `file` 可执行文件。当它不可用且选择策略
允许 fallback 时，`repository` 可以继续提供检测服务。

## 核心工作流

根据输入类型选择最简单的入口：

- `detect_by_filename("photo.JPG")` 检查文件名 glob。
- `detect_by_content(bytes)` 检查内容 magic 规则。
- `RepositoryMimeDetector` 的 `detect_bytes(bytes, Some(filename), policy)` 组合两种结果。
- `detect_reader(reader, filename, policy)` 检查可 seek reader，并恢复 reader 的原始位置。
- `detect_file(path, policy)` 读取本地文件。
- `MimeDetector::detect_path` 和 `detect_async_path` 通过同步或异步文件系统 facade 读取。

默认的 `MimeContentBackend::detect_reader` 从 reader 当前位置检测到 EOF，然后恢复起始位置。
默认缓冲预算为 16 MiB。实现可以覆盖 `max_reader_buffer_size`，也可以提供受控的
`detect_reader`，通过随机访问处理更大的资源。adapter 的 Prefix 请求使用
`MimeConfig::max_buffer_size()` 预算，并通过偏移零点对应调用者当前位置的视图转发完整 reader
检测，因此下游通常从资源开头开始检测的 hook 只会看到当前位置至 EOF 的内容。检测和位置恢复
都失败时，位置恢复错误优先；否则返回读取或检测错误。

这些入口都返回 `MimeResult<Option<String>>`。应先处理外层错误，再解释可选的 MIME 名称。

## 进阶用法

应用需要全局注册 Provider 时，可以使用 `MimeDetectorRegistry::global()`；测试或局部
应用可以使用 `MimeDetectorRegistry::builtin()` 或其他独立 Registry。应用自定义 Provider
需要实现 `ProviderMetadata` 和 `ServiceProvider<MimeDetectorSpec>`，完整的注册与解析流程
见 `examples/custom_provider.rs`。

需要在链接时发现 Provider，可以启用可选的 `inventory` feature：

```toml
[dependencies]
qubit-mime = { version = "0.21", features = ["inventory"] }
qubit-magika = { version = "0.18", features = ["inventory"] }
```

应用代码还需引用 Provider crate（`use qubit_magika as _;`），使提交项参与链接。
此时 `MimeDetectorRegistry::builtin()` 能发现 `magika`，但默认仍选 `repository`；
分类器 Registry 也会发现已链接的提交项，并保留 `ffprobe` 作为默认选择。Provider
crate 通过 `qubit_spi::submit_sync_provider!` 向
`qubit_mime::detector::mime_detector_inventory` 或
`qubit_mime::classifier::media_stream_classifier_inventory` 下的 `Entry` 提交工厂。
重复的 selector 会导致 inventory 构建失败。未启用该 feature 时，仍按上文显式注册。

需要更丰富的元数据时，直接使用 `MimeRepository` 访问 `MimeType` 的元数据、别名、注释、
文件扩展名、magic 规则和父类型关系。对于媒体扩展名有歧义的情况，可以配置媒体流
classifier；如果环境提供 `ffprobe`，再使用 `FfprobeCommandMediaStreamClassifier`。

## 错误与诊断

常见的 `MimeError` 包括：

- `Io` 或 `FileSystem`：输入无法读取；
- `BufferLimitExceeded`：路径前缀请求超过 `MimeConfig::max_buffer_size()`；
- `CompleteContentRequired`：后端要求完整资源；
- `Command`、`DetectorUnavailable` 或 `DetectorBackend`：Native Provider 不可用或执行失败；
- `Config` 或 `InvalidConfigurationValue`：配置无效。
- `InvalidXmlAttribute`：文件名 glob 无法编译，包括超出 regex 编译大小限制的模式。

切换 fallback 前应检查具体错误。成功返回 `None` 不表示运行失败，只表示没有匹配候选。

## 排障

如果结果是 `None`，可以分别调用文件名和内容入口，确认是哪一类规则没有候选。如果
`file` 或 `ffprobe` 失败，请先确认可执行文件位于 `PATH`，再检查返回的 `MimeError` 和
命令配置。文件系统调用失败时，应先核对 Provider 无关路径和文件系统 facade，再调整
检测策略。

## 限制与最佳实践

`detect_path` 和 `detect_async_path` 的 `max_bytes` 是内容前缀上限，不能超过
`max_buffer_size`。`ContentRequirement::Prefix` 表示后端可用的输入上限。对于内置 detector
和通过本 crate blanket `MimeDetector` 实现接入的 backend，前缀检测不会将该前缀传给完整媒体
classifier；直接实现 `MimeDetector` 的类型自行决定其行为。异步路径入口默认读取请求的内容
前缀。前缀后端可以在不加载完整资源的情况下检查大文件；声明
`ContentRequirement::Complete` 后端不能使用前缀入口。自定义完整内容后端可以通过覆盖独立的
`guess_from_async_provider_path` hook 检查异步 Provider 路径。Native command 检测可能需要
暂存输入，并受命令超时、保留输出大小和媒体暂存大小限制。
默认 `file` 和 `ffprobe` runner 还会限制 stdout 与 stderr 的总量，上限为
`MimeConfig::command_output_max_bytes()` 的四倍。因此每流上限为零时，总预算也为零，
任何输出都可能提前终止命令并返回 `OutputLimitExceeded`；命令错误仍保留部分捕获内容。
此策略仅适用于 `qubit-mime` 的这些默认 runner，不会改变 `rs-command` 的全局默认值，也
不会覆盖注入的 runner。注入 `CommandRunner` 的应用可以调用
`without_total_output_limit()` 保持总量不受限，或通过 `max_total_output_bytes(...)` 指定更大预算。

请把 MIME 检测结果作为上传策略的一项输入，不要仅凭返回的 MIME 名称就允许打开、执行
或信任文件内容。

## 延伸阅读

- [中文 README](../README.zh_CN.md) 与 [English README](../README.md)
- [英文用户手册](user_guide.md)
- [Rust API 文档](https://docs.rs/qubit-mime)
- [基础示例](../examples/basic.rs)
- [自定义 Provider 示例](../examples/custom_provider.rs)
