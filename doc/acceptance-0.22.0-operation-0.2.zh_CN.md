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

## 2026-11-17 MIME-001—004 实施与验证

- 实施提交：`691056ff865fc6b9ed0364e834526e2060603fb5`（`fix(mime): align managed detection reporting and packaging`）。独立 review 未发现 Critical 或 Important 问题。
- MIME-001：最终许可释放时，按许可对象身份清理分配索引；测试覆盖 alias 中间释放、最终释放与重新预约、弱引用计数、普通 owned/staging 许可、旧许可不移除替代项及 barrier 并发。
- MIME-002：双语指南列出 repository、file 和 sync-only 自定义 provider 的受管异步能力及限制。
- MIME-003：包清单精确包含验收记录；新测试从 `cargo package --list --allow-dirty` 读取实际包文件并验证四份 Markdown 入口的相对链接闭包。本轮设计和计划文件未进入包清单。
- MIME-004：确认同步 repository 报告原先把受 `max_test_bytes` 限制的匹配报告为 `Complete`。现在报告实际匹配窗口为 `Prefix(n)`；唯一文件名 `PreferFilename` 在同步及受管异步 repository 入口均短路为 `Prefix(0)`，Provider 不会因此读取前缀。`ContentFirst` 与 `VerifyContent` 仍执行有界内容 matcher。`InputExtent` 表示输入完整性，`InspectedExtent` 表示匹配窗口。

本轮 focused 与最终集成 worktree 验证命令如下：

| 命令 | 结果 |
| --- | --- |
| `cargo test --lib mime_budget` | 通过，24 tests。旧逻辑的索引残留回归曾按预期失败；修复后通过。 |
| `cargo test --test package_document_links_tests` | 通过，3 tests（2 个解析单元测试和 1 个包清单集成测试）。 |
| `cargo test --test mod operation_runtime` | 通过，17 tests；旧同步范围行为曾复现 `Complete` 与期望有界 `Prefix(18729)` 的差异。 |
| `./.infra/bin/align-ci.sh` | 通过；Rust style fix、Clippy/check 对齐成功。 |
| `./.infra/bin/style-check.sh` | 通过，机械风格检查无发现。 |
| `cargo test` | 通过：352 integration tests、3 package-link tests、42 doctests 和 1 compile-fail doctest 均成功。 |
| `cargo test --all-features` | 通过，测试与 doctest 均成功。 |
| `cargo doc --no-deps --all-features` | 通过。 |
| `./.infra/bin/ci-check.sh` | 通过：lock/build、feature matrix、测试、文档、package verification、coverage、security audit 均成功。 |

最终 coverage：functions 95.10%（1144/1203），lines 95.58%（9585/10028），regions 95.40%（16387/17177），满足仓库门槛。Security audit 扫描 89 个 Cargo.lock 依赖，未报告漏洞。package verification 成功打包并验证 229 个文件。本轮隔离验证使用登记的相对依赖 worktree `../rs-operation`，精确提交 `da023d6105aae13f1689d789f0e4a6219ad667a1`；正式 manifest 未改写。

### Cargo 依赖解析修正

用户指出 Cargo 不应解析上级目录中的 `rust-common/Cargo.toml`，并明确要求从 crates.io 依赖 `qubit-operation = "^0.3"`。已将 `Cargo.toml` 的依赖改为该 registry 版本范围，并增加空 `[workspace]`，使 `rs-mime` 成为独立 workspace 根，Cargo 不再向父目录探测 workspace/package。`Cargo.lock` 锁定 crates.io `qubit-operation 0.3.0`。

最终配置解析验证：`cargo locate-project --workspace --manifest-path Cargo.toml --message-format plain` 返回本 crate 的 `Cargo.toml`；`cargo metadata --manifest-path Cargo.toml --format-version 1` 和 `cargo tree --manifest-path Cargo.toml --invert qubit-operation` 均确认依赖来源为 crates.io `qubit-operation 0.3.0`。此前依赖本地路径 worktree 的验证记录描述的是修正前状态，不代表最终交付配置。

