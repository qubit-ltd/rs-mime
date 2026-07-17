# Qubit MIME

[![Rust CI](https://github.com/qubit-ltd/rs-mime/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-mime/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-mime/coverage-badge.json)](https://qubit-ltd.github.io/rs-mime/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-mime.svg?color=blue)](https://crates.io/crates/qubit-mime)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

面向 Rust 服务的 MIME 类型检测工具，基于文件名 glob 规则和内容魔数规则。

## 概述

Qubit MIME 是一个基于仓库的 Rust MIME 类型检测库。它使用 freedesktop
shared MIME-info 数据模型：规范 MIME 类型名、别名、本地化说明、文件名
glob、内容魔数规则和父类型关系。

公开 API 分为三层：

- `MimeDetector`：顶层检测器 trait。业务代码需要兼容不同检测器实现时依赖它。
- `detector`：检测器实现和共享检测逻辑，包括
  `MimeDetectorCore`、`MimeDetectorBackend`、`RepositoryMimeDetector` 和
  `FileCommandMimeDetector`。
- `MediaStreamClassifier`：顶层媒体流分类 trait。`classifier` 模块提供
  `FfprobeCommandMediaStreamClassifier`、`MediaStreamClassifierBackend` 和
  `FileBasedMediaStreamClassifier`，用于用更少重复入口代码实现 stream-backed
  或 file-backed classifier。
- `MimeRepository`：底层仓库 API，返回 `MimeType` 元数据和所有最佳候选项，适合
  需要进一步检查规则和说明的调用方。

## 设计目标

- **Freedesktop 数据模型**：遵循 shared MIME-info 名称、别名、glob 规则和
  magic 规则。
- **实用默认值**：内置 freedesktop MIME 数据库。
- **文件名与内容检测**：同时支持 glob 检测和 magic 检测，可单独使用也可组合使用。
- **检测器与分类器层次**：分离 MIME 检测和媒体流细化逻辑，并保留 Rust 的
  所有权与错误处理方式。
- **可预测的冲突处理**：优先选择更高 glob 权重、更长 glob 模式和更高 magic 优先级。
- **符合 Rust 习惯**：使用借用仓库、具体错误类型和标准 `Read + Seek` 检测入口。
- **依赖面小**：运行时依赖保持聚焦和稳定。

## 特性

### 文件名检测

- 支持 literal、extension 和通用 glob 匹配。
- 默认大小写不敏感，同时支持大小写敏感 glob。
- 按 glob 权重和模式长度解决冲突。
- 对路径安全，只使用最后一个文件名组件参与检测。

### 内容魔数检测

- 支持 freedesktop magic 类型：`string`、`byte`、`host16`、`host32`、
  `big16`、`big32`、`little16` 和 `little32`。
- 支持 `0`、`0:1024` 等 offset 范围。
- 支持二进制 magic 的可选 mask。
- 支持嵌套 magic matcher。
- 按 magic 优先级解决冲突。

### 仓库元数据

- 规范名称和别名。
- 本地化 comment 与 description。
- 推荐扩展名和完整扩展名列表。
- 解析 `sub-class-of` 父类型元数据。
- 计算 magic 规则需要读取的最大字节数。

### 检测入口

- 顶层 trait：`MimeDetector`。
- 仓库检测器：`RepositoryMimeDetector`。
- 系统命令检测器：`FileCommandMimeDetector`。
- 仅文件名：`detect_by_filename`。
- 仅内容：`detect_by_content`。
- 文件名与字节组合：`detect` 或 `detect_bytes`。
- 文件名与 reader 组合：`detect_reader`。
- 本地文件路径：`detect_file`。

### 媒体流分类

- 顶层 trait：`MediaStreamClassifier`。
- 流结果枚举：`MediaStreamType`。
- FFprobe 实现：`FfprobeCommandMediaStreamClassifier`。
- 当 `MimeDetectorCore` 配置了 classifier 时，可对 WebM、Ogg 等有歧义的
  媒体 MIME 类型做更精确的音视频区分。

## 安装

在 `Cargo.toml` 中添加：

```toml
[dependencies]
qubit-mime = "0.9"
```

## 快速开始

### 根据文件名和内容检测

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

### 使用 Rust 风格的 `MimeDetector` trait

`MimeDetectorRegistry` 先解析 Provider，再由该 Provider 创建共享的
`MimeDetector` trait object。Provider 选择与 `MimeConfig` 相互独立，因此既没有
selection、也没有 config 的代码可以同时使用 Registry 默认值和配置默认值。

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
    let provider = MimeDetectorRegistry::global().resolve_default()?;
    let detector = provider.create_default()?;

    assert_eq!(
        Some("application/pdf".to_owned()),
        detect_upload(detector.as_ref(), "upload.bin", b"%PDF-1.7\n"),
    );
    Ok(())
}
```

### 使用 `Config` 配置全局默认值

`MimeConfig::default()` 返回当前全局默认 MIME 配置的快照。使用
`MimeConfig::reload_default()` 可以从 `rs-config` 的 `Config` 替换全局默认值；
使用 `MimeConfig::reload_default_from_env()` 可以通过 `Config::from_env()` 从
进程环境载入。

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
    let provider = registry.resolve(mime_config.mime_detector_selection())?;
    let detector = provider.create(&mime_config)?;

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf"),
    );

    MimeConfig::set_default(original);
    Ok(())
}
```

### 使用 registry 和 fallback 选择检测器

`MimeDetectorRegistry::global()` 是进程级领域 Registry。App 可在启动时注册自描述
第三方 Provider，并替换 Registry 默认 `ProviderSelection`。此后任意下游库调用
`global().resolve_default()` 都会观察到同一份 App 配置，而无需知道具体实现。

选择与创建刻意分成两步。`resolve(selection)` 和 `resolve_default()` 返回
`ResolvingServiceProvider<MimeDetectorSpec>`，只会产生
`ProviderSelectionError`。随后既可调用 `create(&MimeConfig)`，也可调用
`create_default()`；创建失败由 `ProviderCreationError` 表达。
`MimeConfig::mime_detector_selection()` 仍可作为显式 selection 的一种可选来源，
但 Registry 不依赖该字段。

`ProviderSelection` 自身携带 fallback policy。默认 `OnAbsence` 策略会在 Provider
报告不支持或不可用时继续；`auto` 按优先级降序、canonical Provider ID 升序排列。
`MimeDetectorRegistry::builtin()` 创建隔离 Registry，适合测试或局部应用。

SPI 类型属于 `qubit-spi`，`qubit-mime` 不会重新导出。第三方 Provider 同时实现
`ServiceProvider<MimeDetectorSpec>` 与 `ProviderDefinition<MimeDetectorSpec>`，并由
`descriptor()` 自己返回注册元数据。

内置 detector selector：

| Selector | 别名 | 行为 |
|----------|------|------|
| `repository` | `repository-mime-detector` | 使用内置 freedesktop MIME 仓库 |
| `file` | `file-command`, `file-command-mime-detector` | 文件名使用仓库，内容使用 `file --mime-type --brief` |
| `auto` | - | 按优先级和 provider id 选择可用 provider |

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
    let provider = registry.resolve(config.mime_detector_selection())?;
    let detector = provider.create(&config)?;

    assert_eq!(
        Some("image/png".to_owned()),
        detector.detect_by_filename("image.png"),
    );
    Ok(())
}
```

完整的 App 启动注册/库 X 默认获取场景如下。App 负责注册和默认选择，库 X
只负责使用服务：

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
    fn create(
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
            ProviderId::new("app-detector").expect("静态 ID 合法"),
        )
    }
}

// 此函数模拟独立发布的库 X 中的代码。
fn library_x_detector() -> Result<Arc<dyn MimeDetector>, Box<dyn std::error::Error>> {
    let provider = MimeDetectorRegistry::global().resolve_default()?;
    Ok(provider.create_default()?)
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

若需要隔离 Provider 集合，可选 builder 也采用同一个单参数注册契约：

```rust
use qubit_mime::{
    MimeDetectorRegistry,
    RepositoryMimeDetectorProvider,
};
use qubit_spi::{ProviderSelection, ServiceProvider};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = MimeDetectorRegistry::builder();
    builder.register(RepositoryMimeDetectorProvider)?;
    let registry = builder.build();
    let selection = ProviderSelection::named("repository-mime-detector")?;
    let detector = registry.resolve(&selection)?.create_default()?;

    assert_eq!(
        Some("text/plain".to_owned()),
        detector.detect_by_filename("notes.txt"),
    );
    Ok(())
}
```

Registry 选择和服务创建使用不同的 SPI 错误类型：

| 错误 | 阶段 | 含义 |
|------|------|------|
| `RegistrationError` | 注册 | Provider ID 或 alias 与已有 Provider 冲突 |
| `ProviderSelectionError` | 解析 | 显式或默认 selection 没有得到候选 Provider |
| `ProviderCreationError` | 创建 | 已选候选创建失败，或 fallback policy 停止遍历 |

### 配置键

`MimeConfig::from_config()` 同时接受逻辑键和环境变量风格键。环境变量使用环境变量
风格名称。列表值既可以是数组，也可以是用 `,` 或 `;` 分隔的字符串；空项会被忽略。
有歧义 MIME 映射按 `;` 分隔，每项格式为 `extension:video-mime,audio-mime`。

| 设置 | 逻辑键 | 环境键 | 默认值 |
|------|--------|--------|--------|
| 默认 MIME detector | `mime.detector.default` | `QUBIT_MIME_DETECTOR_DEFAULT` | `repository` |
| MIME detector fallback | `mime.detector.fallbacks` | `QUBIT_MIME_DETECTOR_FALLBACKS` | 空 |
| 媒体流 classifier | `mime.media.stream.classifier.default` | `QUBIT_MEDIA_STREAM_CLASSIFIER_DEFAULT` | `ffprobe` |
| 媒体流临时 staging 上限 | `mime.media.stream.max.staging.size` | `QUBIT_MEDIA_STREAM_MAX_STAGING_SIZE` | `67108864` |
| 启用精确检测 | `mime.enable.precise.detection` | `QUBIT_MIME_ENABLE_PRECISE_DETECTION` | `true` |
| 精确检测扩展名 | `mime.precise.detection.patterns` | `QUBIT_MIME_PRECISE_DETECTION_PATTERNS` | `webm,ogg` |
| 有歧义 MIME 映射 | `mime.ambiguous.mime.mapping` | `QUBIT_MIME_AMBIGUOUS_MIME_MAPPING` | `webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg` |
| detector 最大 buffer 大小 | `mime.max.buffer.size` | `QUBIT_MIME_MAX_BUFFER_SIZE` | `16777216` |

### 检测文件系统路径

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

### 使用系统 `file` 命令检测器

`FileCommandMimeDetector` 使用内置仓库做文件名候选检测，并使用
`file --mime-type --brief` 做内容检测。

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

### 使用 FFprobe 分类媒体流

`FfprobeCommandMediaStreamClassifier` 可以把媒体文件分类为无媒体流、纯音频、
纯视频或音视频都有。

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

### 从可 seek 的 reader 检测

`detect_reader` 会读取仓库中 magic 规则所需的最大字节数，然后恢复 reader 原来的
流位置。

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

## 组合检测策略

组合检测同时接收文件名和内容字节。`MimeDetectionPolicy` 会让每次调用都显式声明
文件名结果和内容结果的取舍策略。

```rust
use qubit_mime::{
    MimeDetectionPolicy,
    MimeError,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let detector = RepositoryMimeDetector::new()?;
    let pdf_bytes = b"%PDF-1.7\n";

    // 优先使用确定的文件名结果，避免额外检查内容。
    let by_filename = detector.detect_bytes(
        pdf_bytes,
        Some("photo.jpg"),
        MimeDetectionPolicy::PreferFilename,
    );
    assert_eq!(Some("image/jpeg".to_owned()), by_filename);

    // 当内容更可信时，明确检查 magic。
    let by_magic = detector.detect_bytes(
        pdf_bytes,
        Some("photo.jpg"),
        MimeDetectionPolicy::VerifyContent,
    );
    assert_eq!(Some("application/pdf".to_owned()), by_magic);

    Ok(())
}
```

当文件名来自可信来源且希望减少 I/O 时，使用
`MimeDetectionPolicy::PreferFilename`。当文件名来自用户上传或可能被伪造时，使用
`MimeDetectionPolicy::VerifyContent` 更稳妥。

## 仓库元数据

默认检测器会暴露已解析的仓库。当你不仅需要 MIME 名称，还需要扩展名、别名或说明时，
可以直接使用仓库 API。

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

`MimeRepository::detect_by_filename` 和 `MimeRepository::detect_by_content`
会返回所有最佳候选项，而不是只返回一个字符串：

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

## 自定义仓库

当需要小型测试仓库、产品自定义 MIME 数据库，或使用其他来源生成的数据库时，可使用
`MimeRepository::from_xml`。

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

## 底层规则类型

底层类型适合在测试或集成代码中直接检查、构造或验证 MIME 规则。

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

## API 参考

### `MimeDetector`

| 方法 | 描述 |
|-----|------|
| `MimeDetectorRegistry::global()` | 获取 App 与下游库共享的进程级 Registry |
| `MimeDetectorRegistry::builtin()` | 创建包含内置 detector Provider 的隔离 Registry |
| `MimeDetectorRegistry::register(provider)` | 在运行时注册自描述 Provider |
| `MimeDetectorRegistry::register_shared(provider)` | 注册已经共享的自描述 Provider |
| `MimeDetectorRegistry::resolve(selection)` | 解析显式 selection，但不创建 detector |
| `MimeDetectorRegistry::resolve_default()` | 不依赖 MIME config 解析 Registry 默认值 |
| `ResolvingServiceProvider::create(config)` | 使用显式服务配置创建 detector |
| `ResolvingServiceProvider::create_default()` | 使用默认服务配置创建 detector |
| `MimeDetectorRegistry::provider_ids()` | 按注册顺序列出 canonical Provider ID |
| `MimeDetectorProvider` | 可插拔 detector 实现的工厂 trait |
| `detect_by_filename(filename)` | 根据文件名检测一个 MIME 名称 |
| `detect_by_content(bytes)` | 根据内容字节检测一个 MIME 名称 |
| `detect(bytes, filename, policy)` | 根据字节和可选文件名检测 |

### `RepositoryMimeDetector`

| 方法 | 描述 |
|-----|------|
| `new()` | 创建使用内置 freedesktop 仓库的检测器 |
| `with_repository(repository)` | 创建借用显式仓库的检测器 |
| `with_repository_and_config(repository, config)` | 创建借用显式仓库和 MIME 配置的检测器 |
| `repository()` | 借用底层仓库 |
| `detect_by_filename(filename)` | 返回文件名匹配到的第一个 MIME 名称 |
| `detect_by_content(bytes)` | 返回内容 magic 匹配到的第一个 MIME 名称 |
| `detect_bytes(bytes, filename, policy)` | 根据字节和可选文件名检测 |
| `detect_reader(reader, filename, policy)` | 从 `Read + Seek` reader 检测并恢复位置 |
| `detect_file(file, policy)` | 打开并检测本地文件路径 |

### `FileCommandMimeDetector`

| 方法 | 描述 |
|-----|------|
| `new()` | 创建使用内置仓库和系统 `file` 命令的检测器 |
| `with_repository(repository)` | 创建借用显式仓库的检测器 |
| `with_repository_and_runner(repository, runner)` | 使用显式 `qubit_command::CommandRunner` 创建检测器 |
| `with_repository_runner_and_config(repository, runner, config)` | 使用显式仓库、runner 和 MIME 配置创建检测器 |
| `command_runner()` | 借用用于命令执行的 runner |
| `set_command_runner(runner)` | 替换用于命令执行的 runner |
| `is_available()` | 检查 `file` 命令是否可执行 |
| `detect_file_by_content(file)` | 只根据命令输出检测本地文件 |
| `detect_file(file, policy)` | 根据文件名和命令支持的内容检测来检测本地文件 |
| `detect_reader(reader, filename, policy)` | 通过 file-backed 路径检测可 seek reader |

### `MediaStreamClassifier`

| 方法 | 描述 |
|-----|------|
| `MediaStreamClassifierRegistry::global()` | 获取进程级 classifier Registry |
| `MediaStreamClassifierRegistry::builtin()` | 创建包含内置 classifier Provider 的隔离 Registry |
| `MediaStreamClassifierRegistry::register(provider)` | 注册自描述 classifier Provider |
| `MediaStreamClassifierRegistry::register_shared(provider)` | 注册已经共享的自描述 classifier Provider |
| `MediaStreamClassifierRegistry::resolve(selection)` | 解析显式 classifier selection |
| `MediaStreamClassifierRegistry::resolve_default()` | 独立于 MIME config 解析 Registry 默认值 |
| `MediaStreamClassifierRegistry::provider_ids()` | 按注册顺序列出 canonical Provider ID |
| `MediaStreamClassifierProvider` | 可插拔 classifier 实现的工厂 trait |
| `classify_file(file)` | 分类本地媒体文件 |
| `classify_reader(reader)` | 从 reader 分类媒体内容 |
| `classify_content(bytes)` | 分类内存中的媒体内容 |

### `FfprobeCommandMediaStreamClassifier`

| 方法 | 描述 |
|-----|------|
| `new()` | 创建基于 FFprobe 的 classifier |
| `is_available()` | 检查 `ffprobe` 是否可执行 |
| `classify_stream_listing(output)` | 分类 FFprobe `codec_type` 输出 |
| `set_working_directory(directory)` | 设置命令工作目录 |

### `MimeRepository`

| 方法 | 描述 |
|-----|------|
| `from_xml(xml)` | 解析 freedesktop shared MIME-info XML 文档 |
| `empty()` | 创建空仓库 |
| `all()` | 按数据库顺序返回所有 MIME 类型 |
| `get(name)` | 解析规范 MIME 名称或别名 |
| `max_test_bytes()` | 返回 magic 规则需要的最大内容前缀长度 |
| `detect_by_filename(filename)` | 返回最佳文件名候选项 |
| `detect_by_content(bytes)` | 返回最佳内容候选项 |
| `detect(filename, bytes, policy)` | 合并文件名和内容检测 |

### 元数据与规则类型

| 类型 | 用途 |
|-----|------|
| `MimeType` | 单个 MIME 类型的元数据和规则 |
| `MimeTypeBuilder` | 构造独立 `MimeType` 值的 builder |
| `MimeGlob` | 带权重和大小写敏感设置的文件名 glob 规则 |
| `MimeMagic` | 带优先级的一组 magic matcher |
| `MimeMagicMatcher` | 带 offset、value、mask 和子 matcher 的单个 magic 匹配器 |
| `MagicValueType` | freedesktop magic value type 枚举 |
| `MediaStreamType` | 音视频流分类结果 |
| `MimeConfig` | 精确检测和有歧义媒体映射配置 |
| `MimeError` | XML 解析、规则校验和 I/O 错误类型 |

### `MimeConfig`

| 方法 | 描述 |
|-----|------|
| `from_config(config)` | 从 `qubit_config::Config` 解析 MIME 配置 |
| `from_env()` | 从 `Config::from_env()` 解析 MIME 配置 |
| `default()` | 克隆当前全局默认 MIME 配置 |
| `set_default(config)` | 替换未来默认实例使用的全局默认配置 |
| `reload_default(config)` | 从 `Config` 解析并替换全局默认配置 |
| `reload_default_from_env()` | 从进程环境解析并替换全局默认配置 |
| `mime_detector_default()` | 读取配置的 detector selector |
| `mime_detector_fallbacks()` | 读取配置的 detector fallback 链 |
| `media_stream_classifier_default()` | 读取配置的媒体 classifier selector |
| `media_stream_max_staging_size()` | 读取媒体 classifier 的 reader/content staging 上限 |

## 模块结构

源码结构按 detector、classifier 和 repository 职责组织：

```text
src/
  mime_detector.rs              # 顶层 MimeDetector trait
  mime_config.rs                # 精确检测配置
  detector/                     # detector 实现
  classifier/                   # 媒体流 classifier 接口与实现
  repository/                   # MIME 数据库、glob、magic 和元数据类型
