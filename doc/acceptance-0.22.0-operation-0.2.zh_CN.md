# rs-mime 0.22.0 operation 验收记录

- 验收日期：2026-11-17
- 范围：仅 `rs-mime`；按用户约束，将 `rs-operation` 视为已完成 0.3 重构的公共契约。
- 版本与依赖：`qubit-mime` 0.22.0；正式清单以 `path = "../rs-operation"` 引用 `qubit-operation ^0.3`，`qubit-io ^0.19.6` 使用 crates.io。隔离 worktree 的 CLI 登记禁止复用先前已清理的 `../rs-operation` 登记路径；隔离验证临时将两个 Cargo manifest 指向本任务登记的同一 rs-operation 提交 `985a8727d93155962831d62aca4594e330c94928`，之后已恢复正式相对路径。未改全局 Cargo 配置。整合到源工作区后仍须按正式路径重新解析并验证。
- 文档边界：设计文档与实施计划是输入文件，未纳入变更和提交；本记录仅记录实施与验收证据。

## 2026-11-24 固定依赖图与验收证据索引

下表记录本轮任务布局使用的固定仓库基线。`rs-mime` 是 T6 主仓库；其余 T6 节点来自同组只读依赖树。`rs-magika` 是 T4 同组兄弟仓库，不属于 T6 依赖。后续 T6 最终整合结果尚未写入本记录。

```mermaid
flowchart LR
  mime[rs-mime] --> fs[rs-fs]
  mime --> local[rs-local-files]
  mime --> operation[rs-operation]
  mime -. dev .-> fs_local[rs-fs-local]
  fs_local --> fs
  fs_local --> local
  fs_local -. optional .-> registry[rs-fs-registry]
  fs_local -. dev .-> testkit[rs-fs-testkit]
  magika[rs-magika · T4 sibling]
```

| 仓库 | 固定提交 | 本轮角色 |
| --- | --- | --- |
| `rs-mime` | `4b0ae370ffc10f05f2c38373502a049d991ea568` | T6 主仓库基线 |
| `rs-fs` | `1774ede9bbadd2956e8fa67e3761d59415b4290d` | T6 固定依赖 |
| `rs-fs-local` | `c89dfa900b5f9eb751037c9100e28a622ee52c6a` | T6 固定依赖 |
| `rs-fs-registry` | `df6c8b7ce45495995c0b0ab9d64e14d0b41a8b1c` | T6 固定依赖 |
| `rs-fs-testkit` | `3190325a362f9acf2222795d735381bd5b8a2c17` | T6 固定依赖 |
| `rs-local-files` | `5b52230e0af2997fadc45b5ff48be65b0ca5a70d` | T6 固定依赖 |
| `rs-operation` | `636ad4ebfe9d27b63e886d5935286faf27430301` | T6 固定依赖 |
| `rs-magika` | `729421151769760a8ac1931905a017b0534c3b2a` | T4 同组兄弟仓库 |

已有证据按来源区分：本记录较早章节保留本地 focused、CI、package、coverage 与 audit 结果；下方 T8 条目汇总该轮本地门禁和 GitHub hosted workflow 状态。二者各自对应记录中的提交与依赖图，不代表本轮 T6 最终验收。

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

## 2026-11-24 T8 状态：本地门禁通过，Rust CI hosted bootstrap 待复验

本节记录登记提交 `398ecdd15210013fcd1942a0fb834d87a119b2c9` 的 T8 本地验证，以及随后 GitHub 工作流的状态。T8 本地 `.infra/bin/ci-check.sh` 完成；Cargo 根 crate 和两个真实 consumer fixture 均通过。GitHub 文档 consumer 工作流先通过，之后 Rust CI 在 `./update-infra.sh --check` 启动门禁因本地依赖 TSV 仍列在 upstream profile 快照中而失败。profile/快照归属已在本地修正，两个 workflow 启动步骤通过；GitHub Rust CI 尚未复验。

### T8 本地整合门禁