## 2026-11-17 MIME-005—007 生命周期复核

本节仅记录本轮实施证据，不改写上方 MIME-001—004 的历史验证结果。

- MIME-005：增加 `ReadTreeContext` 最终释放时的精确弱索引注销。回归覆盖连续根请求、活跃上下文、同 ID 替代绑定、多 ID、registry 先销毁及 poison 锁恢复。旧逻辑新增索引长度断言曾失败；修复后 focused 测试通过。
- MIME-006：顺序外部父子请求回归在修复前确认第二个子请求绕过了累计读取上限；父操作保持存活，但第一个子请求完成 settlement 并释放 handle 后才提交第二个。修复后父资源域保留一个读取预算锚点，顺序及并发子请求共享首次确定的累计上限。不同根／父仍有独立额度；父资源条目容量不足、停止或结束时以 `ReadBudgetBinding` 携带错误种类拒绝并归还原输入。
- MIME-007：barrier 回归确认旧实现会在 execution port 的 `prepare()` 返回前报告 ready，并接受准备中的 root/child 提交。修复后仅在执行端成功后发布 ready；失败仍可重试，prepare 与 shutdown 由同一生命周期锁串行。
- 兼容性：未更改 `qubit-operation = "^0.3"` registry 依赖或独立 workspace。`MimeSubmitRejection` 新增 `ReadBudgetBinding(ResourceManageErrorKind)`，穷尽匹配的下游调用方需要处理这一新拒绝种类；失败 input 由 `MimeSubmitFailure` 原样持有。无需升级 `rs-magika`。

### 本轮验证命令

| 命令 | 结果 |
| --- | --- |
| `cargo test --lib read_tree` | 通过；包含根索引注销、外部父预算复用、失效弱索引替换及并发注册覆盖。 |
| `cargo test --lib mime_execution_services` | 通过，18 tests；覆盖 prepare 状态发布、失败重试及 prepare/shutdown 竞争。 |
| `cargo test --test mod sequential_mime_children_keep_external_parent_read_budget -- --nocapture` | 修复前因第二个子请求未被预算拒绝而失败；修复后通过，1 test。 |
| `cargo test --test mod external_parent_read_budget -- --nocapture` | 通过，2 tests，验证 binding 错误种类及原 input 归还。 |
| `cargo test --test mod concurrent_external_parent_children_share_one_budget_binding -- --nocapture` | 通过，1 test，验证并发 child 共用一个父预算锚点。 |
| `./.infra/bin/align-ci.sh` | 通过；包含 Rust 风格修复和对齐检查。 |
| `./.infra/bin/style-check.sh` | 通过。 |
| `cargo test` / `cargo test --all-features` | 通过；完整 CI 分别运行默认与 feature-matrix 测试，200 个库测试、355 个 mod 集成测试均通过。 |
| `cargo doc --no-deps --all-features` | 通过；完整 CI 文档检查通过。 |
| `./.infra/bin/coverage.sh --output /tmp/rs-mime-coverage-final5.json` | 通过；functions 95.07% (1195/1257)，lines 95.60% (10040/10502)，regions 95.48% (17293/18111)。 |
| `./.infra/bin/ci-check.sh` | 最终门禁通过；包含 style、build、test、release build、feature matrix、package、coverage 与 audit。 |

## 2026-11-17 MIME-008/009 并发关闭与 Prefix 分类

- MIME-008：`FixedMimeExecutionPort` 使用独立 shutdown 结果锁串行关闭调用，并在 worker join 完成后缓存成功／失败终态。锁在 join 时不占用 running 或 queue 锁；并发关闭调用复用实际 worker 退出结果，dispatch 在关闭期间仍及时拒绝。shutdown Rustdoc 明确只能从 worker pool 外部同步上下文调用。
- MIME-009：旧受管路径的真实 Ogg fixture 命中 `ogg` 精细映射后，会把 `Prefix` 送入 async classifier 并得到 `CompleteContentRequired`。修复后，受管 `repository` 的 `Prefix` 返回基础 MIME，不调用 classifier；同一 fixture 的 `Complete` 对照仍调用 classifier 并按 `AudioOnly` 结果返回映射类型。`mime_submission.rs` 中非随机访问 Provider 经 `read_prefix` 读取后会构造成 `BorrowedMimeInput::Bytes { extent: Prefix }`，同一 core 分支覆盖该路径。
- 双语用户指南同步说明：受管前缀不会做完整媒体精化；需要精化时提供完整输入。并发关闭会等待 worker 实际退出，且调用位置须在 worker pool 外部。

