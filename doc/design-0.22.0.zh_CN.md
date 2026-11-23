# rs-mime 0.22.0 详细设计：可取消的异步检测与受控后端组合

- 状态：历史设计草案，不再作为当前实施依据。当前用法见[英文用户指南](user_guide.md)和[中文用户指南](user_guide.zh_CN.md)；实现状态与验收证据见[0.22.0 验收记录](acceptance-0.22.0-operation-0.2.zh_CN.md)。
- 初稿日期：2026-11-11；缺失恢复与上游核对：2026-11-13。
- 初稿时版本：qubit-mime 0.21.1；计划目标：0.22.0。
- 初稿时上游基线：qubit-command 0.12.0、qubit-local-files 0.6.0；Magika 适配对应 qubit-magika 0.19.0。

## 0. 实施前置条件（历史约束）：先实现 rs-operation

**本设计当时要求先实现并验收 rs-operation 0.1.0 的基础协议，再开始库实现。** 只有设计文档或空 crate 不满足此前置条件；起草阶段仅修订设计，没有修改实现或 Cargo 清单。

本节引用的 rs-operation 0.1.0 设计仅记录当时的前置设想，不再是当前契约入口。当前行为和实施证据见上方用户指南与验收记录。以下列出的同步/异步观察、取消、子操作登记、资源清理、恢复所有权及结果与释放分离要求保留为历史设计背景；不得据此推断当前公开 API 名称或版本。

### 0.1 统一职责与接口规则

- 本库直接依赖 `qubit-operation = "^0.1"`；同步托管接口也使用它，不把通用协议仅放在 async feature 后。具体领域异步 API 仍可由本库 async feature 控制；导入 crate 或构造纯配置不启动服务线程。
- 应用创建并注入共享 `OperationSupervisor`；本库创建受管理子 scope，不因领域服务各异而再创建私有通用 supervisor。应用关闭共享服务，本库只关闭自己受理范围内的子操作；显式独占的便利构造必须说明其服务归属。
- 同步/异步共用 `OperationContext`、`OperationHandle<T,E>`、`OperationObserver`、`ResourceUse` 和 `SettlementReport`。领域包装仅增加强类型结果/领域方法，不创建另一套 token、deadline、detach、恢复注册表或调度循环。
- `result()/result_blocking()` 一次性取业务结果；`released()/released_blocking()` 观察稳定终结。等待 future Drop 不取消仍持有的 handle，handle Drop 请求停止，detach 返回公共 observer；Unconfirmed 只能观察或超期返回快照，不能视为 Released。
- 请求提交必须先预留公共登记槽和领域容量，attach 领域 control 后 commit 才能产生外部副作用。停止信号和启动竞争使用公共线性化协议，未启动受理失败归还 owned 输入。
- 领域 worker、进程 reaper、原生 Session 和领域字节预算仍由本库管理；通用 supervisor 不执行阻塞 I/O 或推理。子任务从父 scope 切分配额，不重复争抢 root 槽，也不将同一资源登记两次。
- `OperationContext` 只携带共有上下文；领域 options 保留权限、版本、原子性、输出或采样策略。总期限向下收紧，不重置；清理不复用已取消的业务 token。生命周期停止请求不撤销已发生副作用。
- 不增加进度接口、progress feature 或事件总线依赖。业务自行使用 rs-progress / rs-event-bus；本库状态与诊断仅用于正确终结，不自动上报业务阶段。

### 0.2 共用验收与发布门禁

先通过 rs-operation 核心测试，再用本库假执行器验证：取消先于 commit 不启动、业务结果早于资源释放、队列满仍能停止/清理、Unconfirmed 持有资源、恢复 owner 不丢、借用对象不逃逸、外部 driver 丢失不伪造释放、公共 handle 的同步/异步对等。以下领域专属验收在此基础上叠加，不能用领域测试替代上游前置验收。