| 验证范围 | 结果 |
| --- | --- |
| 原工作区 `398ecdd`：`.infra/bin/ci-check.sh` | 通过；测试、feature matrix、package verification、coverage 与 audit 均成功。coverage：functions 97.33%（983/1010）、lines 95.66%（12176/12729）、regions 95.68%（20916/21860）。 |
| 登记 `main` 树：`.infra/bin/ci-check.sh` | 通过；235 个单元测试、406 个集成测试、1 个 operation API consumer 测试、3 个 package document link 测试通过；coverage：functions 97.33%、lines 95.62%、regions 95.65%；coverage 门槛和 audit 通过。 |
| Cargo consumers | 根 crate、documentation consumer、operation consumer 的 locked 本地候选检查通过；GitHub 文档工作流运行 documentation consumer fixture 并编译 operation consumer 均通过。 |
| GitHub 工作流 | 文档 consumer 工作流先通过。随后 Rust CI 于 2026-11-24 00:48 UTC 在 `./update-infra.sh --check` 失败，报告 `.infra/ci/local-path-dependencies.tsv` 与 upstream snapshot 不符。profile/快照修正后，本地该检查退出码 0，文档 consumer 的依赖准备启动步骤退出码 0；GitHub Rust CI 仍待复验。 |

MIME-021–038 的逐项结论未在本记录中展开，不能以未附地址的外部报告作为验收依据。纯 registry 发布解析仍受 `qubit-operation 0.3.2` 尚未发布限制；系统未安装真实 `ffprobe`，受控 fake 命令测试不替代真实系统命令验证。此状态不对 rs-magika 或 hosted 后续运行作结论。

### operation 依赖候选与结果边界

根 crate 和两个消费者当前都使用本地 `qubit-operation 0.3.2` 候选，源提交为 `636ad4ebfe9d27b63e886d5935286faf27430301`。候选提供组级 recovery handle 快照，使关闭门禁读取完整组账本，不依赖可能受诊断长度限制的单个 settlement 报告。

检测业务结果、单 operation 资源结算、MIME 服务结算和 operation group Drain 是不同观察结果：`wait_outcome()` 返回 MIME 结果，`wait_settlement()` 报告该 operation 的资源／恢复责任，`wait_services_settled()` 等待已跟踪操作完成结算，`wait_group_shutdown()` 返回最终组关闭报告。组级 recovery 快照用于处理完整恢复责任，不能用业务成功或单项诊断报告替代。

### 本地依赖图验证

三个依赖图均以 `--locked --offline` 解析到登记路径中的 `qubit-operation 0.3.2`，没有解析成另一份同版本包：

| 范围 | 命令 | 结果 |
| --- | --- | --- |
| 根 crate | `cargo check --locked --offline` | 通过，退出码 0。 |
| 文档消费者 | `cargo check --locked --offline --manifest-path tests/fixtures/documentation_consumer/Cargo.toml` | 通过，退出码 0；无未使用的编译示例警告。 |
| operation 消费者 | `cargo check --locked --offline --manifest-path tests/fixtures/operation_consumer/Cargo.toml` | 通过，退出码 0。 |

对三个清单运行 `cargo tree --locked --offline -i qubit-operation` 均显示同一个本地 `0.3.2` 路径候选。两个 fixture 的运行／消费验证属于本地候选验证。纯 registry 发布解析仍待 `qubit-operation 0.3.2` 发布后执行；本地 path 覆盖不能证明发布包解析。

### 命令环境与受控回归

当前系统 `command -v ffprobe` 退出码为 1，没有可用的系统 `ffprobe`，因此没有将真实系统命令执行记作通过。受控回归使用 `PathEnvGuard` 注入临时 fake `ffprobe`：`cargo test --locked --offline --test mod test_public_ffprobe_async_provider_classifies_complete_bytes` 通过，1 test；它检查完整输入分类及相关拒绝路径。固定 worker 容量回归也由受控 executor 覆盖；T3 提交 `a5b6c45` 的 `test_full_command_queue_returns_typed_busy_after_preserving_lifecycle` 通过，队列满时返回 typed Busy 并保留生命周期责任。它们不替代系统 `ffprobe` 可用时的真实命令验证。