本轮集成提交：`d47ef95`（指南）、`d598446`（并发 shutdown）、`d734d87`（Prefix 精化）。并发关闭回归曾在旧实现下因第二个 shutdown 提前返回而失败；受管 Prefix 对照曾在旧实现下以 `CompleteContentRequired` 失败。整合后 focused 验证如下：

| 命令 | 结果 |
| --- | --- |
| `cargo test --lib fixed_mime_execution_port` | 通过，10 tests。 |
| `cargo test --test mod operation_runtime` | 通过，21 tests，包含映射命中的 Prefix／Complete classifier 调用次数对照。 |
| `cargo test --test mod prefix_paths_skip_classifier` | 通过，1 test。 |
| `git diff --check` | 通过。 |

并发测试随后依据独立 review 调整为由第二个调用先尝试获取 shutdown 结果锁；只有实际观察到锁被占用时才发出同步信号，之后检查 worker 释放前调用未提前返回。无论检查结果如何都会释放 job 并 join 两个线程。调整后再次运行 `cargo test --lib fixed_mime_execution_port`，10 tests 通过。完整格式对齐、style、默认／all-features 测试、文档、coverage、audit 与 CI 结果将在下方补录。

## 2026-11-17 MIME-010 精细分类暂存峰值报告

- T1 请求计量原语提交：`f0d53e4`（`fix(mime): track request staging usage`）。每个 `ReadTreeContext` 单独持有请求暂存 tracker；RAII 许可释放当前计量但保留峰值。global staging 额度、请求计量溢出回滚、无 tracker 路径和 poison 锁恢复均有测试。
- T2 报告修复提交：`443c943`（`fix(mime): report request staging peak`）。受管 FFprobe classifier 按 operation 选择 tracker，最终报告取 backend 原始值与请求峰值的较大值，避免重复相加。Prefix 不执行完整分类且报告零值；自定义 backend 的外部暂存仍由其 `MimeCandidates` 提供证据。
- 新增真实 Ogg runtime 回归：通过 repository 精细映射和真实 FFprobe classifier（注入可控命令执行器），分类结果为 `audio/x-vorbis+ogg`，`staging_bytes` 与输入长度相等。
- 独立 review 指出并发报告隔离仍缺端到端断言，随后以 `4dc0ddf` 补测：两个不同大小的 Ogg 请求同时持有 staging 许可，由双到达异步 gate 确认重叠，不用 sleep；释放后分别断言各自 MIME 和各自输入长度。

| 命令 | 结果 |
| --- | --- |
| `cargo test --lib request_staging` | 通过，4 tests。 |
| `cargo test --lib staging_budget` | 通过，8 tests。 |
| `cargo test --lib test_runtime_report_includes_real_ffprobe_staging_for_ogg_refinement` | 通过，1 test；精细 MIME 与报告暂存量均有断言。 |
| `cargo test --test mod operation_runtime` | 通过，21 tests；包含 Prefix 零暂存报告。 |
| `cargo test --lib test_concurrent_runtime_reports_keep_request_staging_isolated` | 通过，1 test；两种输入并发重叠并分别报告自身峰值。 |
| `cargo test --lib async_operation_tests` | 通过，8 tests。 |

最终格式对齐、style、完整测试、文档、coverage、audit、CI 和原工作区整合验证结果待本节下方补录。

## 2026-11-18 MIME-011/012 异步注册封闭与回调锁边界