### 0.3 恢复时与当前 rs-operation 的核对

本文从此前保留的文档定义和生命周期整合脚本恢复；目标 minor 版本不变。rs-operation 当前已经有实现，实施门禁指其所需契约已验收，而不是声称它仍为空库。以其当前设计（综合评审修订 2026-11-12，相关提交 60195ea）为准，本文示意 API 不能覆盖上游最终契约。

长期领域服务使用公共服务受理组标识归属，不把所有请求塞进占用活动 root 许可的长期 scope。每个独立请求申请自己的 root，已有请求内的调用使用子 scope；长期领域资源通过公共服务资源 scope 和授权租约提供。共享 supervisor 仍由应用关闭，服务只关闭本组工作。Drain 停止新请求但允许已有请求完成必要子步骤；Cancel 阻止新的业务 commit，清理仍可推进。

只有整体成功需要等待 required 子操作业务结果均已确定；失败、停止和 panic 可以先发布，资源仍由公共 released 独立确认。清理前置依赖失败时，不能继续销毁后继仍需使用的资源，应保留恢复组。不可比较的时钟域、清理依赖与业务结果未决状态，都必须遵守上游新增约束，不能凭本地便利接口自行降级。

## 1. 定位与目标

MIME 库负责输入语义、候选识别、选择与细化，不应自行维护通用阻塞线程池、子进程或临时文件清理协议。它仍须负责把上游操作正确组合起来，特别是取消时先终结命令，再清理其输入文件。

0.22.0 覆盖 repository/file 检测器、ffprobe 分类器、第三方 provider 契约和检测 runtime。目标是异步入口无隐藏文件/命令/推理阻塞，输入和资源预算有界，取消可观察，前缀与完整内容语义保持准确。数据库规则更新、新模型、改变 MIME 候选排序、自动下载工具及通用作业框架不在范围内。

## 2. 当前问题与代码依据

- `src/detector/file_command_mime_detector.rs` 通过 blocking::unblock 包住复制、暂存、同步 file 命令和清理。
- `MimeDetectorBackend::guess_from_content_async` 默认直接调用同步方法；只把方法写成 async 不能保证非阻塞。
- `MimeDetectorCore` 的细化可调用同步 MediaStreamClassifier；修复 file 后端不能覆盖整个异步链。
- `src/temp_staging.rs` 已正确使用 LocalTempFile 并保留检测失败与清理失败，新的组合层必须保留这些行为。
- `ContentRequirement::{Prefix, Complete}` 已阻止把任意前缀当完整文件；Magika 有独立的随机访问 provider 路径，不能统一退化成读全量。
- 现有 MimeRuntime 共享配置和 repository，不是完整执行服务；需要扩展其资源注入，但不能在注册 provider 时执行探测、启动模型或 I/O。

## 3. 公共契约与领域对象

MimeRuntime 不再实现自己的 scope 协调线程、取消 token 和恢复注册表；它注入公共 OperationSupervisor 以及命令/文件/推理领域服务。框架提供生命周期，MIME 只提供检测策略和组合 body。

| 对象 | 用途 |
| --- | --- |
| `MimeExecutionServices` | 公共 supervisor 引用、CommandExecutor、本地暂存服务与 MIME 领域配额 |
| `MimeOperationOptions` | 输入/暂存预算收紧，取消和期限从公共 context 继承 |
| `MimeOperation` | `OperationHandle<MimeDetectionReport, MimeError>` 别名 |
| `MimeDetectionReport` | 选中 MIME、输入范围、后端及领域预算事实；不复制 SettlementReport |
| `AsyncMimeDetectorBackend` | 显式实现的 object-safe 异步检测协议，接收公共 scope/context |
| `AsyncMediaStreamClassifierBackend` | 显式异步精细分类协议，沿用同一子 scope |
| `MimeBackendCapabilities` | 输入要求、同步/异步、随机访问/暂存能力 |