### MIME-038：异步 magic matcher 让出执行器

修复前，单 worker 在 65,540 字节输入、宽 `0..=65,535` offset 的高成本负向规则上可连续占用约 150–230 ms/poll；显式驱动的 20 ms stop 信号约在 147.17 ms 才可观察到。提交 `aa1b8c0` 将异步 matcher 的 offset 与宽值比较工作按 4,096 个单元分片并在分片间检查停止状态，同时以显式栈保留原嵌套顺序。

以下是同一 Linux x86_64、Rust 1.94.0、单 worker 合成 workload 的两个构建 profile 结果。工作负载为 65,540 字节、128 条规则、每条四个宽 offset 负向 matcher；总时间统计两次异步查询：

| Profile | 两次异步查询总时间（同步对照） | 最大单次 poll | 同 worker 探针延迟 | 显式 20 ms stop 的观察时间 |
| --- | ---: | ---: | ---: | ---: |
| debug | 2.859–3.018 s（同步 4.933 s） | 1.206–1.247 ms | 0.396–0.665 ms | 20.37 ms |
| release | 114.1–118.2 ms（同步 143.2 ms） | 52–56 μs | 29–62 μs | 20.06 ms |

该 workload 的 debug 最大 poll 从基线约 150–230 ms 降到约 1.2 ms，约两个数量级；release 最大 poll 为
52–56 μs。这是特定合成规则和输入上的调度／吞吐探测，不代表所有 MIME 数据的性能预测。T4 将 managed
async detection 的 deadline 和 stop signal 与 backend future 一起驱动；matcher 仍只在协作批次边界检查停止。
下面列出 T4 假钟回归覆盖。Repository 构建与查询的独立测量见[基准记录](../benches/repository_matching.md)，
不应与上方 synthetic matcher poll 数据混为一谈。T8 的本地整合门禁与 clean sibling 布局验证已记录于上文；
GitHub Rust CI 需在提交 profile/快照修复后复验。

## 2026-11-24 T4：受管操作 deadline 与停止协作

T4 提交 `b761851` 为 managed async detection 同时轮询 operation stop 与 deadline signal。deadline 到期可以唤醒
仍挂起且不会自行唤醒的 provider read，operation 以 `Stopped(Timeout)` 结束；回归还确认 reader 在 settlement
后释放。repository matcher 在协作批次间检查停止状态，因此 deadline 能在批次边界停止大量规则匹配。用户取消与
deadline 竞争时，已接受的 `UserCancelled` 原因保留。

对应的受控回归为 `test_managed_deadline_wakes_never_waking_provider_read_and_releases_reader`、
`test_managed_deadline_stops_cooperative_repository_matching` 和
`test_user_cancel_wins_when_racing_a_managed_deadline`。它们使用假钟及受控 provider，不证明真实系统命令已运行
或被 deadline 终止。

deadline 或 stop 使 async detection future 返回，不代表已经派发的 command 或 staging worker job 也已退出。它们
继续持有 operation 资源，直到 worker 实际结算；应用仍须等待 settlement，并解决或转交恢复责任，之后才能停止服务。
同步子操作没有可供 runtime 抢占的 async future，只在具体同步实现的协作检查点观察停止请求。
## MIME-039—055 逐项变更与阶段证据

> 本片段对应固定整合候选 `rs-mime 7cf770cd671f725b8ef24fc5ab91f4a2c0598514`。表中的 focused 回归和 T6 文档检查使用各阶段已验证结果；这里列出可复核的测试入口，不表示本片段生成时重新运行了全量验证。最终 CI、原工作区整合和 hosted workflow 证据留待最终验收补录。

