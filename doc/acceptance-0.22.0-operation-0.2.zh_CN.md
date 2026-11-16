# rs-mime 0.22.0 operation 目标验收记录

- 核对日期：2026-11-16
- 状态：部分实现，尚未完成 0.22.0 验收；本文记录当前 checkout 的可见证据，不代表发布或完整接受设计。
- 范围：仅记录 rs-mime。按设计前提，operation 0.3 是已完成的消费契约；本记录不检查或验收其他仓库。
- 版本边界：本 checkout 的 `Cargo.toml` 仍为 `qubit-mime` 0.21.1，依赖 `qubit-operation = "^0.3"`。本次文档说明针对源码中已出现的目标 API，不应理解为 0.22.0 已发布。README 和手册中的安装版本继续以当前包元数据和文档版本测试为准。

## 当前源码与公开入口

`src/lib.rs` 重导出 `MimeRuntime`、`MimeExecutionServices`、`MimeExecutionPort`、`FixedMimeExecutionPort`、`MimeOperation`、`AsyncMimeOperation`、`OwnedMimeInput`、`InputExtent`、`MimeOperationOptions`、`MimeDetectionReport`、async detector/classifier traits 等。`MimeRuntime` 提供受管提交、父 scope 子提交和同步子提交入口。`MimeExecutionServices` 要求有限非零累计读取上限，显式 `prepare`，并提供 shutdown observer。

文档仅描述已从这些源码定义核对的能力。尤其是：托管 API 不代表每个既有 Provider 均实现 async backend。当前 `FileCommandMimeDetector` 声明 `Complete` 输入要求及 `RandomAccessCapability::Unsupported`，并对 Prefix 返回 `CompleteContentRequired`。README 和手册没有承诺 file 或 ffprobe 在所有托管异步路径上可用。

## 已有测试证据

以下用例在本 checkout 中已经存在；这里只确认源文件和断言存在，不将未运行的测试写成通过：

| 范围 | 当前用例 | 覆盖内容 |
| --- | --- | --- |
| 提交与退还 | `tests/detector/operation_runtime_tests.rs`: `operation_runtime_reject_returns_owned_input`, `operation_runtime_owned_budget_limit_returns_input`, `operation_runtime_wrong_manager_returns_original_input`, `operation_runtime_ended_parent_returns_original_input`, `operation_runtime_closed_group_returns_original_input` | 若干预检、manager/group 拒绝返回原始 owned 输入；拒绝情形未覆盖设计列出的全部错误交错。 |
| 容量与共享 | 同文件：`operation_runtime_queue_and_budget_boundaries`, `operation_runtime_child_does_not_reserve_root`, `operation_runtime_parallel_children_share_actual_read_budget` | 有限队列边界、子操作与读取额度共享的指定场景。 |
| shutdown | 同文件：`operation_runtime_group_shutdown_keeps_other_group`, `operation_runtime_shutdown_waits_for_accepted_work` | 测试了旁组可用及已接纳工作等待；不等同完整 shutdown/recovery 门禁。 |
| repository async | `tests/detector/repository_async_operation_tests.rs`: `repository_async_candidates_equal_sync`, `repository_async_yields_and_stops` | repository 异步候选与同步结果对照、分批让出及停止。 |
| Provider 能力 | `tests/detector/operation_provider_tests.rs`: `operation_provider_registration_is_pure`, `operation_provider_sync_only_is_rejected`, `operation_provider_selected_backend_runs_async_without_sync_factory` 等 | 注册纯净、sync-only 拒绝及选定 async backend 路径；其余用例包括运行时 repository 注入和 classifier 路由。 |
| file 输入能力 | `tests/detector/file_operation_tests.rs`: `file_operation_complete_only_rejects_prefix`, `file_operation_declares_complete_content_and_no_random_access` | 验证 Prefix 拒绝，以及 Complete-only/无随机访问的能力声明；不是 file 命令端到端执行验收。 |
| refinement | `tests/classifier/refinement_operation_tests.rs`: async 路径不调用 sync classifier、PreferFilename 快速路径跳过 classifier、未配置 classifier 不启动工作 | 指定分支的 classifier 行为。 |
| 暂存生命周期 | `src/tests/managed_staging_tests.rs`: 包括 `managed_staging_cancel_before_create`, `managed_staging_owner_waits_for_scope_end_and_all_references`, `managed_staging_outcome_precedes_helper_input_release`, `managed_staging_cleanup_failure_retains_owner_for_retry`, `managed_staging_commit_failure_retains_partial_owner_and_permit`, `managed_staging_unconsumed_outcome_does_not_hold_temp` | 有限输入/暂存边界、scope 与引用约束、部分 owner、清理重试和不读取 outcome 的指定单测。不能据此声称所有真实 command/provider 组合通过。 |

## 尚未验证或仍未实现的设计项

这些是设计/计划中的明确门禁；本 checkout 的上述测试列表没有提供足够证据，不可宣称已完成：

- Provider 对同一个已打开对象的长度、前缀、随机样本和 refinement 一致性；路径替换与并发原地写入的能力检查。
- 未知长度在恰好达到上限时必须再取得 EOF 证据的完整组合，以及所有 L-1/L/L+1、`u64::MAX` 和写入前预算边界的端到端证明。
- MIME command helper 晚释放时真实 file/ffprobe 命令、stdout/stderr、stdin helper 和 temp 的完整集成；真实系统 `file`/`ffprobe` 版本矩阵及错误策略。
- queue 满时 stop/cleanup 仍能前进；父操作持资源时等待依赖该资源释放的 child settlement 不死锁；cleanup 恢复失败时 shutdown 仍正确等待恢复责任。
- 所有拒绝分支预算预留均回滚、owned 别名完整分配只计一次、输出收集峰值和服务缓存寿命的完整计量证据。
- bytes/provider/stream × Prefix/Complete/RandomAccessSamples × sync/async × refinement 的完整消费矩阵，以及独立公开 API consumer 对最终 0.22.0 文档示例的编译验证。
- 计划要求的最终全套闸门：`cargo test --all-features`、`cargo test --no-default-features`、clippy、rustdoc、文档 consumer 及 MIME consumer。没有本轮运行结果前不得标记通过。

上列未验证项不影响文档对当前可见公共接口及已有单测的有限描述，但意味着 0.22.0 operation 目标还不能被完整验收。

## 本次文档检查结果

本节仅填写本次实际运行的文档检查命令与结果。它们不能替代上文列出的完整功能门禁。

- `cargo test --doc`：未运行，Cargo 无法解析 `qubit-operation`（crates.io index 报 `no matching package named qubit-operation found`）。未修改 `Cargo.toml` 或 `Cargo.lock`。
