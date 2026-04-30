# Qubit MIME

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-mime.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-mime)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-mime/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-mime?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-mime.svg?color=blue)](https://crates.io/crates/qubit-mime)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

面向 Rust 服务的 MIME 类型检测工具，基于文件名 glob 规则和内容魔数规则。

## 概述

Qubit MIME 是一个基于仓库的 Rust MIME 类型检测库。它使用与 Java
`common-mime` 模块相同的 freedesktop shared MIME-info 数据模型：
规范 MIME 类型名、别名、本地化说明、文件名 glob、内容魔数规则和父类型关系。

本 crate 参考 Java `common-mime` 的设计，同时暴露符合 Rust 习惯的类型。
公开 API 分为三层：

- `MimeDetector`：顶层检测器 trait，对应 Java 的 `MimeDetector` 接口。业务代码
  需要兼容不同检测器实现时依赖它。
- `detector`：Java 风格的检测器实现和共享检测逻辑，包括
  `AbstractMimeDetector`、`RepositoryMimeDetector`、`FileCommandMimeDetector`、
  `StreamBasedMimeDetector` 和 `FileBasedMimeDetector`。
- `MediaStreamClassifier`：顶层媒体流分类 trait，对应 Java 的
  `MediaStreamClassifier` 接口。`classifier` 模块提供
  `FfprobeCommandMediaStreamClassifier`、`AbstractMediaStreamClassifier` 和
  file-based 辅助实现。
- `MimeRepository`：底层仓库 API，返回 `MimeType` 元数据和所有最佳候选项，适合
  需要进一步检查规则和说明的调用方。

## 设计目标

- **对齐 Java**：行为和数据库模型参考 Java `common-mime` 实现。
- **实用默认值**：内置 freedesktop MIME 数据库。
- **文件名与内容检测**：同时支持 glob 检测和 magic 检测，可单独使用也可组合使用。
- **检测器与分类器层次**：对齐 Java detector/classifier 拆分，同时保留 Rust 的
  所有权和错误处理方式。
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
- 文件系统路径：`detect_path`。

### 媒体流分类

- 顶层 trait：`MediaStreamClassifier`。
- 流结果枚举：`MediaStreamType`。
- FFprobe 实现：`FfprobeCommandMediaStreamClassifier`。
- 当 `AbstractMimeDetector` 配置了 classifier 时，可对 WebM、Ogg 等有歧义的
  媒体 MIME 类型做更精确的音视频区分。

## 安装

在 `Cargo.toml` 中添加：

```toml
[dependencies]
qubit-mime = "0.1.0"
```

## 快速开始

### 根据文件名和内容检测

```rust
use qubit_mime::{
    MimeError,
    RepositoryMimeDetector,
};

fn main() -> Result<(), MimeError> {
    let detector = RepositoryMimeDetector::new()?;

    let by_name = detector.detect_by_filename("photo.JPG");
    assert_eq!(Some("image/jpeg".to_owned()), by_name);

    let by_content = detector.detect_by_content(b"%PDF-1.7\n");
    assert_eq!(Some("application/pdf".to_owned()), by_content);

    let combined = detector.detect_bytes(b"%PDF-1.7\n", Some("report.pdf"), true);
    assert_eq!(Some("application/pdf".to_owned()), combined);

    Ok(())
}
```

### 使用 Rust 风格的 `MimeDetector` trait

`Box<dyn MimeDetector>` 实现了 `Default`，会根据配置和后端可用性返回 boxed
detector。只需要 MIME 名称的代码可以依赖 trait，而不是依赖具体检测器类型。

```rust
use qubit_mime::{
    BoxMimeDetector,
    MimeDetectionPolicy,
    MimeDetector,
};

fn detect_upload(detector: &dyn MimeDetector, filename: &str, content: &[u8]) -> Option<String> {
    detector.detect(content, Some(filename), MimeDetectionPolicy::VerifyContent)
}

fn main() {
    let detector = BoxMimeDetector::default();

    assert_eq!(
        Some("application/pdf".to_owned()),
        detect_upload(detector.as_detector(), "upload.bin", b"%PDF-1.7\n"),
    );
}
```

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
    let detected = detector.detect_path(&path, MimeDetectionPolicy::VerifyContent)?;
    std::fs::remove_file(&path).ok();

    assert_eq!(Some("application/pdf".to_owned()), detected);
    Ok(())
}
```

### 使用系统 `file` 命令检测器

`FileCommandMimeDetector` 对应 Java 中的 file-command detector。它使用内置仓库
做文件名候选检测，并使用 `file --mime-type --brief` 做内容检测。

```rust,no_run
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
    Ok(())
}
```

### 使用 FFprobe 分类媒体流

`FfprobeCommandMediaStreamClassifier` 对应 Java 中的 FFprobe classifier。它可以把
媒体文件分类为无媒体流、纯音频、纯视频或音视频都有。

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
    let stream_type = classifier.classify_path(Path::new("sample.webm"))?;

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
| `BoxMimeDetector::default()` | 选择配置或默认的 boxed 检测器 |
| `BoxMimeDetector::from_name(name)` | 按实现名称选择 boxed 检测器 |
| `ArcMimeDetector::default()` | 选择配置或默认的共享检测器 |
| `ArcMimeDetector::from_name(name)` | 按实现名称选择共享检测器 |
| `detect_by_filename(filename)` | 根据文件名检测一个 MIME 名称 |
| `detect_by_content(bytes)` | 根据内容字节检测一个 MIME 名称 |
| `detect(bytes, filename, policy)` | 根据字节和可选文件名检测 |

### `RepositoryMimeDetector`

| 方法 | 描述 |
|-----|------|
| `new()` | 创建使用内置 freedesktop 仓库的检测器 |
| `with_repository(repository)` | 创建借用显式仓库的检测器 |
| `repository()` | 借用底层仓库 |
| `detect_by_filename(filename)` | 返回文件名匹配到的第一个 MIME 名称 |
| `detect_by_content(bytes)` | 返回内容 magic 匹配到的第一个 MIME 名称 |
| `detect_bytes(bytes, filename, policy)` | 根据字节和可选文件名检测 |
| `detect_reader(reader, filename, policy)` | 从 `Read + Seek` reader 检测并恢复位置 |
| `detect_path(path, policy)` | 打开并检测文件系统路径 |

### `FileCommandMimeDetector`

| 方法 | 描述 |
|-----|------|
| `new()` | 创建使用内置仓库和系统 `file` 命令的检测器 |
| `with_repository(repository)` | 创建借用显式仓库的检测器 |
| `is_available()` | 检查 `file` 命令是否可执行 |
| `detect_path_by_content(path)` | 只根据命令输出检测本地文件 |
| `detect_path(path, policy)` | 根据文件名和命令支持的内容检测来检测路径 |
| `detect_reader(reader, filename, policy)` | 通过 file-backed 路径检测可 seek reader |
| `set_execution_timeout(timeout)` | 保存与 Java API 对齐的命令超时设置 |
| `set_working_directory(directory)` | 设置命令工作目录 |

### `MediaStreamClassifier`

| 方法 | 描述 |
|-----|------|
| `default_media_stream_classifier()` | 后端可用时返回默认 classifier |
| `<dyn MediaStreamClassifier>::default_classifier()` | Java 风格的默认 classifier 构造入口 |
| `classify_path(path)` | 分类本地媒体路径 |
| `classify_content(bytes)` | 分类内存中的媒体内容 |

### `FfprobeCommandMediaStreamClassifier`

| 方法 | 描述 |
|-----|------|
| `new()` | 创建基于 FFprobe 的 classifier |
| `is_available()` | 检查 `ffprobe` 是否可执行 |
| `classify_stream_listing(output)` | 分类 FFprobe `codec_type` 输出 |
| `set_execution_timeout(timeout)` | 保存与 Java API 对齐的命令超时设置 |
| `set_working_directory(directory)` | 设置命令工作目录 |
| `set_disable_logging(value)` | 保存与 Java API 对齐的关闭日志标志 |

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

## 模块结构

源码结构有意对齐 Java 实现：

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

## 测试与代码覆盖率

本项目测试统一放在 `tests/` 目录下，覆盖仓库解析、文件名匹配、内容 magic 匹配、
reader/path 检测和覆盖率阈值。

### 运行测试

```bash
# 运行所有测试
cargo test