| 条目 | 变更位置与端口 | 阶段回归证据 | 兼容性／迁移 |
| --- | --- | --- | --- |
| MIME-039 组关闭准入 | `rs-operation/src/v02/manager/core/internal/manager_core.rs`、`group_core.rs`；rs-mime `src/detector/mime_execution_services/mime_shutdown_observer.rs` | `rs-operation/tests/v03_scope_tests.rs::test_cached_parent_group_view_rejects_closed_foreign_group_and_keeps_true_descendants`；rs-mime `tests/detector/operation_runtime_tests.rs::test_operation_runtime_drain_accepts_external_parent_child_until_stop_freezes` | 关闭后，外部 parent 不能首次加入已关闭组；已接纳的组内父 scope 仍可派发后代。需在关闭前完成新的外部准入，或从已接纳父 scope 派发必要后代；沿用既有错误类型。 |
| MIME-040 worker 准备与关闭 | `src/command_execution/mime_execution_worker/fixed_mime_execution_port.rs`；`MimeExecutionPort` prepare/dispatch/shutdown 边界 | `test_worker_side_final_drop_does_not_join_other_workers`、`test_shutdown_waits_for_in_progress_prepare_and_worker_cleanup`、`test_full_queue_returns_unaccepted_job_while_accepted_work_drains`；`tests/detector/operation_runtime_tests.rs::test_operation_runtime_prepare_retries_after_failure_and_rejects_after_shutdown` | worker 启动仍需显式 `prepare()`；池外 `shutdown()` 负责 join，池内最后 owner 析构只关闭并唤醒，不等待同池 worker。队列满仍返回原 job／typed busy 语义。 |
| MIME-041 部分临时文件创建恢复 | `src/managed_temp_staging.rs`、`managed_temp_staging/staged_mime_input.rs`、`managed_temp_staging/managed_temp_cleanup.rs`；共享 local-files 创建错误恢复所有者 | `test_managed_staging_creation_recovery_is_settled_and_retryable`、`test_managed_staging_commit_failure_retains_partial_owner_and_permit`、`test_managed_staging_cleanup_failure_retains_owner_for_retry`；file 与 ffprobe 共用 staging helper | 创建错误携带部分文件 owner 时，owner 作为清理资源登记在 operation scope；应用需观察 settlement/recovery，成功或失败 outcome 本身都不等于清理完成。没有新增下游调用签名迁移。 |
| MIME-042 pending read-tree 索引注销 | `src/detector/internal/mime_budget/read_tree_registry.rs`、`pending_read_tree_binding.rs`、`pending_read_tree_root.rs` | `test_pending_root_removal_preserves_later_replacement`、`test_dropping_batch_bindings_for_distinct_parents_clears_pending_index` | 按对象身份注销 pending root，旧对象失败/丢弃不得删除后续替代绑定；不改变公开 API。 |
| MIME-043 祖先读取树延迟绑定 | `src/detector/mime_runtime.rs`、`src/detector/internal/mime_submission.rs`、`src/detector/internal/mime_budget/read_tree_registry.rs` | `test_operation_runtime_parallel_children_share_actual_read_budget`、`test_sequential_mime_children_keep_external_parent_read_budget`、`test_managed_provider_sibling_children_share_external_parent_read_budget`、`test_external_parent_read_budget_binding_failures_return_input` | 只有发生 Provider 读取的子孙才建立/复用祖先累计预算；唯一文件名短路和 owned bytes 不预占父锚点。实际读请求仍受首次有效绑定确定的树上限约束。 |
| MIME-044 capability 读取计划 | `src/detector/internal/mime_submission.rs`、async backend/provider registry；提交锁外读取 capability 并在执行时复用同一不可变计划 | `tests/detector/operation_provider_tests.rs::test_provider_capability_hook_can_reenter_runtime_before_submission_lock` | 保留既有 capability hook 签名；hook 可重入 runtime，执行阶段不再二次查询或以另一组能力判断。 |
| MIME-045 受计量 ManagedProvider | `src/detector/async_mime_detector_backend.rs`、`src/detector/mime_input.rs`、runtime/provider dispatch；扩展 async consumer fixture | `test_complete_extension_always_uses_metered_sequential_reads_and_eof_probe`、`src/tests/managed_staging_tests.rs::test_metered_random_access_reads_share_budget_and_reject_sequential_offsets`；operation consumer 覆盖重复读和父/兄弟累计额度 | 穷尽匹配 `BorrowedMimeInput` 的扩展需处理 `ManagedProvider`；该 enum 不再 `Copy`/`Clone`。Complete/SamplesSameObject 读取使用 `MeteredReadSession::read()`/`read_at()`；不得重开路径。Prefix(Bytes) 和现有同步 Magika Provider 不需改签名。 |
| MIME-046 provider-native basename | `src/detector/mime_detector_backend.rs`、`mime_detector_core.rs`、`internal/async_repository_matcher.rs`、file/Magika hooks | `test_typed_path_preserves_backslash_in_posix_basename`、`test_non_utf8_local_path_matches_lossy_native_basename`、`test_managed_provider_uses_native_basename_for_filename_short_circuit`、`test_managed_provider_candidate_fallback_preserves_object_key_basename` | 路径输入先用 native `file_name()` 语义提取 basename；POSIX basename 中的反斜杠保留为名称数据，非 UTF-8 本地 basename 单独 lossy 转换。`OwnedMimeInput::Bytes.filename` 保持调用方原有行为。 |
| MIME-047 bounded matcher work | `src/repository/mime_magic_matcher.rs`、`src/detector/internal/async_repository_matcher.rs`；报告：[MIME-047 matcher baseline and follow-up](mime-047-matcher-baseline.md) | `test_cooperative_work_budget_caps_cross_root_and_byte_comparisons` 确定性断言每 poll 最多 4,096 个 matcher work units；`test_cooperative_matching_yields_and_observes_stop_inside_wide_rule`、`test_cooperative_matching_yields_across_wide_shallow_branch` 覆盖协作让出/停止 | “4,096”是规则、节点、候选 offset、字节比较的工作单元上限，不是纳秒或延迟。报告另列 debug test profile 的实测 query/poll 时间、吞吐及 stop-response 样本；共享主机数据仅作方向性测量。旧版 max work/poll 未有计数器，不能声称旧/新工作量直接比较。 |
| MIME-048 scope deadline | `src/detector/internal/mime_submission.rs`、`src/detector/mime_operation_options.rs`；追加 worker 排队超时所有权回归 | `test_managed_deadline_wakes_never_waking_provider_read_and_releases_reader`、`test_managed_deadline_stops_cooperative_repository_matching`、`test_user_cancel_wins_when_racing_a_managed_deadline`、`src/tests/command_execution_tests.rs::test_timeout_outcome_keeps_accepted_queued_command_owner_until_worker_returns` | Async detection 同时等待 stop/deadline/backend future，超时以 `Stopped(Timeout)` 表示；用户取消竞争时保留已接受原因。同步子操作只在协作检查点观察 stop，不会被 runtime 强制抢占。已派发 command/staging job 的 owner 继续跟踪到 job 实际 settlement；不得把 outcome 当作 worker 已退出。真实系统命令未由这些受控回归证明。 |
| MIME-049 跨 registry canonical identity | `src/detector/async_selection.rs`、detector/classifier registry 的共享 selector catalog/快照边界 | `test_sync_and_async_mime_selectors_share_canonical_identity`、`test_classifier_sync_async_selector_conflicts_are_symmetric`、`test_async_registry_aliases_are_shared_by_clones_and_failed_registration_is_atomic`、`test_inventory_selectors_participate_in_cross_capability_conflicts` | 同一 canonical ID 可有同步/异步能力；冲突 alias 不再可分别指向不同 Provider。已有冲突配置需改名或统一 canonical ID；成功注册 API 保持。 |
| MIME-050 managed lifecycle 示例 | `examples/managed_lifecycle/mod.rs`、双语指南同源 marker、`tests/documentation_version_tests.rs`、documentation consumer workflow | `test_managed_lifecycle_examples_match_runnable_consumer` 通过；示例与 consumer 输出 `Some("application/pdf")`、`Prefix(9)`、9 bytes；包含受控读取失败、settlement 和 recovery transfer/retry 路径 | 应用负责保留 recovery store/retry queue；释放 typed handle、处理 recovery 后再 stop/drain。无 API 迁移。 |
| MIME-051 managed runtime 文档 | 双语 README/指南；`MimeRuntime` 与 file provider Rustdoc | `test_provider_configuration_examples_match_compilable_consumer`、`test_managed_lifecycle_examples_match_runnable_consumer`、`cargo doc --locked --no-deps` 已通过阶段检查 | 文档明确 services 默认注入 repository、sync-only provider 在选择 factory 阶段失败、拒绝输入返还时点、file 同步与 managed 能力及 classifier 拒绝。无 API 迁移。 |
| MIME-052 XML supported subset/errors | `src/repository/mime_repository.rs`、`xml_parser.rs`、`src/tests/mime_type_tests.rs`、`tests/fixtures/repository-supported-subset.xml`、指南/Rustdoc | `test_repository_fixture_loads_supported_rules_and_ignores_other_standard_elements`、`test_numeric_magic_attribute_errors_preserve_the_xml_attribute_name`、`test_from_xml_rejects_numeric_magic_values_that_exceed_matcher_width`；DOCTYPE 原文行列和 prolog 顺序回归保留 | 依赖方应只依赖 documented standard element subset 和 shared-mime-info namespace；其他标准元素/外部 namespace 扩展被忽略，非法标准属性仍报结构化错误。 |
| MIME-053 MagicIndex 顺序与去重 | `src/repository/magic_index.rs`、Rustdoc | `src/tests/magic_index_tests.rs::test_magic_index_deduplicates_independent_equal_priority_rules_in_stable_order` | 多条同优先级同 MIME magic 规则只产生一个 MIME 候选，跨 MIME 的优先级与插入顺序保持稳定；无公共 API 迁移。 |
| MIME-054 验收证据索引 | `doc/acceptance-0.22.0-operation-0.2.zh_CN.md`：固定仓库图、历史阶段证据、环境限制与本轮状态 | 文档交叉核对固定提交和各阶段回归入口；本条是验收记录，不另造测试通过结果 | 必须区分固定候选与历史 baseline；不要把 phase-focused green 写成最终 CI、原工作区整合或 hosted 通过。 |
| MIME-055 Markdown 入口与包内链接 | `README.md`、`README.zh_CN.md`、两份指南、验收/指南相对链接 | `tests/package_document_links_tests.rs::test_packaged_markdown_links_resolve_inside_package_file_set` 通过；双语共享代码片段由 `documentation_version_tests` 检查 | 保留双语切换入口和真实文件目标；验收页对 MIME-047 使用同目录相对链接。无 API 迁移。 |