保留同步接口，从同步 backend 移除内联调用阻塞方法的默认 async hook。异步 trait 返回 `Pin<Box<dyn Future<Output = MimeResult<T>> + Send + 'a>>`，注册表仍 object-safe；同步-only provider 在异步选择时返回明确 AsyncUnsupported。provider 注册不初始化模型、不探测命令、不创建服务。

repository 显式实现异步协议，纯内存规则匹配分有界批次推进，加载和 XML 解析在受管理准备阶段完成。对同步受管理检测同样传递公共 context；普通纯内存同步便利方法不强制后台调度。

## 4. 入口与请求上下文

以下是目标 API 摘要，省略 builder 与默认选项方法，不是已实现示例。

```rust,ignore
type MimeOperation = OperationHandle<MimeDetectionReport, MimeError>;
impl MimeRuntime {
    fn with_execution_services(self, services: MimeExecutionServices) -> Self;
    fn submit_detection_in(&self, scope: &OperationScope, input: OwnedMimeInput,
        options: MimeOperationOptions) -> Result<MimeOperation, MimeSubmitFailure>;
    async fn prepare_in(&self, scope: &OperationScope, selection: ProviderSelection)
        -> MimeResult<Arc<dyn AsyncMimeDetector>>;
}
trait AsyncMimeDetector: Debug + Send + Sync {
    fn detect_content_async_in<'a>(&'a self, scope: &'a OperationScope,
        input: &'a [u8], options: MimeOperationOptions)
        -> MimeFuture<'a, Option<String>>;
    // provider 路径入口接收 AsyncFileSystem、FsPath 与同一 scope。
}
```

owned submit 先完成公共登记和 MIME 配额准入，再 commit 检测 body。借用 bytes/provider/reader 的便利方法只在其 future 存活时读取借用；交给领域 worker 的字节必须拥有所有权并先申请预算。后台收尾只保留已登记 owned 的命令/temp/observer，绝不捕获已销毁的借用输入。

上下文沿“检测→读取→后端→精细分类”显式传递。为每个业务阶段建立必要子 scope，但低层重复包装同一个操作时透传登记，不再建立 root。公共 handle 分离 result 和 released；Option<String> 便利方法需在必需收尾已确认后映射结果，不能隐藏未释放状态。

MIME 不定义另一套 wait/detach/取消完成协议。超期需提前返回时提供公共 observer 和领域摘要，不把 Unconfirmed 转成成功清理。同步托管 facade 使用公共阻塞观察接口，不在异步 poll 中调用它。

## 5. 输入范围与结果不变量

Prefix 表示调用方只知道这些字节，不能根据长度猜测是否完整；Complete 表示输入确已到 EOF 或调用方明确提供完整内容。为后端结果新增 `InspectedExtent`：Prefix、Complete、RandomAccessSamples，报告采样预算和一致性事实。

保留现有 `ContentRequirement` 两种含义；能力增加 random_access 支持描述，不将它改名为 Complete。Complete 后端可用完整逻辑资源上的有限采样实现检测，不能声称采样等于读取每个字节。内容细化只使用确实保留的前缀/完整内容；随机访问结果没有可用前缀时不得凭空生成一个。

字节入口、seekable reader、provider 路径遵循现有候选排序和检测策略。非 seekable 流需要完整内容但不能在预算内暂存时返回明确 InputLimit/CompleteContentRequired，不静默降级为前缀猜测。需要恢复 reader 位置的同步入口保持原契约；异步借用流取消后不承诺恢复，提供拥有 reader 的 operation 及显式 settle/restore 路径，并报告恢复失败。

## 6. file 后端：先保留语义，再评估优化

0.22.0 的默认行为继续使用路径模式，不以 stdin 等价性未经验证为前提。

```text
获得请求配额 → 检查输入范围/大小 → rs-local-files 暂存并关闭写端
→ rs-command 异步执行 file → 校验完整 stdout/MIME → 命令终结 → cleanup → 返回
```