```

普通业务代码优先使用 crate root 的 re-export。只有在需要查看或扩展具体 detector、
classifier 或 repository 组件时，才需要直接使用嵌套模块。

## 检测规则

### 文件名规则

文件名检测只使用路径的最后一个组件。匹配结果按以下顺序排序：

1. glob 权重更高者优先。
2. 权重相同时，glob 模式更长者优先。
3. 仓库 API 在多个候选项完全并列时保留数据库顺序。

### 内容规则

内容检测会用传入的字节前缀检查每个 MIME 类型的直接 magic 规则。结果按更高
magic 优先级排序。可使用 `repository.max_test_bytes()` 获取当前仓库最值得读取的
最大前缀长度。

### 组合规则

组合检测会先执行文件名 glob 检测。当文件名只有一个匹配项且未强制检查 magic 时，
直接使用该文件名结果。否则继续执行内容 magic 检测，并与文件名候选项合并。

## 与 Java `common-mime` 对比

| 方面 | Java `common-mime` | Qubit MIME |
|-----|--------------------|------------|
| 数据库模型 | Freedesktop shared MIME-info | 相同模型 |
| 文件名检测 | Glob 规则 | Glob 规则 |
| 内容检测 | Magic 规则 | Magic 规则 |
| 别名查找 | 支持 | 支持 |
| 检测器接口 | `MimeDetector` | `MimeDetector` trait |
| 媒体流分类接口 | `MediaStreamClassifier` | `MediaStreamClassifier` trait |
| 仓库检测器 | `RepositoryMimeDetector` | `RepositoryMimeDetector` |
| file 命令检测器 | `FileCommandMimeDetector` | `FileCommandMimeDetector` |
| FFprobe classifier | `FfprobeCommandMediaStreamClassifier` | `FfprobeCommandMediaStreamClassifier` |
| 仓库加载 | XML 资源 | 内置 XML 或显式 XML |
| 返回风格 | Java 对象与集合 | Rust `Option`、slice 和 vector |
| 错误处理 | Java 异常 | 具体 `MimeError` |

## 测试

```bash
# 使用默认的空 feature 集测试核心 API
cargo test --no-default-features

# 测试核心 API 和正则校验
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