### MIME-047 工作量与实测延迟

工作计数器给出确定性上限：实现后每个 poll 至多处理 4,096 个 matcher work units；它统计规则、matcher 节点、候选 offset 与字节比较，既不计纳秒，也不是延迟样本。延迟来自独立的 debug test-profile 计时：例如 late-mismatch 场景新的 query 样本为 119,484 / 115,340 / 124,952 ns，最大 poll 样本为 156,810 / 286,726 / 291,779 ns；64 条 normal-rules 场景新的 query 样本为 55,328 / 62,159 / 63,947 ns，最大 poll 为 112,761 / 96,398 / 158,263 ns。测量在共享 Linux 主机上、未固定 CPU affinity，单样本平均 200 次 sync 与 200 次 async 查询；属于方向性实测，不构成通用性能保证。旧实现没有 work counter，因此只可比较旧/新实测时间，不能推断旧实现每 poll 的工作量。

### 本轮固定候选与证据边界

| 仓库 | 固定候选 | 本轮角色 |
| --- | --- | --- |
| `rs-operation` | `5e52eb7e7e5073f556394185001aa1fb966546ec` | `qubit-operation 0.3.2` 候选；包含组关闭准入与 cached group view 回归，是本轮 operation 边界，不沿用 T6 固定依赖 baseline `636ad4ebfe9d27b63e886d5935286faf27430301` 作为最终候选标识。 |
| `rs-mime` | `7cf770cd671f725b8ef24fc5ab91f4a2c0598514` | 合并 T1—T6 后的固定 MIME 验收候选；此片段只基于该对象库与阶段证据整理。 |
| `rs-magika` | `bc3480261c3829512c808ec9376cec94008606a5` | 同轮 T4 Magika 同步入口/消费者候选；它是兄弟消费者，不是 rs-mime 的依赖，也不代表 Magika 异步集成。 |