- MIME-011：两个 Registry 的 `seal()` 与异步注册共享异步表写锁；封闭状态仍由 `ProviderRegistry` 唯一维护。封闭后通过 clone 注册会得到明确错误，已有 canonical ID 与 alias 仍可创建。
- MIME-012：异步创建只在读锁内按选择顺序克隆 provider 和缓存的 canonical ID，随后在锁外调用 factory。冲突诊断也使用缓存身份，不在异步表锁内调用既有 provider 的 descriptor。
- 新增 detector 与 classifier 回调重入测试：factory 委托另一个线程注册不同 provider，并用 channel 有界观测注册是否在回调返回前完成；此测试不靠 sleep 等待。
- 双语指南说明应先完成注册再封闭、克隆共享封闭状态和创建使用 provider 快照。

### 本轮验证

#### MIME-018/019/020（2026-11-18）

- MIME-018：只有精确的内建 `RepositoryMimeDetectorProvider` 类型才绑定内建 MIME repository。canonical ID、alias 和 selector chain 均保留 Provider filename 的短路；同 ID 的自定义 provider 仍实际执行，不会误用内建 repository。repository 初始化延迟到 factory 返回后，避免在注册表锁内做 I/O。
- MIME-019：文档 consumer workflow 将主 crate 与所有本地路径依赖检出到保持相对布局的 `sources/` 目录，并固定每个依赖提交。尚未发布的 `qubit-mime ^0.22` fixture 改为指向本地 crate；fixture lockfile 已同步，根 lockfile 未改。
- MIME-020：glob 简单扩展名索引仅把 matcher 实际使用的 `*`、`?`、`[` 视为模式语法。`!`、`^`、`{`、`}`、`]` 等普通字符扩展名现在能正确参与精确索引。

主仓库任务提交：`279b60a`、`252afff`、`edb1c73`。只读依赖工作副本固定为 rs-fs `bbb7a5d269f2f3a955fcd25839177eae4795898f`、rs-fs-local `1067ff1ffe75376f2349c2a484ccf2423e29d20e`、rs-fs-registry `f979cd4425d9a50ea0a3ca5544ab94a8bffad31b`、rs-fs-testkit `72af3d22362de9884c77e0bcaadbccfca4725a35`、rs-operation `7fcda0375286f9466dbdfbb63d720d10c88fb423`。

| 命令 | 结果 |
| --- | --- |
| `.infra/bin/align-ci.sh` | 通过；Rust 格式、文档与仓库对齐检查通过。 |
| `.infra/bin/style-check.sh` | 通过。 |
| `.infra/bin/ci-check.sh` | 通过；库测试 193、集成测试 387、文档测试 64 及 compile-fail 测试通过，功能矩阵、文档和审计通过。 |
| `.infra/bin/ci-check.sh` coverage 汇总 | functions 95.03%（1186/1248），lines 95.55%（9913/10375），regions 95.49%（16947/17747），均达到门槛。 |
| `cargo check --manifest-path tests/fixtures/documentation_consumer/Cargo.toml --locked` | 通过。 |
| `cargo check --manifest-path tests/fixtures/operation_consumer/Cargo.toml --locked` | 通过。 |
| `cargo tree --locked --manifest-path tests/fixtures/operation_consumer/Cargo.toml` | 通过；确认解析到本地 rs-mime、rs-fs、rs-operation 路径。 |
| `git diff --check` | 通过。 |

rs-magika 的动态加载 CI 同步固定上述依赖及 rs-mime `edb1c737dc4efac1bbff1e673fbc70a73d31882f`。原 workflow 使用的 ONNX Runtime GitHub tarball URL 返回 404；因此改为下载带 SHA-256 校验的官方 1.24.2 CPython 3.11 manylinux wheel，并在本机用该产物实际运行 dynamic-loader fixture。rs-magika `.infra/bin/align-ci.sh`、`.infra/bin/ci-check.sh`、动态加载 feature 自检、README consumer、动态加载 fixture 均通过；覆盖率 functions 90.02%、lines 93.68%、regions 94.45%，依赖审计通过。
