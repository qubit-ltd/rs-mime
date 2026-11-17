# rs-mime 0.22.0 operation 验收记录

- 验收日期：2026-11-17
- 范围：仅 `rs-mime`；按用户约束，将 `rs-operation` 视为已完成 0.3 重构的公共契约。
- 版本与依赖：`qubit-mime` 0.22.0；正式清单以 `path = "../rs-operation"` 引用 `qubit-operation ^0.3`，`qubit-io ^0.19.6` 使用 crates.io。隔离 worktree 的 CLI 登记禁止复用先前已清理的 `../rs-operation` 登记路径；隔离验证临时将两个 Cargo manifest 指向本任务登记的同一 rs-operation 提交 `985a8727d93155962831d62aca4594e330c94928`，之后已恢复正式相对路径。未改全局 Cargo 配置。整合到源工作区后仍须按正式路径重新解析并验证。
- 文档边界：设计文档与实施计划是输入文件，未纳入变更和提交；本记录仅记录实施与验收证据。

## 实施范围

完成计划 M1—M8 所述 rs-mime 变更：定义 operation 领域 API、runtime 与准入/输入归还/预算；实现 repository async backend；提供受管 staging、worker 和 command 生命周期；迁移 file 与 ffprobe/refinement；扩展 provider 注册与消费接口；更新迁移指南、README、版本和 consumer fixtures。保留同步接口，不增加下游仓库验收。

最终审查修复了两处资源报告错误：托管文件 staging 现在把实际写入字节传递到 `MimeCandidates` 和 `MimeDetectionReport`；同步提交报告现在记录真实 owned 输入长度。后者补有 `test_operation_runtime_sync_submission_uses_parent` 断言，前者由 `test_file_operation_runs_managed_command_through_fixed_worker` 断言 staging 字节等于输入字节数。

## 验证证据

命令均在 rs-mime 实施 worktree 执行，`CARGO_HOME=/tmp/rs-mime-99d2bb4b-cargo-home`，不使用全局 Cargo 配置。

| 命令 | 结果 |
| --- | --- |
| `CARGO_HOME=... cargo test --lib detector::file_command_mime_detector::file_operation_tests::test_file_operation_runs_managed_command_through_fixed_worker` | 通过，1/1；实际 `file` 命令 staging 量与输入长度相等。 |
| `CARGO_HOME=... bash .infra/bin/align-ci.sh` | 最终修复轮通过；Rust style、Clippy/check 对齐成功。 |
| `CARGO_HOME=... bash .infra/bin/ci-check.sh` | 修复 staging 统计轮通过：build、182 unit tests、351 integration tests、42 doctests + 1 compile-fail、feature matrix、package verify、coverage 和 audit 均通过。 |
| 同轮 coverage 门禁 | functions 95.07% (1137/1196)，lines 95.55%，regions 95.37%，满足仓库阈值。 |
| 同轮 security audit | 扫描 Cargo.lock 中 89 个 crate 依赖，未报告漏洞。 |
| 同步报告计量新增修复 | 随后再次运行 `align-ci.sh` 通过；最终 CI 通过；同步报告断言已包含在本轮 182 unit tests 与 351 integration tests 中。 |

仓库 CI 没有 downstream all-features 测试；未运行上下游仓库测试。文档 consumer 中的 `qubit-mime ^0.22` 在 crates.io 发布前不能直接从 registry 解析；整合验证将以命令级 Cargo patch 将该 consumer 指向本地项目，正式清单和全局配置不作改写。

## 已验证的主要行为

- Runtime 支持 root/child 与同步/异步受管提交、停止与观察；准入失败保留输入，shared budget 约束内存、读取及 staging。
- Provider 输入基于已打开对象读取；测试覆盖路径替换快照、driver 丢失保留 pending 与随机访问 extent。
- Repository 使用有界异步匹配；provider 注册不触发执行器启动，sync-only provider 不静默回退到同步 async hook。
- Staging/command 生命周期使用 ResourceRef、受管 cleanup 和 recovery；覆盖取消、短读/哨兵、额度边界、parent/child 资源关系、cleanup 重试和未消费 outcome。
- File backend 明确要求完整内容；ffprobe refinement 使用受管输入并复用 staging 额度，不在 async 路径调用同步 classifier。

## 有边界的环境说明

隔离工作区的正式 `../rs-operation` 位置曾登记并清理；CLI 明确禁止在同一任务中复用已清理登记位置，因此 worktree 中无法直接以正式路径运行 Cargo。验证使用了任务登记的 rs-operation worktree，commit 为 `985a8727d93155962831d62aca4594e330c94928`，并在最终集成源工作区恢复正式相对依赖后再次解析、构建及运行必要 CI。此路径限制不改变交付清单中的正式依赖关系。