最终证据待补：固定候选下的最终 `.infra/bin/align-ci.sh`、`.infra/bin/ci-check.sh` 与 registry 发布解析；原工作区依赖路径整合后的必要复验；GitHub hosted Rust CI；真实系统 `ffprobe`；T1 新并发关闭回归的最终候选执行结果。上述项目目前保留为待验收项，不从 focused tests 或 task worktree 结果推断。

### T7 中间检查点（尚未放行）

在当前整合工作树中，`.infra/bin/align-ci.sh` 于 Rust 1.94.0 下通过（日志：任务外 `/tmp/rs-mime-t7-validation-7ba30d05/align-ci-checkpoint.log`）。覆盖缺口诊断后新增 focused tests：`test_candidate_selection_filters_unmapped_backend_results`、`test_temp_creation_worker_panic_returns_error_and_releases_reservation`、`test_classifier_configuration_defaults_and_mutators`，各自定向运行通过。正式 `.infra/bin/coverage.sh` JSON 报告显示 functions `1029/1088`（94.58%）、lines `13301/14099`（94.34%）、regions `23045/24366`（94.58%），低于 functions 95% 门槛；报告和日志位于任务外 `/tmp/rs-mime-t7-validation-7ba30d05/coverage-after-three-tests.json` 与 `coverage-after-three-tests.log`。因此本检查点不代表完整 CI 通过，仍需整合并行补测并重跑 coverage 与完整 CI。