使用 LocalFileSystem 的私密 Host 暂存区和有限 stage 操作；不把 Rooted 显示路径交给外部命令。命令使用结构化参数、`--`、敏感路径标记、现有超时/输出预算和严格 MIME 格式校验。`MimeCommandExecutor` 拆为 sync 与 async 两个测试接口，异步生产实现只调用上游异步 runner。

scope 在命令提交前登记 temp owner，在命令完成/取消并确认不再访问输入后清理。若进程回收未确认，不删除可能仍被使用的临时资源，交给后台 scope 保留并报告 Pending/Unconfirmed。检测和清理同时失败继续保留主错误与 typed cleanup context；取消也可以附带清理错误。

可选 stdin 路线作为实验/显式选项，默认关闭。启用前必须对 file 平台版本、空内容、短前缀、完整内容、大输入和错误路径建立与路径模式的差异报告；若结果不等价，不自动选择。暂存文件还服务 ffprobe，因此即使 file 可改 stdin，也保留资源组合机制。

## 7. ffprobe 与细化全链路

新增异步分类接口，包括 async provider/本地路径和已拥有内容的入口；不得在异步检测完成后调用同步 `refine_detected_mime_type`。新增共享选择算法上的异步 refinement，业务映射与同步一致。

ffprobe 通过 rs-command 异步执行；输入是不能直接访问的 provider/stream 时，以现有 media staging 上限有限暂存，先检查超限哨兵再执行命令。请求检测与精细分类共享预算，已有内容能复用就复用，不能重复读取/暂存后重新计算全部预算。

未显式配置分类器时不自动启用 ffprobe。取消、期限、输入超限、不完整命令输出和初始化错误不可转成“未知 MIME”成功；格式本身无法识别才返回 None。保留现有 provider fallback 选择规则；用户取消和资源超限不触发另一后端继续花费预算。

## 8. 领域预算、容量与取消

活动检测默认 4、排队 64、受理等待者 64，作为 MIME 领域容量；公共 root scope 数量和领域容量分别约束。调用方已建 root 时检测使用子 scope，不重新争抢公共 root 槽。owned payload 总额默认 16 MiB，暂存磁盘预留总额 256 MiB；单请求 max_buffer_size/media staging 继承当前配置，与 context 收紧值和服务上限取最小。

字节引用共享只计一次真实 owned 分配，复制另计；读/采样/暂存/精细分类累计预算不能因子 scope 重置。命令容量归 CommandExecutor，I/O 容量归 LocalIoExecutor，推理容量归 MagikaRuntime；MIME 不把这些工作塞入自己的池。

取消和总期限由 rs-operation 传播：停止新读取/分类/命令，通知已启动领域操作，再按 ResourceUse 依赖清理。先释放进程和全部 helpers 对输入的使用租约，再删除暂存文件；正在 native 推理则等待其真实释放。公共 result 已就绪不代表 released 已完成。

借用异步输入 future 被丢弃后，公共 scope 只能接管 owned 工作；若 provider 执行绑定外部 driver，必须登记 driver 存活租约，应用先关闭检测 scope 再关闭 driver。driver 提前丢失报告 Unconfirmed，不声称全部收尾可在未知线程继续执行。

## 9. 执行服务与扩展后端

MimeExecutionServices 注入 OperationSupervisor 和领域服务，删去旧草案中 MimeRuntime 自建协调线程的方案。业务组合 future 由公共 PortableDriver 或明确指定 ExternalDriver 执行；MIME 不再维护第三种驱动器。共享服务的关闭权属于应用，MIME 只能停止自身子树受理并通过公共 observer 等待。

file、ffprobe、Magika 的领域 control/cleanup 直接挂在相同 scope 树下，复用 ResourceId 和资源使用租约。不能在 MIME 中另存一份 temp owner，再让文件库重复清理。自定义 backend 必须明确实现异步和领域 control 协议，或只支持同步；不自动用任意 spawn_blocking 闭包包装未知后端。

