# qubit-mime 用户手册

[English user guide](user_guide.md) · [README](../README.zh_CN.md) ·
[API 文档](https://docs.rs/qubit-mime)

本手册面向使用 `qubit-mime` 0.22 检查上传文件、文件系统资源或媒体流的 Rust 应用。
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

`detect_bytes` 和 `detect_by_content` 面向完整内容。启用精确媒体检测并注入 classifier 时，
classifier 可能检查整份资源。只有前缀时请通过 `MimeDetector` 调用 `detect_prefix`；内置
检测器不会把前缀交给 classifier。精确检测必须同时开启配置并显式注入 classifier。

默认的 `Freedesktop` 策略还会使用当前仓库中的父子类型信息处理候选冲突。例如，文件名匹配
`text/html`、内容匹配其父类型 `text/plain` 时，检测器会返回更具体的文件名候选
`text/html`。比较前会把别名解析为规范类型；仓库中没有记录的类型关系不会被推断，未知名称
只有规范化后相同才视为相同类型。其他策略继续采用各自原有的选择规则。如果应用依赖旧版按
名称精确比较时的冲突结果，可选择 `ContentFirst`，并按应用自己的允许列表校验返回类型。

## 贯穿场景：检查上传文件

假设上传文件名是 `report.jpg`，但文件开头的字节表明它是 PDF。成功标准是返回内容
检测出的类型，而不是信任可能误导的扩展名：

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

可观察结果是 `Some("application/pdf")`。如果文件名和已检查的字节都没有匹配候选，
返回 `None` 也是正常结果。

## 安装与最小配置

在 `Cargo.toml` 中添加：

```toml
[dependencies]
qubit-mime = "^0.22"
```

`RepositoryMimeDetector::new()` 使用内置仓库，不需要外部命令。若要显式选择 Provider，
请创建 `Config`，将其解析为 `MimeConfig`，再解析 Provider 并创建检测器：

下面的示例使用 `Config`；实现应用自定义 Provider 时还会用到 `ServiceProvider`。完整的 Provider 配置应声明以下直接依赖：

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

`file` Provider 在检查内容时要求系统中存在 `file` 可执行文件。当它不可用且选择策略
允许 fallback 时，`repository` 可以继续提供检测服务。

Provider 选择链默认采用严格策略：链中的 Provider ID 若未在 registry 注册，整个解析会返回诊断，
不会静默跳过该项。若链中包含可能缺席的可选 Provider，请改用
`ProviderSelection::chain_allowing_missing`。格式无效的 selector 和空链仍会校验失败。

## 环境配置与整体回退

`MimeConfig::load()` 会读取环境变量并返回配置；进程级默认值的延迟初始化会调用这个方法。
只要有一个值无效，就会丢弃整份环境配置，改用完整的内置默认值；这个方法不会返回解析错误。例如，虽然
`QUBIT_MIME_MAX_BUFFER_SIZE=33554432` 有效，但若
`QUBIT_MIME_ENABLE_PRECISE_DETECTION=maybe` 无法解析，缓冲区上限也会一并回到内置默认值。
这种回退让默认配置仍可用，但也可能掩盖拼错或格式错误的设置。

进程级默认值会在首次需要时从环境变量初始化。之后修改环境变量不会自动更新已有快照。
如果应用要求启动时发现配置错误，应调用 `MimeConfig::from_env()?` 并向上传播错误。需要将
解析后的配置设为进程级默认值时，应在解析成功后再调用 `MimeConfig::set_default(config)`。
`MimeConfig::reload_default_from_env()?` 用于显式重载：成功时替换当前默认值；失败时返回错误，
并保留原有默认值。需要静默回退时使用 `load()`；需要看到配置错误时使用 `from_env()` 或显式重载。

精确媒体检测默认关闭。使用时必须同时将 `mime.enable.precise.detection` 设为 `true`，并向
检测器 core 显式注入 `MediaStreamClassifier`。仅打开配置不会自动创建或安装 `ffprobe`
classifier。

下面的函数面向已完整读取的上传内容：它开启精确检测，在调用检测器前创建并注入内置
`ffprobe` classifier。请将以下直接依赖加入 `Cargo.toml`，版本沿用本指南前文：

```toml
[dependencies]
qubit-mime = "^0.22"
qubit-config = "0.15"
qubit-spi = "0.14"
```

需在作用域中引入 `qubit_spi::ServiceProvider`，才能调用 provider 的
`create_configured` 方法。

<!-- mime-doc-example:precise-detection -->
```rust
fn detect_precisely(content: &[u8], filename: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut source = qubit_config::Config::new();
    source.set(qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)?;
    let config = qubit_mime::MimeConfig::from_config(&source)?;
    let classifier = qubit_mime::FfprobeCommandMediaStreamClassifierProvider.create_configured(&config)?;
    let mut detector = qubit_mime::RepositoryMimeDetector::from_mime_config(config);
    detector.core_mut().set_media_stream_classifier(Some(classifier));
    Ok(detector.detect_bytes(content, Some(filename), qubit_mime::MimeDetectionPolicy::VerifyContent)?)
}
```

没有 MIME 候选时，函数会返回 `None`。创建 classifier 时会检查 `ffprobe` 是否可用；
若缺少该命令，创建过程返回错误并由函数向上传播。应用应明确决定传播错误还是放弃精确分类。
不要把内容前缀传给此函数，因为前缀不会执行媒体分类。

## 核心工作流

根据输入类型选择最简单的入口：

- `detect_by_filename("photo.JPG")` 检查文件名 glob。
- `detect_by_content(bytes)` 检查内容 magic 规则。
- `RepositoryMimeDetector` 的 `detect_bytes(bytes, Some(filename), policy)` 使用完整资源字节组合两种结果；只有有界前缀时应通过 `MimeDetector` 调用 `detect_prefix`。
- `detect_reader(reader, filename, policy)` 按整份资源检查可 seek reader，并恢复 reader 的原始位置。
- `detect_file(path, policy)` 读取本地文件。
- `MimeDetector::detect_path` 和 `detect_async_path` 通过同步或异步文件系统 facade 读取。

文件名提示会同时把 `/` 和 `\` 视为分隔符，只有最后一段决定扩展名。例如，
`C:\cache.v1\clip` 没有扩展名，因此精确媒体检测会使用现有的 MIME 映射回退逻辑。实际打开本地
文件时仍遵循宿主平台的路径语义。修正此前对这类名称的误判后，以前被跳过的细分现在可能执行，
classifier 错误也可能向调用方传播；若名称含有扩展名但该扩展名未启用，仍不会细分。

顶层 `MimeDetector::detect_reader` 将 reader 视为整份资源：即使调用者当前游标不在开头，检测也从
逻辑字节零开始，结束后恢复原始位置。默认的 `MimeContentBackend::detect_reader` 契约不同，只从
当前位置读取，缓存所选窗口后把 `&[u8]` 传给 `detect_bytes`，不会调用 reader 专用 hook。`Prefix(n)` 最多读取 n 字节，
遇到 EOF 时提前停止。`Complete` 会在默认 16 MiB 缓冲预算（或覆盖后的预算）内读到 EOF，
并额外探测一个字节以检查超限。内容 backend 的两种模式
都会恢复各自的起始位置；读取和恢复都失败时，恢复错误优先。

这几种输入路径的约定不同。默认的 `MimeContentBackend::detect_reader` 会缓存所选窗口，
再把字节切片传给 `detect_bytes`；这个同步 hook 不接收 `InputExtent`。默认的
`MimeDetectorBackend::guess_from_reader` 也会按声明的 `content_requirement()` 读取窗口，
再把字节切片传给 `guess_from_content`；它返回候选和前缀字节，不返回 extent。只有自动受管读取普通
Provider 的分支，才会把实际交付的前缀保守标记为 `InputExtent::Prefix`。
`OwnedMimeInput::Bytes` 会保留调用方给出的字节和 `InputExtent`，不会暗中截短，也不计作 Provider I/O。
在自动受管 Provider 读取分支中，`content_requirement()` 会把 `Prefix(n)` 限制在请求窗口内，并继续受服务上限和
EOF 约束。扩展 backend 的完整读取
和同对象随机采样总是通过 `MeteredReadSession` 完成。扩展 backend 也可以选择让普通顺序前缀读取使用该
session；此时 backend 自行设置读取窗口，而 session 的每次请求受请求额度和祖先额度约束。这些 session
读取共享 operation 读取额度。内置受管 `file` Provider 则在自己的 staging 读取中计量。在
`PreferFilename` 策略下，唯一文件名匹配可以在受管检测中短路，因此无需读取 Provider。旧版 `detect_path`、
`detect_async_path` 的 `max_bytes` 参数仍是这些路径调用显式
指定的前缀上限，不会替代普通 reader hook 的 `content_requirement()`。


### 本地路径行为因 Provider 而异

`detect_file` 不会统一各 Provider 的符号链接策略。repository 检测会打开路径并跟随指向普通文件的
有效链接；悬空链接或目录会在打开/读取时出错。默认 `file` runner 将路径原样交给系统命令，不添加
`-L`，所以链接、悬空链接和目录的处理遵循当前命令的路径对象语义，可能因平台、`file` 版本和环境而异。
注入 executor 的行为由注入者决定。若应用要求一致的内容检测语义，应按自身链接策略打开普通文件，再用
reader 检测。拒绝链接应由应用在打开路径时执行；单独调用 metadata 检查仍无法消除竞态。

这些入口对应不同 backend 契约。默认 `MimeDetectorBackend` reader hook 按
`content_requirement()` 中声明的 `Prefix(n)` 读取；只有未覆盖该方法时，才由
`max_test_bytes()` 提供默认需求。backend 声明 `Complete` 时会返回
`CompleteContentRequired`。需要完整资源的 backend 必须覆盖
`guess_from_reader`，自行实现有界读取和位置恢复；若本地文件需要特殊处理，还应覆盖
`guess_from_file`。`MimeDetectorAdapter` 通过偏移零点对应调用者当前位置的视图委托给该 reader
hook；自定义随机访问覆盖仍可使用。adapter 的 Prefix 请求使用
`MimeConfig::max_buffer_size()` 预算。无文件名且同一 MIME 的精确检测映射对应不同 MIME 对时，
细化会保留原 MIME，且不会调用 classifier。有文件名扩展名时，该扩展名必须列在
`MimeConfig::precise_detection_patterns()` 中。没有扩展名时（包括未提供文件名），映射查询会忽略该集合，
并检查所有包含检测 MIME 的配置映射；只有这些映射都使用相同的视频/音频 MIME 对时才会细化。
因此，即使某个扩展名未启用，它的冲突映射也会阻止无扩展名输入的细化。传入匹配且已启用的文件名，
或移除冲突映射，才能明确选择细化对。
MIME 类型和子类型的比较忽略 ASCII 字母大小写，但配置的输出拼写会保留。例如，
`webm:Video/WebM,Audio/WebM` 可以匹配检测结果 `video/webm`，并按 classifier 结果返回配置中的拼写。
配对等价按位置比较：视频与视频比较，音频与音频比较。没有文件名且多个映射仅大小写不同，
会选择字典序最小的扩展名作为输出代表，因此不受映射表遍历顺序影响。真正不同的配对会跳过分类。
这可能使此前因大小写不同而未进入细化的输入开始调用 classifier，也可能传播其错误。

内置 `FileCommandMimeDetector` 检测 reader 时，最多暂存 `MimeConfig::max_buffer_size()` 字节，并在
EOF 时停止。该上限来自 MIME 配置；所选文件名仓库中的 magic 偏移不会决定读取量。`detect_by_content(bytes)`
仍接收调用方提供的字节，本地文件检测仍将本地路径传给 `file` 命令。超过配置预算的 reader 资源可能因
命令只看到暂存前缀而得到不同结果。如需调整 reader 预算，可使用现有
`mime.max.buffer.size` 配置。

`MimeDetectionReport::budget.staging_bytes` 表示本请求同时计入暂存预算的峰值，不是累计写入量，
也不是报告生成时仍存活的暂存量。并发请求分别计量。该值不代表清理或 operation settlement 已完成；
自定义 backend 若在内置暂存预算之外管理资源，应通过 `MimeCandidates` 提供自己的暂存用量。

这些入口都返回 `MimeResult<Option<String>>`。应先处理外层错误，再解释可选的 MIME 名称。

## 进阶用法

应用需要全局注册 Provider 时，可以使用 `MimeDetectorRegistry::global()`；测试或局部
应用可以使用 `MimeDetectorRegistry::builtin()` 或其他独立 Registry。应用自定义 Provider
需要实现 `ProviderMetadata` 和 `ServiceProvider<MimeDetectorSpec>`，完整的注册与解析流程
见 `examples/custom_provider.rs`。
启动配置完成后再调用 `seal()`。封闭状态由 Registry 克隆共享，之后同步和异步注册都会被拒绝，
已有 Provider 仍可读取和创建。异步创建会先快照选中的 Provider，再调用其工厂；回调不持有异步
注册表锁，回调期间注册的新 Provider 只影响后续创建。
同步和异步能力可以使用同一个 canonical ID，但两侧的每个 selector（ID 或 alias）都必须指向
同一个 ID。若原配置让两侧的同一 selector 指向不同 Provider，升级前应重命名其中一个 selector，
或让两种能力统一使用同一个 canonical ID。

需要在链接时发现 Provider，可以启用可选的 `inventory` feature：

```toml
[dependencies]
qubit-mime = { version = "^0.22", features = ["inventory"] }
qubit-magika = { version = "0.18", features = ["inventory"] }
```

应用代码还需引用 Provider crate（`use qubit_magika as _;`），使提交项参与链接。
此时 `MimeDetectorRegistry::builtin()` 能发现 `magika`，但默认仍选 `repository`；
分类器 Registry 也会发现已链接的提交项，并保留 `ffprobe` 作为默认选择。Provider
crate 通过 `qubit_spi::submit_sync_provider!` 向
`qubit_mime::detector::mime_detector_inventory` 或
`qubit_mime::classifier::media_stream_classifier_inventory` 下的 `Entry` 提交工厂。
重复的 selector 会导致 inventory 构建失败。未启用该 feature 时，仍按上文显式注册。

通过 registry 使用 Magika inventory，要求 `qubit-mime` 0.22 和 `qubit-magika` 0.18
均已发布。2026-11-24 的独立 registry manifest 检查确认 `qubit-mime` 0.22.0
尚未发布，因此当时还不满足该前提。版本发布齐全前，请使用下方固定源码组合。

### 固定源码接入 Magika

consumer fixture 与新应用使用两个相互独立的源码布局。下表记录的是早期验收所用的
历史 fixture 源码组合，不代表当前 CI 源码图。历史应用布局应先将 Magika 检出到
`d1f213ec3a5fa35a0aa15ad6ee9a7ee233e0b48f`，再准备其固定的兄弟依赖。
将 fixture 仓库放在单独的 `fixture-root/` 目录下：

```text
fixture-root/
├── rs-magika/
├── rs-mime/
├── rs-operation/
├── rs-fs/
├── rs-fs-local/
├── rs-fs-testkit/
├── rs-fs-registry/
└── rs-local-files/
```

| 仓库 | 历史 Fixture revision |
| --- | --- |
| `rs-mime` | `c05273cb8ac7ff8bb22d96c4385e38fb12b99d63` |
| `rs-magika` | `d78dd23a1b849ff8a3e3cbfcdd97def889ce0a7b`（仅用于 fixture）|
| `rs-operation` | `2821f739c996250fa9cfe33a86efc54c2fab6373` |
| `rs-fs` | `c7ea1d035498aa633f4f3c04d90fdb5d4b73de97` |
| `rs-fs-local` | `3bf4275ed601e39fdbf8532f9d80d563d9cc7182` |
| `rs-fs-registry` | `df6c8b7ce45495995c0b0ab9d64e14d0b41a8b1c` |
| `rs-fs-testkit` | `28bf195b8eef9c03d9cb1c5428186758b76aaacd` |
| `rs-local-files` | `5b52230e0af2997fadc45b5ff48be65b0ca5a70d` |

逐个 clone 仓库并检出对应的 fixture revision。例如：

```bash
git clone https://github.com/qubit-ltd/rs-mime.git
git -C rs-mime checkout --detach c05273cb8ac7ff8bb22d96c4385e38fb12b99d63
git clone https://github.com/qubit-ltd/rs-magika.git
git -C rs-magika checkout --detach d78dd23a1b849ff8a3e3cbfcdd97def889ce0a7b
```

其余仓库按表中 revision 检出。要核对 fixture 布局的 MIME 锁定数据，请在
`fixture-root/rs-mime` 中运行 MIME 准备检查：

```bash
cd fixture-root/rs-mime
./.infra/bin/prepare-local-path-dependencies.sh --check-locked
```

#### 当前 CI fixture

当前 workflow 使用触发 CI 的这份 `rs-mime` checkout，并将
[`rs-magika` 固定到远端可检出的提交 `432370c4dc5a6b5ae0c9faf467a2e272ef1f5e12`](https://github.com/qubit-ltd/rs-magika/commit/432370c4dc5a6b5ae0c9faf467a2e272ef1f5e12)。
其余兄弟仓库仍使用[workflow](../.github/workflows/magika-consumer.yml)中固定的版本。
运行下方命令前，请准备独立的 `current-fixture-root/` sibling 布局：将包含本指南的当前
`rs-mime` checkout 放在 `current-fixture-root/rs-mime`，并把兄弟仓库检出到 workflow
指定的固定提交。不要复用上方用于历史复现的 `fixture-root/`。例如，在
`current-fixture-root/` 中执行：

```bash
git clone https://github.com/qubit-ltd/rs-magika.git
git -C rs-magika checkout --detach 432370c4dc5a6b5ae0c9faf467a2e272ef1f5e12
```

其余兄弟仓库使用 workflow 中的固定提交：`rs-operation`
`893bcf6c7efb397cc8fac2f7fe18fb5e87017e63`、`rs-fs`
`c7ea1d035498aa633f4f3c04d90fdb5d4b73de97`、`rs-fs-registry`
`df6c8b7ce45495995c0b0ab9d64e14d0b41a8b1c`、`rs-local-files`
`5b52230e0af2997fadc45b5ff48be65b0ca5a70d`、`rs-fs-local`
`3bf4275ed601e39fdbf8532f9d80d563d9cc7182`、`rs-fs-testkit`
`28bf195b8eef9c03d9cb1c5428186758b76aaacd`。此源码图中的 `rs-mime` 使用当前
checkout，不绑定固定 SHA。

当前 MIME checkout 中的 fixture [manifest](../tests/fixtures/magika_inventory_consumer/Cargo.toml)
和[锁文件](../tests/fixtures/magika_inventory_consumer/Cargo.lock)通过相对路径引用源码：
`qubit-mime` 指向当前 checkout，`qubit-magika` 和 `qubit-operation` 分别指向同级的
`rs-magika` 和 `rs-operation`。本指南不固定 MIME 提交；CI 使用触发 workflow 的当前
checkout，因此不会依赖尚未产生的未来提交。从 `current-fixture-root/rs-mime` 根目录运行下方命令。

workflow 会在两种 feature 模式下检查 `qubit-operation` 只有一个包，并运行显式注册和
inventory 发现两条消费路径。从 `rs-mime` 根目录可用以下命令检查当前源码图：

```bash
cargo +1.94.0 metadata --locked --format-version 1 --manifest-path tests/fixtures/magika_inventory_consumer/Cargo.toml
cargo +1.94.0 tree --locked --manifest-path tests/fixtures/magika_inventory_consumer/Cargo.toml -i qubit-operation
cargo +1.94.0 check --locked --manifest-path tests/fixtures/magika_inventory_consumer/Cargo.toml
cargo +1.94.0 run --locked --manifest-path tests/fixtures/magika_inventory_consumer/Cargo.toml
cargo +1.94.0 metadata --locked --format-version 1 --manifest-path tests/fixtures/magika_inventory_consumer/Cargo.toml --features inventory
cargo +1.94.0 tree --locked --manifest-path tests/fixtures/magika_inventory_consumer/Cargo.toml --features inventory -i qubit-operation
cargo +1.94.0 check --locked --manifest-path tests/fixtures/magika_inventory_consumer/Cargo.toml --features inventory
cargo +1.94.0 run --locked --manifest-path tests/fixtures/magika_inventory_consumer/Cargo.toml --features inventory
```

运行 fixture 还需动态加载 ONNX Runtime 1.24.2。将 `ORT_LIBRARY_PATH` 指向共享库；Linux
用户还需按 workflow 的设置，将库目录加入 `LD_LIBRARY_PATH`。

#### 历史固定源码应用图

新应用接入固定源码时，应另建 `consumer-root/` 布局，并按 Magika 的
[固定依赖清单](https://github.com/qubit-ltd/rs-magika/blob/d1f213ec3a5fa35a0aa15ad6ee9a7ee233e0b48f/.infra/ci/local-path-dependencies.tsv)
检出版本；上面的 fixture revisions 仅用于复现 fixture，并不是应用使用的 Magika pins。
在 `consumer-root/rs-magika` 中，先运行 Magika 的[依赖准备脚本](https://github.com/qubit-ltd/rs-magika/blob/d1f213ec3a5fa35a0aa15ad6ee9a7ee233e0b48f/.infra/bin/prepare-local-path-dependencies.sh)，
补齐缺少的兄弟 checkout。然后仍从 Magika 仓库根目录运行只读的
[`check-locked-local-path-dependencies.sh`](https://github.com/qubit-ltd/rs-magika/blob/d1f213ec3a5fa35a0aa15ad6ee9a7ee233e0b48f/.infra/lib/check-locked-local-path-dependencies.sh)，
核验固定提交和工作区干净状态。脚本会报告提交不符或工作区有未提交改动；请在符合 Magika pins 的独立布局中处理漂移。
下方应用清单假设位于 `consumer-root/app/Cargo.toml`，兄弟 checkout 按该 Magika 固定依赖清单准备：

```toml
[dependencies]
qubit-magika = { path = "../rs-magika", default-features = false }
qubit-mime = { version = "^0.22", path = "../rs-mime", default-features = false }
qubit-operation = { version = "^0.3.2", path = "../rs-operation" }
qubit-spi = "^0.14"
ort = { version = ">=2.0.0-rc.12,<2.1", default-features = false, features = ["std", "ndarray", "load-dynamic", "api-24"] }

[patch.crates-io]
qubit-operation = { path = "../rs-operation" }

[features]
inventory = ["qubit-magika/inventory"]
```

相对路径以应用的 `Cargo.toml` 所在目录为基准。patch 会让传递依赖中的 crates.io
`qubit-operation` 也解析到同一份 checkout。普通模式会创建 `ManagedInferenceRuntime` 并显式注册
`MagikaMimeDetectorProvider`。fixture 直接引用 `qubit_magika` 类型，因此 inventory 模式能链接到其提交项；
自建应用若没有其它符号引用，也可用 `use qubit_magika as _;` 保留该 crate。随后
`MimeDetectorRegistry::builtin()` 能发现 `magika`。两种模式的默认选择都仍是 `repository`，应用必须显式选择
`magika`。完整、可复制的入口见[固定版本中的 consumer fixture](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/src/main.rs)。

历史 fixture 使用动态加载 ONNX Runtime。安装 1.24.2 共享库，并将 `ORT_LIBRARY_PATH` 指向库文件；
Linux 还需把库所在目录加入 `LD_LIBRARY_PATH`。
[固定版本的 consumer workflow](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/.github/workflows/magika-consumer.yml)
记录了经过校验的 wheel 下载地址和 SHA-256。以下命令用于复现历史固定图；在 `fixture-root/` 中，
使用该 fixture 自带的锁文件运行：

```bash
cargo +1.94.0 check --locked --manifest-path rs-mime/tests/fixtures/magika_inventory_consumer/Cargo.toml
cargo +1.94.0 run --locked --manifest-path rs-mime/tests/fixtures/magika_inventory_consumer/Cargo.toml
cargo +1.94.0 check --locked --manifest-path rs-mime/tests/fixtures/magika_inventory_consumer/Cargo.toml --features inventory
cargo +1.94.0 run --locked --manifest-path rs-mime/tests/fixtures/magika_inventory_consumer/Cargo.toml --features inventory
```

不启用 inventory 的命令走显式注册；启用时走 registry 发现并显式选择 Magika。两条路径都会检查默认 Provider
仍为 `repository`，并断言 Python 内容识别和 PDF 文件名识别结果。

fixture 自带 `Cargo.lock`。在独立的 `consumer-root/app` 布局中，新建应用通常没有锁文件，
应先为应用生成；随后以 `app/Cargo.toml` 为 manifest path，分别运行普通和 inventory 配置的
`check` 与 `run`。也要用同一 manifest 对两种配置运行 `cargo tree -i qubit-operation`；
每棵树都应只显示一份由 Magika 固定清单指定的 `rs-operation` checkout 中的 `qubit-operation`。

```bash
cargo +1.94.0 generate-lockfile --manifest-path app/Cargo.toml
cargo +1.94.0 tree --locked --manifest-path app/Cargo.toml -i qubit-operation
cargo +1.94.0 tree --locked --manifest-path app/Cargo.toml --features inventory -i qubit-operation
cargo +1.94.0 check --locked --manifest-path app/Cargo.toml
cargo +1.94.0 check --locked --manifest-path app/Cargo.toml --features inventory
cargo +1.94.0 run --locked --manifest-path app/Cargo.toml
cargo +1.94.0 run --locked --manifest-path app/Cargo.toml --features inventory
```

两条 `cargo tree` 都应只显示固定 `rs-operation` checkout 中的一份 `qubit-operation`。
fixture 的 [manifest](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/Cargo.toml)
和[锁文件](https://github.com/qubit-ltd/rs-mime/blob/c05273cb8ac7ff8bb22d96c4385e38fb12b99d63/tests/fixtures/magika_inventory_consumer/Cargo.lock)
可在固定版本中查看，但不会进入 crate package；`.github` 工作流也不打包。

Inventory 只改变发现方式：Magika 仍是同步 Registry Provider；它的
`detect_async_path` 便捷方法不会把任务关联到父级 `OperationScope`。带 scope 的
`MimeRuntime` 路径使用 Provider 级异步 registry，因此要求所选 Provider 明确支持异步。
内置 `repository` 和 `file` 支持该路径；选择 `magika` 会得到
`AsyncUnsupported`。启用 inventory 时仍须保留 `use qubit_magika as _;` 链接引用；关闭
inventory 时则显式注册 Magika。

需要更丰富的元数据时，直接使用 `MimeRepository` 访问 `MimeType` 的元数据、别名、注释、
文件扩展名、magic 规则和父类型关系。对于媒体扩展名有歧义的情况，可以配置媒体流
classifier；如果环境提供 `ffprobe`，再使用 `FfprobeCommandMediaStreamClassifier`。

加载 shared MIME-info XML 时，根元素和标准规则元素必须使用
`http://www.freedesktop.org/standards/shared-mime-info` 命名空间。其他命名空间的元素会作为扩展
忽略，其嵌套元素也不会被解析；标准元素中的非法属性仍会报错。标准 `magic` 元素仍必须至少包含一个
标准 `match` 元素。解析的标准子集包括 `mime-type`、`comment`、`alias`、`sub-class-of`、`glob`、
`magic` 和嵌套的 `match`；其他标准元素会被忽略。

嵌套 magic matcher 最多 128 层，根 matcher 计为第 1 层。超过限制的 XML 规则会返回
`InvalidXmlAttribute`，程序化构造的 matcher 会返回 `InvalidMagicMatcher`。可在保持匹配语义的前提下
减少冗余嵌套，或改写为等价的浅层 alternatives。

MIME 父类型继承关系使用迭代方式遍历，因此长继承链不会为每个类型消耗一个调用栈帧。
遍历所需的存储随已访问的图增长；这与上面的 128 层嵌套 matcher 限制是两项独立机制。

## 精确检测映射的配置错误

解析 `MimeConfig` 时会逐条校验 `mime.ambiguous.mime.mapping`。两端名称都必须符合已接受的
`type/subtype` token 语法。例如 `webm:video/webm,garbage` 会立即失败，因为音频侧名称缺少斜线；
不会等到 classifier 报告“仅音频”后才发现问题。错误为
`MimeError::InvalidConfigurationValue`：key 是 `CONFIG_MIME_AMBIGUOUS_MIME_MAPPING`，value 是
完整原始映射项，reason 会指出错误一侧及语法原因。合法自定义 MIME 名称无需存在于内置仓库，
也不要求以 `video/` 或 `audio/` 开头。

## 受管检测操作（0.22.0）

`MimeRuntime` 提供一条可选的受管提交路径，供已使用 `qubit-operation` 的应用接入。
输入采用 `OwnedMimeInput`，字节输入必须明确标记为 `Prefix` 或 `Complete`。应用创建
`MimeExecutionServices` 时需要传入自己的 `OperationManager`、MIME 操作组、
`MimeExecutionPort` 和有限且非零的累计读取上限；随后通过
`MimeRuntime::with_execution_services` 注入服务。根操作提交前要显式调用 `prepare()`。
准备阶段不会由 runtime 构造自动触发。
`prepare()` 正在执行时，根操作和子操作都会被拒绝；只有 execution port 成功返回后才会
发布 ready 状态。调用方应等待 `prepare()` 返回 `Ok(())` 后再提交任务。

下面的函数摘自可运行的
[`managed_lifecycle` 示例](../examples/managed_lifecycle.rs)。示例使用内存 Provider 保持运行结果稳定；
实际应用应传入自己的 `AsyncFileSystem` facade，并由应用长期持有 recovery store 和重试队列。

<!-- mime-doc-example:managed-lifecycle -->
```rust
fn managed_lifecycle(
    file_system: Arc<AsyncFileSystem>,
    recovery_store: &RecoveryStore,
    retry_queue: &mut Vec<OwnedMimeInput>,
) {
    let manager = OperationManager::new(ManagerOptions::default()).expect("operation manager");
    let group = manager
        .group(GroupOptions { capacity: 8 })
        .expect("MIME operation group");
    let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 8)), 1024 * 1024)
        .expect("execution services");
    services.prepare().expect("prepare MIME services");

    let runtime = MimeRuntime::builtin()
        .expect("built-in repository")
        .with_execution_services(services.clone());
    let input = OwnedMimeInput::Provider {
        file_system,
        path: Path::parse("/upload/report.pdf").expect("provider path"),
    };
    let mut operation = match runtime.try_submit_detection(input, MimeOperationOptions::default()) {
        Ok(operation) => operation,
        Err(failure) => {
            eprintln!("submission rejected: {:?}", failure.reason);
            retry_queue.push(failure.input);
            let shutdown = services.shutdown();
            let settled = block_on(shutdown.wait_services_settled()).expect("settle service operations");
            assert!(settled.operations_settled);
            let stopped = shutdown.stop_services().expect("stop MIME services");
            assert!(stopped.services_stopped);
            let report = block_on(shutdown.wait_group_shutdown()).expect("drain MIME group");
            assert_eq!(report.snapshot.active_operations, 0);
            return;
        }
    };

    let outcome_issue = match block_on(operation.wait_outcome()) {
        Ok(OperationOutcome::Succeeded(report)) => {
            assert_eq!(report.mime_type.as_deref(), Some("application/pdf"));
            assert_eq!(report.budget.bytes_read, 9);
            println!(
                "{}: {:?} ({:?}, {} bytes read)",
                report.backend_id, report.mime_type, report.inspected_extent, report.budget.bytes_read,
            );
            None
        }
        Ok(OperationOutcome::Failed(error)) => {
            let diagnostic = format!("{error:?}");
            eprintln!("unexpected PDF detection failure: {error}");
            Some(diagnostic)
        }
        Ok(other) => {
            eprintln!("unexpected PDF detection outcome: {other:?}");
            Some(format!("{other:?}"))
        }
        Err(error) => Some(format!("outcome observation failed: {error:?}")),
    };

    let settlement = block_on(operation.wait_settlement());
    let settlement_released = settlement.all_released();
    let settlement_has_no_recovery = settlement.recoveries.is_empty();
    drop(operation);

    let (failure_observed, failure_released, failure_recoveries) = controlled_failure(&runtime);

    let shutdown = services.shutdown();
    let settled = block_on(shutdown.wait_services_settled()).expect("settle tracked operations");
    assert!(settled.operations_settled);
    let mut recoveries = shutdown.recovery_handles_snapshot();
    recoveries.extend(settlement.recoveries.iter().cloned());
    recoveries.extend(failure_recoveries);
    process_recoveries(recoveries, recovery_store);
    let stopped = shutdown
        .stop_services()
        .expect("stop MIME services from this sync context");
    assert!(stopped.services_stopped && stopped.unresolved_recoveries == 0);
    let report = block_on(shutdown.wait_group_shutdown()).expect("wait for MIME group Drain");
    assert_eq!(report.snapshot.active_operations, 0);
    assert!(
        settlement_released && settlement_has_no_recovery,
        "sample PDF settlement should release all resources"
    );
    assert!(failure_observed, "controlled MIME error should be visible");
    assert!(failure_released, "failed detection resources should be settled");
    assert!(
        outcome_issue.is_none(),
        "sample PDF outcome should succeed: {outcome_issue:?}"
    );
}
```

提交拒绝分支把原输入放回调用方的重试队列。任务接纳后，代码分别观察 MIME 报告与 operation
settlement，再释放 typed handle，最后等待组 Drain。若存在未解决的 recovery handle，示例会在 worker
停止前按 recovery phase 尝试处理或转交给应用长期持有的 recovery store。共享入口还运行受控读取失败，
检查错误可见且 settlement 仍确认资源释放；独立恢复用例确认 transfer 被 store 接收，再由 store 重试成功。
暂存清理回归还验证了另一种时序：转交内建暂存 recovery 后，先完全停止原 MIME 服务，再由应用持有的
`RecoveryStore` 重试并完成清理。此服务停止后的库内恢复路径仅保证内建暂存 recovery；其备用清理 port
由进程持有，随 MIME 进程生命周期结束。需要继续重试时，`RecoveryStore` 必须独立于 MIME 服务存活；
自定义清理应在服务停止前完成，或自行保留可继续工作的依赖。
完整样例可通过 `cargo run --locked --example managed_lifecycle` 运行，CI 的 consumer fixture 也运行同一入口。

`MimeRuntime::builtin()` 默认使用内置 repository。调用 `with_execution_services` 附加执行服务时，
也会默认安装 repository 受管异步 backend；如需其他异步实现，可再用
`with_async_provider_selection` 替换它。为异步 backend 解析仅支持同步的 Provider 工厂时，会在
runtime 配置阶段以 `AsyncUnsupported` 失败，早于任务提交。内置 `repository` 和 `file` Provider 都提供受管异步
backend。`file` 要求完整内容；
它不支持随机访问，只表示不能处理要求随机访问的输入。注册 Provider 不会启动检测任务。
未配置执行服务时，提交会以 `ExecutionServicesRequired` 拒绝。上文仅支持同步的 Provider 所致
`AsyncUnsupported` 是 runtime 配置错误，并非提交失败。另一个独立入口 `try_submit_detection_sync_in`
会以 reason 为 `MimeSubmitRejection::AsyncUnsupported` 的 `MimeSubmitFailure`
拒绝 Provider 输入，并返还原输入；该同步子操作入口可接收字节输入。
其他提交前拒绝也会通过 `MimeSubmitFailure` 原样返还 `OwnedMimeInput`，调用方可以重试或改走其他入口。任务接纳后
发生的检测错误则由 operation handle 以 `OperationOutcome::Failed` 报告，不再通过
`MimeSubmitFailure` 返还输入。资源释放状态需要另行等待 settlement。
现有 `FileCommandMimeDetector` 同步 API 仍可使用；受管 backend 通过 `MimeRuntime` 执行
operation 管理的任务。
若为受管 `file` factory 附加任何 async classifier，runtime 配置阶段会以
`UnsupportedCapability` 拒绝，即使输入不会触发 classifier 精化也一样。
受管 `file` backend 在内部 staging 流程中计量 Provider 读取；扩展 backend 的
`ManagedProvider` session 契约见下文。
旧版异步内容 hook 返回 `AsyncUnsupported`；若需让 operation 跟踪 staging、worker 执行和清理，
应使用受管路径。

受管 `repository` Provider 对标记为 `Prefix` 的输入只执行基础 MIME 匹配；即使精细分类映射命中，
也不会把此前缀交给异步媒体 classifier。需要媒体流精化时，应以 `Complete` 提交完整字节。
完整输入的 classifier 错误仍由 operation 报告。

### 扩展 backend 的受计量 Provider 读取

异步扩展 backend 若对 `BorrowedMimeInput` 做穷尽匹配，需要处理新增的
`ManagedProvider` 变体；该枚举现在也不再实现 `Copy` 或 `Clone`。对于此变体，runtime 只打开一次
Provider 对象，并向 backend 借出 `MeteredReadSession`。顺序读取使用 `session.read()`；要从同一已打开
对象采样偏移时使用 `session.read_at()`。处理受管输入时不要重新打开路径或使用原始 filesystem handle。

声明完整读取或 `RandomAccessCapability::SamplesSameObject` 的 backend 始终会收到该受计量 session。
旧 `uses_managed_provider_reads()` hook 仍可供选择顺序 session 读取的 `Prefix(n)` backend 使用；
返回 `false` 不能关闭完整读取或随机访问的计量。常规 `Prefix(n)` backend 收到的是最多 n 字节的窗口，不保证输入至少有 n 字节。请求或服务的静态上限可以缩短窗口，
EOF 也会使实际字节变少。提交阶段会预留 owned 缓冲区；预留失败时拒绝提交并返还原输入。任务接纳后，Provider 读取还需
从共享祖先树预留额度；若额度已被其他已接纳操作消耗，当前 operation 会以 `MimeError::BudgetExceeded { kind: "read", .. }` 失败。读取不会超过
最终窗口。若 backend 需要最低字节数，必须检查实际长度并自行返回类型化错误。

顺序读取应持续调用 `read()`，直到返回零以确认 EOF；单次短读不代表 EOF。调用 `read_at(offset, buffer)`
可从同一个已打开对象采样。每次调用单独计费，重复或重叠的采样也会计费。session 会将单次调用裁切到请求与祖先的
剩余额度；若读取额度已耗尽，或无法取得独立的 owned-memory scratch 许可，则返回相应的 typed budget error。
不要重新打开路径或改用未受计量的 handle。

```rust,ignore
let mut buffer = [0; 4096];
loop {
    let count = session.read(&mut buffer).await?;
    if count == 0 { break; } // EOF
    inspect(&buffer[..count]);
}

let mut sample = [0; 512];
let count = session.read_at(1024, &mut sample).await?;
inspect_sample(&sample[..count]);
```

每次 session 读取都会受调用方 buffer、payload 上限、请求与祖先读取额度以及 8 KiB 分块上限共同约束，并可裁切到
当前可用额度。runtime 会另行预留服务级 owned-memory 许可，用于 Provider I/O 前的私有 scratch，再把成功返回的字节复制给
backend。Provider 错误不会暴露暂存区中的部分数据。EOF 返回零；读取额度按实际返回字节计费，重复或重叠的
`read_at` 采样会重复计费。每次读取的 scratch 上限与 backend 保留的输入内存或命令暂存内存是不同预算。

handle 提供两种彼此独立的观察方式。`wait_outcome()` 等待 MIME 业务结果；成功时结果包含
`MimeDetectionReport`。业务成功可能先于清理完成，因此成功结果不能证明资源已经释放。
`wait_settlement()` 返回资源处置报告；`SettlementReport.status` 为
`SettlementStatus::Released`、`SettlementStatus::Transferred` 或
`SettlementStatus::RecoveryRequired`，分别表示资源已释放、已转交或需要恢复。报告中的 recovery handle
代表应用或其 store 后续仍需处理的清理责任；转交 handle 只会转移重试责任，不表示清理已经完成。
提前得到 Stopped 或 Failed 也不能证明暂存文件和后台 helper 已退出；丢弃 handle 或 future 也不代表任务
已取消。报告包含 MIME 结果、backend 标识、检查范围和预算摘要，不能替代 operation 的
settlement 诊断。`InputExtent` 说明调用方提供的是前缀还是完整输入；`InspectedExtent::Prefix(n)`
说明交给内容匹配的有界连续窗口，不是逐字节访问计数。因此完整输入也可能对应前缀检查范围。

需要为受管操作设置时限时，在 `MimeOperationOptions.operation.context.deadline` 中配置 scope
deadline。异步检测会同时等待 deadline、stop signal 和 backend future；即使 provider read 不会自行唤醒，
deadline 也能使该等待结束。scope deadline 通过 `OperationOutcome::Stopped` 通道报告
`StopReason::Timeout`，不是 `OperationOutcome::Failed`，也不是 settlement 结果。若已接受的用户取消请求与 deadline
同时发生，结果保留 `UserCancelled` 原因。repository matcher 只在协作批次边界检查停止状态，不会在批次中途
被强制打断。deadline 和 stop 还会在 backend 已就绪、结果提交之前再次裁决，因此已过期的 deadline
可以使成功或错误结果转为 `Stopped(Timeout)`。同步子操作也不会被强制中断，只能在自身的协作检查点观察 stop。

临时文件创建若在失败前已生成部分文件，文件系统错误可能携带仍需清理的 owner。runtime 会在
返回创建错误前，把该 owner 作为清理资源登记到同一 operation scope。发生此类错误后仍应观察
settlement 和 recovery；仅看到失败 outcome 不能确认部分文件已删除。
已经派发给 command 或 staging worker 的工作也遵循同一规则：其资源 owner 会持续跟踪到 job 实际结算。

已有 `qubit-operation::OperationScope` 时，可调用 `try_submit_detection_in` 创建子操作；它沿用
父操作的 manager 视图并绑定 MIME 组。同步子操作入口为
`try_submit_detection_sync_in`。若 Provider 输入无法接纳（包括同步子操作不支持该输入类型），
`MimeSubmitFailure` 会返还原输入，供调用方重试。任务一旦接纳，执行错误由 operation outcome
报告；资源释放情况需另行等待 settlement。Provider 能力如下：

嵌套 MIME operation 按祖先身份和适用 scope cap 共享 Provider 读取计量。owned bytes 输入和由唯一文件名短路
完成的输入不会绑定祖先读取树，也不预留祖先的资源槽。首个需要 Provider 读取的后代会从共享读取树预留额度。
若准入前无法绑定，或祖先已结束，提交会拒绝并返还原输入；若任务接纳后待处理绑定提交失败，则 operation outcome
报告 `ReadBudgetBinding`。

读取额度只按成功交付的字节计费。Provider 打开失败计零；前缀读取若在错误或取消前已返回部分字节，已交付字节
继续计费，未用额度会退还。普通 `Prefix(n)` 声明的是最大输入窗口，不是最小长度。提交预检会按请求和服务的 buffer/read 上限缩短窗口；
Provider 遇到 EOF 时，实际返回字节也可能更少。若预检无法预留 owned 前缀缓冲区，提交会被拒绝并返还原输入。
任务接纳后，其他操作可能先耗尽共享祖先读取额度；此时预留失败会由已接纳 operation 以 `MimeError::BudgetExceeded { kind: "read", .. }` 报告。
`MimeBudgetSummary.bytes_read` 只统计本请求发起并由 MIME 计量路径成功交付的 Provider 字节；重复或重叠读取会重复计数，
后代请求的字节记在后代自己的报告中。扩展代码绕过 `MeteredReadSession` 执行的 Provider I/O 不计入此值，
但祖先额度仍会约束整棵读取树的实际用量。
扩展 backend 可选择 `MeteredReadSession`：每次读取按当前可用额度裁切，临时 scratch 内存另行预留。
内置受管 `file` Provider 则在自己的 staging 读取中计量。

| Provider | 受管异步 backend | 输入与精化限制 |
| --- | --- | --- |
| `repository` | 支持 | 根据提供的字节匹配仓库规则；不支持随机访问输入。 |
| `file` | 支持，须显式选择 | 要求完整内容，不支持随机访问。任何附加的异步 classifier 都会在 factory 创建时以 `UnsupportedCapability` 拒绝。 |
| 仅同步的自定义 Provider | 不支持 | 为异步选择解析其工厂时，会在 runtime 配置阶段以 `AsyncUnsupported` 失败；不会回退为同步执行。 |

路径输入会按 native path 语义提取 basename，再交给专用文件名匹配入口；完整路径不会被重新当作文件名解析。
POSIX basename 内的反斜杠仍是文件名内容；非 UTF-8 本地 basename 会以 lossy 字符串进行匹配。
`OwnedMimeInput::Bytes` 仍使用输入中由调用方提供的 filename。

选择命令 Provider 不保证外部命令可用或运行成功；任务接纳后的运行问题由 operation 报告。
Classifier 也需要单独显式选择异步实现并满足同一对象能力要求；不能假定每种 detector 选择都能
使用 `ffprobe`。

关闭与重试顺序请参照可运行的
[`managed_lifecycle` 示例](../examples/managed_lifecycle.rs)。普通路径会先观察 outcome 和 settlement，
释放 operation handle，再等待组 Drain。示例的 Pending 路径同时保留组级 service resource 和一次清理失败
产生的 recovery。关闭时有两种不同的 Pending：若 `stop_services()` 返回
`OperationShutdownPending` 且 `snapshot.group` 尚无值，说明组 Drain 尚未发布；服务准备、操作结算或准入
可能仍在进行，也可能有另一个关闭尝试正在执行。操作结算后，关闭尝试会先发布 Drain，再可能因组资源或 recovery
未解决而返回 Pending。此时 MIME worker 仍可接收任务。示例先等待操作结算，所以其首次 Pending 属于后一种情况。
未解决操作、资源或 recovery 导致的 Pending 期间，execution port 仍可用；`stop_services()` 成功返回表示端口已停止。
若另一个关闭尝试仍在执行，应检查 `snapshot.services_stopped`。成功后完整组 Drain 仍可能因调用方持有 typed handle
而继续等待。等待完整 Drain 前应释放这些 handle 和阻塞中的 recovery。应用检查 shutdown snapshot，把 recovery 转交给自己长期持有的
`RecoveryStore`，关闭 service resource scope 并释放资源引用；观察 shutdown snapshot，直到 service resource
清理结束，再重试 `stop_services()`，最后等待组 Drain。
转交成功后，重试责任由 `RecoveryStore` 承担；若之后还要恢复，store 及其重试机制必须继续运行。
库保证的服务关闭后重试路径仅适用于内建暂存 recovery；其备用清理 port 由进程持有，随 MIME 进程生命周期
结束。组 Drain 尚未完成时，MIME execution port 仍可供 MIME 自己的 command 和 staging 清理使用。
自定义清理器应在 `stop_services()` 成功前完成，或保留端口停止后仍可独立使用的依赖。不要在仍持有
typed operation handle 或
未解决 recovery 时等待完整 Drain。组准入关闭前应先让所需的外部 scope 进入；关闭后，已接纳的父 scope
仍可派发后代，且可能在早先快照之后产生新的 recovery。每次重试前都刷新 `snapshot()` 和
`recovery_handles_snapshot()`，再处理或转交当前所有未解决项。Pending 表示工作尚未解决，不能据此确认资源已释放；
`stop_services()` 不会自动重试 recovery。

受管操作各阶段的结果应分别处理：累计读取上限为零时，构造 execution services 会返回
`InvalidOperationBudget`；选择仅支持同步的
backend 时，在提交前返回 `AsyncUnsupported`。提交遇到服务未准备、能力不匹配或准入关闭等问题，会以
`MimeSubmitFailure` 拒绝并返还原 `OwnedMimeInput`。任务接纳之后，Provider 或 backend 检测失败经
operation outcome 报告为 `OperationOutcome::Failed`，不是提交拒绝。scope deadline 则经
`OperationOutcome::Stopped` 报告 `StopReason::Timeout`，不是 Failed outcome。
清理处置由 settlement 报告。业务成功也可能先于清理完成，因此不能据此确认资源已释放。
通过 settlement 报告检查资源处置：`SettlementStatus::Released`、
`SettlementStatus::Transferred` 或 `SettlementStatus::RecoveryRequired`。
其中 recovery handle 表示后续清理责任；转交 handle 只转移重试责任，不表示资源已清理。再根据 snapshot
解决或转交 recovery、释放 typed handle 和组资源，从 worker pool 外重试停止。关闭返回
`OperationShutdownPending` 时，应检查 `snapshot().group`：尚无值表示
Drain 未发布；有值则表示 Drain 已发布，但组资源或 recovery 仍可能未解决。每次重试前刷新 shutdown 和
recovery 快照，处理当前所有 recovery。只有 `stop_services()` 成功返回才确认 execution port 已停止；完整组
Drain 之后仍可能等待调用方释放 typed handle。
`FixedMimeExecutionPort::shutdown()` 必须从 worker pool 之外的同步上下文调用。并发关闭调用会等待同一批
worker 实际退出，并观察相同的最终结果。

## 错误与诊断

常见的 `MimeError` 包括：

- `Io` 或 `FileSystem`：输入无法读取；
- `BufferLimitExceeded`：路径前缀请求超过 `MimeConfig::max_buffer_size()`；
- `CompleteContentRequired`：后端要求完整资源；
- `AsyncUnsupported`：后端没有异步内容 hook；
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
前缀。内置旧 `RepositoryMimeDetector` 是例外：在 `PreferFilename` 下，唯一文件名候选仍会直接返回；
依赖内容的策略则会在打开 Provider 路径前返回 `AsyncUnsupported`。自定义
`MimeDetectorBackend` 默认保留原有行为。覆盖 `supports_async_content_detection` 并返回 `false` 可在读取前拒绝；
需要检查内容时则提供异步内容 hook。前缀后端可以在不加载完整资源的情况下检查大文件；声明
`ContentRequirement::Complete` 后端不能使用前缀入口。自定义完整内容后端可以通过覆盖独立的
`guess_from_async_provider_path` hook 检查异步 Provider 路径。受管异步操作使用 runtime 配置的 backend；
附加执行服务时默认使用 `repository`。解析仅支持同步的 Provider 工厂会在 runtime 配置阶段以
`AsyncUnsupported` 失败，不会在异步流程中回退到同步执行。受管路径要求注入执行服务和有限预算，并将业务
结果与资源 settlement 分开观察。丢弃进行中的 future 不能证明已接纳工作停止；释放资源前应观察
settlement。前缀读取期间可能同时保留 Provider 缓冲区和 worker 的 owned 副本；`max_buffer_size`
限制单个缓冲区，不限制两者合计内存。
Native command 检测可能需要
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