本地 probe 报告 `/tmp/rs-mime-t7-probe/report.txt` 与 `followup-report.txt` 记录：根 crate、两个消费者的 locked metadata 一致，消费者和 managed lifecycle 示例运行、package list、`/usr/bin/file` 后端及受控 ffprobe runner 检查通过。环境没有真实 `ffprobe`。隔离 registry 解析尝试失败，索引未提供 `qubit-mime ^0.22`（当时可见版本止于 0.21.1）；这不是 crates.io 发布安装验证。尚无 GitHub hosted CI 结果。

### T7 最终本地门禁结果

最终被测 MIME 代码提交为 `2497729ad407a9496d04416a56923df6e053a05e`（基于并行整合提交 `deb45be697290a112ae8f07b4e0b61f4081ee66a`）。Rust 1.94.0 下 `.infra/bin/align-ci.sh` 退出码 0，完整 `.infra/bin/ci-check.sh` 退出码 0；日志分别为任务外 `/tmp/rs-mime-t7-validation-7ba30d05/align-ci-integrated.log` 和 `ci-check-integrated.log`。完整 CI 的 coverage 为 functions `1040/1088`（95.59%）、lines `13409/14206`（94.39%）、regions `23277/24572`（94.73%）；本地采集 JSON `/tmp/rs-mime-t7-validation-7ba30d05/coverage-integrated.json`，文本摘要在仓库生成的 `coverage.txt`。CI 包含 style、Clippy、coverage-only Clippy、验证、strict docs、README、release build、feature matrix、project hook、package、coverage 和 audit。单测/集成测试 profile 汇总各 420 项通过；Rustdoc 64 项通过、1 项忽略；文档包链接测试 3 项通过。依赖 audit 扫描 89 个 Cargo.lock 依赖，未报告漏洞。