注册只提供工厂与描述；依赖服务 Closed 时返回错误，不临时新建 executor 绕过容量和关闭。纯粹业务阶段汇报留给调用方使用 rs-progress / rs-event-bus，不给 MimeExecutionServices 添加 progress sink。

## 10. 错误与结果观察

MimeError 保留 CommandError、LocalFileError、FsError、ProviderFailure 的 typed source 和既有 TemporaryCleanup；容量/输入限制/异步不支持保持领域错误，共有停止与期限通过公共 OperationOutcome 表达，不能维护两套终态裁决。

MimeDetectionReport 保留业务选中结果、后端、输入范围及已使用预算。清理失败、恢复 owner、Unconfirmed 由公共 SettlementReport/observer 表达；便利 API 若必须维持 Result<Option<String>>，使用明确的 MimeCompletionError 组合业务结果和收尾诊断，不覆盖原错误或伪造成功。

日志只包含 ID、backend、范围和计量，不默认输出原始内容/完整路径。生命周期诊断不是用户进度，不新增自动业务事件。

## 11. 依赖、兼容性与实施顺序

| 依赖 | 目标 | 必需能力 |
| --- | --- | --- |
| qubit-operation | ^0.1，必须先实现并验收 | scope/control/资源登记/公共同步异步观察 |
| qubit-command | ^0.12，启用 async | 同 scope 命令执行、helpers 释放确认 |
| qubit-local-files | ^0.6，启用 async | 同 scope 暂存/清理、ResourceUse |
| qubit-fs | ^0.4，启用 async | 托管 SPI、同对象随机读取、通用资源能力 |

qubit-magika ^0.19 为下游，不加入本库依赖。rs-fs-local 同步/异步适配也需另行迁移到 fs 0.4/local-files 0.6；未完成前不得宣称新本地 provider 已可用，但远程 provider 和 owned bytes 路线可以独立验收。

实施顺序固定为 rs-operation → rs-command/rs-local-files/rs-fs 的协议与适配 → rs-mime → rs-magika，再做跨仓库消费验证。command/local-files/fs 的协议工作可在上游门禁通过后并行，不能让 MIME 的独立临时方案替代它们。

迁移全异步链后删除 rs-mime 对 blocking 的直接依赖；保留同步候选排序与检测策略。async trait、classifier/context 注入及 fs 依赖变化为本次 minor 破坏性变更，不改 Cargo 版本直到实现完成。

关联的历史设计基线：rs-command 0.12.0、rs-local-files 0.6.0、rs-fs 0.4.0 和 qubit-magika 0.19.0。这些版本仅用于追溯草案背景，不代表当前实施依赖或契约。

## 12. 实施与验收

先完成 rs-operation 前置验收及上游适配，再按异步 trait/context → file → ffprobe/refinement → runtime/取消 scope → provider/downstream 迁移推进。期间同步路径作为业务结果基线，不改变候选排名或猜测策略。

验收矩阵：repository/file/Magika × bytes/prefix/本地/provider/stream × 同步/异步 × 精细分类开关。检查空输入、错误文件名、Complete 后端拒绝 prefix、随机访问样本不冒充完整内容、文件替换、尺寸预算和累计读取预算、超大上传、精细分类不重复计费、取消发生在排队/暂存/命令/refinement/cleanup 的每个阶段、命令未确认终结时暂存仍存活、双错误保留、无分类器时不执行 ffprobe、取消不触发 fallback。

用受闸门控制的假命令/文件服务验证异步执行器心跳持续推进；测试不能靠任务快到“看起来不阻塞”。加入每次 poll 的有限工作量测试、总容量峰值、服务关闭、未观察错误诊断和 rs-magika 下游文档消费测试。通过这些验收后才能宣称异步链路已完整迁移。