# 生成覆盖率报告
./coverage.sh

# 生成文本格式覆盖率报告
./coverage.sh text

# 运行 CI 检查（格式化、clippy、测试、文档、覆盖率、audit）
./ci-check.sh
```

## 依赖项

运行时依赖保持很少：

- `regex` 用于编译和运行文件名 glob 匹配器。
- `roxmltree` 用于解析 shared MIME-info XML。
- `thiserror` 用于实现具体的 `MimeError`。

## 许可证

Copyright (c) 2026. Haixing Hu, Qubit Co. Ltd. All rights reserved.

根据 Apache 许可证 2.0 版（"许可证"）授权；
除非遵守许可证，否则您不得使用此文件。
您可以在以下位置获取许可证副本：

<http://www.apache.org/licenses/LICENSE-2.0>

除非适用法律要求或书面同意，否则根据许可证分发的软件
按"原样"分发，不附带任何明示或暗示的担保或条件。
有关许可证下的特定语言管理权限和限制，请参阅许可证。

完整的许可证文本请参阅 [LICENSE](LICENSE)。

## 贡献

欢迎贡献。请保持改动与现有 Rust 项目结构一致，并在提交 Pull Request 前运行
`./ci-check.sh`。

## 作者

**胡海星** - *Qubit Co. Ltd.*

## 相关项目

Qubit 旗下的更多 Rust 库发布在 GitHub 组织
[qubit-ltd](https://github.com/qubit-ltd)。

---

仓库地址：[https://github.com/qubit-ltd/rs-mime](https://github.com/qubit-ltd/rs-mime)