合并后 `.infra/bin/align-ci.sh` 仅对 selector recovery 三个 Rust 文件执行了 rustfmt 和 import 排序，变更已审核并提交；`git diff --check` 通过。此前诊断 coverage JSON 曾报告 5 个函数数据不匹配，但最终完整 `ci-check.sh` coverage 阶段通过且完整日志无该警告。

本验收页更新后重跑 `cargo test --locked --test package_document_links_tests --test documentation_version_tests`，退出码 0：包内 Markdown 链接 3 项通过，文档版本/示例同步 6 项通过。

### 后续 hosted 回归及新候选

旧候选 `64838461fb11a453f0c1f38ab64c183fd15a315b` 的本地正式 CI 已通过；推送后的 hosted 日志 `/tmp/7ba30d05-rs-mime-hosted-failed.log` 显示 `test_repository_async_yields_and_stops` 在 no-default-features 与 coverage/all-features 两条 job 中均失败，实际结果是 target `Succeeded`，并触发测试的“应在 stop 前结束”断言。根因是原测试仅构造 256 条规则，低于 repository matcher 的单 poll 工作量，heartbeat 取消与 target 完成因此依赖 executor 调度顺序。

修复提交 `f95f83e1992e354799267ab0e1eeb7fb94f7eae0` 把回归改为 8,192 条规则，并由同一个 test body poll backend future；首次观察到 `Pending` 时立即请求 stop，再断言 matcher 返回错误且 operation outcome 为 `Stopped(UserCancelled)`。此方式验证真实协作 yield 和取消观察，不依赖 sleep 或外部 heartbeat。no-default-features 和 all-features 的定向用例各重复 10 次全部通过，`align-ci.sh` 也通过；日志分别为 `/tmp/rs-mime-hosted-race-no-default.log`、`/tmp/rs-mime-hosted-race-all-features.log` 和 `/tmp/rs-mime-t7-validation-7ba30d05/align-ci-hosted-regression.log`。

随后候选 SHA `b58d998c01930341eb88e79bf12854cbc19d770d` 的 hosted Rust CI run 45339680383 中，no-default-features matrix job 138261490029 的 420 项测试有 419 项通过，`test_managed_deadline_stops_cooperative_repository_matching` 失败，错误为 `accepted matcher should register its deadline`（[job 日志](https://github.com/qubit-ltd/rs-mime/actions/runs/45339680383/job/138261490029)）。提交 `de20c6e8bd2e91abda5151b894a6e0cf12cf8f1e` 随后把测试改为由 backend 在首次观察到 `Pending` 后推进假钟；在本轮基线 `8e02d8509f927319570d8251a518e487f93143a4` 上用 Rust 1.94.0 执行 `cargo test --locked --test mod test_managed_deadline_stops_cooperative_repository_matching`，1 项通过。查询时 hosted run 仍为 `in_progress`，这里只记录已结束的失败 job，不据此给整条 run 下结论。

当前 MIME 候选将固定 operation sibling `6071fc67f6123582353620d51ffd298ae3ddd937`。Cargo manifest 仍解析 `qubit-operation 0.3.2` 本地 path，根锁文件与两个消费者锁文件的 package/version 预期保持一致；更新 pin 后先核验 resolved metadata 和锁文件差异，再执行正式门禁。此处记录时新候选完整 `ci-check.sh` 尚待运行；旧 SHA 的通过结果不能代表新候选通过。

此前固定候选 probe 的边界仍适用：locked metadata、两个消费者与 managed lifecycle 示例、`cargo package --list --locked`、真实 `/usr/bin/file` 和受控 ffprobe runner 均为本地通过；真实 `ffprobe` 不在 PATH。隔离 Cargo registry 检查因当前 registry 无 `qubit-mime ^0.22` 解析版本而失败，不代表发布解析通过。上述旧候选 probe 未运行 GitHub hosted workflow；本段记录的 `b58d998` hosted run 只确认到一个失败 job，整条 run 仍未完成，因此不推断其余 hosted jobs 或 crates.io 发布解析通过。
