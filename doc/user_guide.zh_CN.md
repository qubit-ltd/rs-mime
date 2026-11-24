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
当前位置读取。`Prefix(n)` 最多读取 n 字节，提前遇到 EOF 时停止；`Complete` 在默认 16 MiB
缓冲预算（或覆盖后的预算）内读到 EOF，并额外探测一个字节以检查超限。内容 backend 的两种模式
都会恢复各自的起始位置；读取和恢复都失败时，恢复错误优先。


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

## 受管检测操作（0.22.0 目标）

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
    assert!(settlement_released && settlement_has_no_recovery, "sample PDF settlement should release all resources");
    assert!(failure_observed, "controlled MIME error should be visible");
    assert!(failure_released, "failed detection resources should be settled");
    assert!(outcome_issue.is_none(), "sample PDF outcome should succeed: {outcome_issue:?}");
}
```

提交拒绝分支把原输入放回调用方的重试队列。任务接纳后，代码分别观察 MIME 报告与 operation
settlement，再释放 typed handle，最后等待组 Drain。若存在未解决的 recovery handle，示例会在 worker
停止前按 recovery phase 尝试处理或转交给应用长期持有的 recovery store。共享入口还运行受控读取失败，
检查错误可见且 settlement 仍确认资源释放；独立恢复用例确认 transfer 被 store 接收，再由 store 重试成功。
完整样例可通过 `cargo run --example managed_lifecycle` 运行，CI 的 consumer fixture 也运行同一入口。

`MimeRuntime::builtin()` 默认使用内置 repository。调用 `with_execution_services` 附加执行服务时，
也会默认安装 repository 受管异步 backend；如需其他异步实现，可再用
`with_async_provider_selection` 替换它。仅支持同步的 Provider 会在该选择配置阶段以
`AsyncUnsupported` 拒绝，发生在提交任务之前。内置 `repository` 和 `file` Provider 都提供受管异步
backend。`file` 要求完整内容；
它不支持随机访问，只表示不能处理要求随机访问的输入。注册 Provider 不会启动检测任务。
未配置执行服务时，提交会以 `ExecutionServicesRequired` 拒绝；所选 backend
没有异步能力时会以 `AsyncUnsupported` 拒绝。这些提交前拒绝都通过
`MimeSubmitFailure` 原样返还 `OwnedMimeInput`，调用方可以重试或改走其他入口。任务接纳后
发生的检测错误则由 operation handle 报告。
现有 `FileCommandMimeDetector` 同步 API 仍可使用；受管 backend 用于通过 `MimeRuntime` 执行
operation 管理的任务。
若为 `file` 选择 async classifier，factory 会在 runtime 配置阶段以 `UnsupportedCapability` 拒绝，
即使某个输入本身不会触发 classifier 精化也一样。

受管 `repository` Provider 对标记为 `Prefix` 的输入只执行基础 MIME 匹配；即使精细分类映射命中，
也不会把此前缀交给异步媒体 classifier。需要媒体流精化时，应以 `Complete` 提交完整字节。
完整输入的 classifier 错误仍由 operation 报告。

### 扩展 backend 的受计量 Provider 读取

异步扩展 backend 若对 `BorrowedMimeInput` 做穷尽匹配，需要处理新增的
`ManagedProvider` 变体；该枚举现在也不再实现 `Copy` 或 `Clone`。对于此变体，runtime 只打开一次
Provider 对象，并向 backend 借出 `MeteredReadSession`。顺序读取使用 `session.read()`；要从同一已打开
对象采样偏移时使用 `session.read_at()`。处理受管输入时不要重新打开路径或使用原始 filesystem handle。

声明完整读取或 `RandomAccessCapability::SamplesSameObject` 的 backend 始终会收到该受计量 session。
旧 `uses_managed_provider_reads()` hook 仍可供明确需要顺序 session 读取的 `Prefix(n)` backend 选择使用；
返回 `false` 不能关闭完整读取或随机访问的计量。常规 `Prefix(n)` backend 会收到 `Bytes`，最多包含 n
字节，并受请求与服务的 buffer/read 预算进一步限制；若提前遇到 EOF，实际字节会更少，读取不会越过前缀上限。

顺序读取应持续调用 `read()`，直到返回零以确认 EOF；单次短读不代表 EOF。调用 `read_at(offset, buffer)`
可从同一个已打开对象采样。每次调用单独计费，重复或重叠的采样也会计费。若读取超过请求或祖先的剩余额度，
调用会返回读取预算错误；应将其作为检查失败处理，不要重新打开路径或改用未受计量的 handle。

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

每次 session 读取都会受调用方 buffer、payload 上限、请求与读取额度树剩余额度以及 8 KiB 分块上限共同约束。
runtime 在 Provider I/O 前为私有暂存区预留最多 8 KiB，并只把成功返回的字节复制给 backend。Provider
错误不会暴露暂存区中的部分数据。EOF 返回零；读取额度按实际返回字节计费，重复或重叠的
`read_at` 采样会重复计费。每次读取的暂存区上限与 backend 保留的输入内存或命令暂存内存是不同预算。

handle 提供两种彼此独立的观察方式。`wait_outcome()` 等待 MIME 业务结果；成功时结果包含
`MimeDetectionReport`。`wait_settlement()` 观察资源是否已释放或恢复责任是否仍待处理。提前得到
Stopped 或 Failed 并不能证明暂存文件和后台 helper 已退出；丢弃 handle 或 future 也不代表任务
已取消。报告包含 MIME 结果、backend 标识、检查范围和预算摘要，不能替代 operation 的
settlement 诊断。`InputExtent` 说明调用方提供的是前缀还是完整输入；`InspectedExtent::Prefix(n)`
说明交给内容匹配的有界连续窗口，不是逐字节访问计数。因此完整输入也可能对应前缀检查范围。

需要为受管操作设置时限时，在 `MimeOperationOptions.operation.context.deadline` 中配置 scope
deadline。异步检测会同时等待 deadline、stop signal 和 backend future；即使 provider read 不会自行唤醒，
deadline 也能使该等待结束，operation 会报告 `Stopped(Timeout)`。若用户取消请求已被接受并与 deadline
竞争，结果保留 `UserCancelled` 原因。repository matcher 只在协作批次边界检查停止状态，不会在批次中途
被强制打断。同步子操作也不会被强制中断，只能在自身的协作检查点观察 stop。

临时文件创建若在失败前已生成部分文件，文件系统错误可能携带仍需清理的 owner。runtime 会在
返回创建错误前，把该 owner 作为清理资源登记到同一 operation scope。发生此类错误后仍应观察
settlement 和 recovery；仅看到失败 outcome 不能确认部分文件已删除。
已经派发给 command 或 staging worker 的工作也遵循同一规则：其资源 owner 会持续跟踪到 job 实际结算。

已有 `qubit-operation::OperationScope` 时，可调用 `try_submit_detection_in` 创建子操作；它沿用
父操作的 manager 视图并绑定 MIME 组。同步子操作入口为
`try_submit_detection_sync_in`。Provider 能力如下：

同一外部父操作下的所有 MIME 子操作共享一个累计 Provider 读取额度；即使前一个子操作已完成
settlement 且 handle 已释放，后续子操作也会继续使用父操作原有额度。首次子操作将累计树上限固定为
请求上限与服务上限中的较小值，后续子操作不能扩大它。MIME 会在父操作资源域中保留一个读取预算锚点，
因此父操作至少需要一个可用资源条目。资源不足、父操作已停止或已结束时，提交会以
`ReadBudgetBinding` 及对应资源错误种类拒绝，并通过 failure 原样归还输入。独立根操作和不同父操作
使用彼此独立的累计额度。

| Provider | 受管异步 backend | 输入与精化限制 |
| --- | --- | --- |
| `repository` | 支持 | 根据提供的字节匹配仓库规则；不支持随机访问输入。 |
| `file` | 支持，须显式选择 | 要求完整内容，不支持随机访问。附加要求同一对象精化的 classifier 会以 `UnsupportedCapability` 拒绝。 |
| 仅同步的自定义 Provider | 不支持 | 异步选择返回 `AsyncUnsupported`，不会回退为同步执行。 |

路径输入会先提取 basename，再交给专用文件名匹配入口；完整路径不会被重新当作文件名解析。
`OwnedMimeInput::Bytes` 仍使用输入中由调用方提供的 filename。

选择命令 Provider 不保证外部命令可用或运行成功；任务接纳后的运行问题由 operation 报告。
Classifier 也需要单独显式选择异步实现并满足同一对象能力要求；不能假定每种 detector 选择都能
使用 `ffprobe`。

调用 `MimeExecutionServices::shutdown()` 会关闭新的根提交并返回可重复观察的 observer。先用
`wait_services_settled()` 等待已跟踪操作完成 settlement。若需要恢复，通过
`recovery_handles_snapshot()` 获取 operation group 中仍未解决的 handle，并在停止服务前完成恢复或转交。
新的外部操作应在组准入关闭前完成接纳；也可以从已接纳到 MIME 组中的父 operation scope
派发必要后代。
组准入关闭后，新的外部 scope 不能首次加入，但已接纳的组内父 scope 仍可继续派发其后代。
随后在外部同步上下文调用 `stop_services()`，再用 `wait_group_shutdown()` 等待组 Drain。
Pending 表示工作仍未解决，不是资源已释放的确认；调用方仍持有 typed handle 时，组关闭也可能继续等待。
操作或 recovery 未解决时，`stop_services()` 会返回 `OperationShutdownPending`，且不会自动重试恢复。
`FixedMimeExecutionPort::shutdown()` 必须从 worker pool 之外的同步上下文调用。并发关闭调用会等待同一批
worker 实际退出，并观察相同的最终结果。
0.22.0 目标仍处于验收阶段；已验证行为和未完成的生命周期检查见[验收记录](acceptance-0.22.0-operation-0.2.zh_CN.md)。

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
`guess_from_async_provider_path` hook 检查异步 Provider 路径。受管异步操作必须显式选择异步 backend；仅支持同步的 backend 会返回
`AsyncUnsupported`，不会在异步流程中回退到同步执行。受管路径要求注入执行服务和有限预算，并将业务
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
