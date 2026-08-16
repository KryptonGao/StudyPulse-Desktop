# StudyPulseClient Code Review Report

审查日期：2026-08-16
审查分支：`agent/phase3-learning-exam-ai`
审查基线：`f6721ed feat: implement Phase 3 learning and exam AI`
审查方式：只读静态审查 + 四个并行 SubAgents（Rust/Core、Tauri/安全、React/TypeScript、测试/工程）+ 本地验证。除本报告外未修改业务代码。

## 结论摘要

当前代码没有确认的 P0，但存在多项应在发布前处理的 P1 问题，主要集中在：

- Workspace/Agent 的符号链接边界和并发读改写；
- 外部路径写入、备份导出以及 OAuth 深链安全；
- Notebook、计时器、考试答题和 AI 结果的状态/数据一致性；
- 发布 Workflow 会对同一版本 Release 执行覆盖上传。

建议先处理 P1，再补齐 Phase 3 与 Agent 的并发、失败恢复和跨层契约测试。现有自动化检查全部通过，说明问题主要在未覆盖的竞态、错误路径和 IPC 安全边界，而不是编译或基础单元测试回归。

严重度定义：P1 = 发布前应修复；P2 = 下一迭代应修复或明确接受风险；P3 = 低风险正确性/可维护性问题。

## 验证结果

| 检查 | 结果 |
|---|---|
| `npm test` | 通过，3 个测试文件、8 个测试 |
| `npm run typecheck` | 通过 |
| `npm run lint` | 通过 |
| `npm run build` | 通过；Vite 提示主 JS chunk 约 642 kB |
| `cargo fmt --manifest-path core/Cargo.toml --all -- --check` | 通过 |
| `cargo test --manifest-path core/Cargo.toml --workspace` | 通过，工作区测试无失败 |
| `cargo clippy --manifest-path core/Cargo.toml --workspace --all-targets -- -D warnings` | 通过，无 warning |
| `cargo check --manifest-path src-tauri/Cargo.toml` | 通过 |
| `git diff --check` | 通过 |
| 工作树 | 审查前后无业务代码未提交修改；本报告为新增交付物 |

## P1 发现

### P1-01 Agent 固定目录绕过符号链接防护

位置：`core/crates/studypulse-workspace/src/workspace.rs:375-510`。

`Agent/turns`、`Agent/runs`、`Agent/memory`、`Agent/artifacts` 和 `Agent/notebooks.json` 由 `root.join(...)` 直接访问，未统一调用 `ensure_no_symlink_components`。`SafeRelativePath::parse` 只校验字符串，不等价于实际文件系统的符号链接检查。

影响：本地其他进程或被篡改的 Workspace 可将 Agent 固定目录/文件替换为符号链接，导致 Workspace 外部文件被读、覆盖或追加，违背仓库规定的路径安全边界。

建议：为所有 Agent 固定路径增加统一的“逐段 symlink/reparse-point 检查 + canonical root 检查”辅助函数；为 turns/runs/memory/artifacts/notebooks 增加 symlink 回归测试。

### P1-02 JSONL/数组读改写没有覆盖完整事务锁

位置：`core/crates/studypulse-workspace/src/workspace.rs:708-720,1298-1305,1465-1505`。

`upsert_jsonl_record`、`delete_jsonl_record`、`upsert_subject` 等先读旧文件、修改内存快照，最后才由 `write_jsonl_records`/`write_json_value` 获取 `write_lock`。两个并发调用可同时读取同一旧快照，后写入者覆盖先写入者。

影响：任务、成绩、学习会话、科目等记录可能静默丢失；`atomic_write` 只保证一次发布的完整性，不能解决读改写竞态。

建议：让锁覆盖“读取—修改—序列化—atomic_write”全过程，并拆分已持锁的底层写函数，避免重复加锁。

### P1-03 计时器状态跨 Workspace 泄漏或丢失

位置：`core/crates/studypulse-ffi/src/lib.rs:1011-1058,2676-2759,3249-3277`。

`active_timer` 在 `install_workspace`、`close_workspace` 和 `rebuild_runtime` 时没有清空；`start_timer` 也不要求已打开 Workspace。切换 Workspace 后调用 `finish_timer` 会把旧 Workspace 的计时会话写入新 Workspace；没有 Workspace 时，`finish_timer` 先 `take()` 再返回错误，导致计时状态丢失。

建议：计时器必须绑定 Workspace ID；切换/关闭前拒绝或明确取消 active timer；`start_timer` 要求 Workspace；`finish_timer` 在持久化成功后再清除内存状态，并覆盖写失败测试。

### P1-04 Coach 批准不是原子且不可安全重试

位置：`core/crates/studypulse-ffi/src/lib.rs:2341-2412`。

批准流程先逐个 `upsert_task`，最后才把 proposal 标记为 `approved`。任一任务写入或最终 `write_coach_data` 失败，都可能形成“任务已创建、proposal 仍 pending”的半完成状态；重试会生成随机任务 ID，造成重复任务。未知的 `decision` 值也会落入 reject 分支。

建议：在 Workspace 写锁下完成任务与 proposal 的同一事务；使用确定性任务 ID/应用动作 ID 做幂等；严格解析 `approve/reject` 枚举，拒绝未知值。

### P1-05 Backup Merge 缺少冲突决议时默认采用 incoming

位置：`core/crates/studypulse-workspace/src/backup.rs:265-280,1424-1496`。

Merge 逻辑只有在显式 `use_incoming == false` 时保留本地；缺少某个 conflict key 时会直接写入 incoming。当前桌面 UI 只暴露 Replace，但 FFI/Core 已暴露 Merge 能力，调用者漏传决议即可静默覆盖本地记录或媒体。

建议：`apply_backup` 开始前强制要求 resolutions 与 inspection 中的全部冲突一一匹配，同时拒绝未知决议键；缺失决议直接失败，不采用默认值。

### P1-06 Agent 事件 sequence 分配与 buffer 插入不是原子操作

位置：`core/crates/studypulse-agent/src/lib.rs:2022-2074`。

事件先用原子计数器分配 sequence，随后经过 checkpoint/run-log 持久化，最后才插入内存 buffer。并发下 sequence=2 可能先于 sequence=1 返回；前端按 `event.sequence` 推进 cursor 到 2 后，会永久跳过迟到的 sequence=1。

建议：用单一事件锁覆盖 sequence 分配、持久化和 buffer 插入，或让 `wait_for_events` 保证按 sequence 排序并保留 gap，直到缺口补齐。

### P1-07 OAuth 自定义深链缺少 state/PKCE

位置：`core/crates/studypulse-model-client/src/lib.rs:300-345`、`src-tauri/src/lib.rs:386-404`、`src-tauri/tauri.conf.json:29-32`。

登录 URL 只携带 `return_to`；回调只校验 `studypulse://auth/callback` 和 token 前缀，没有 state、nonce、PKCE，也没有绑定当前登录流程。自定义 scheme 还可能被本机其他应用抢先注册/截获。

影响：Cloud access/refresh token 可能泄露，或任意收到的首个合法格式深链触发账号登录注入。

建议：优先使用 Universal/App Links 或 loopback + PKCE；state/nonce 由宿主生成、绑定并一次性消费；避免在深链 query 中直接传 bearer token。

### P1-08 报告写入命令允许 IPC 调用者指定任意扩展和外部文件

位置：`src-tauri/src/lib.rs:1027-1061`。

`report_destination` 只比较“文件后缀是否等于调用者传入的 `extension`”，没有允许列表；随后 `write_report_file` 对路径执行 `fs::write`，最终符号链接也可能被跟随。Workspace 外部任意可写文件均可成为目标，例如传入 `.plist` 路径和 `extension=plist`。

建议：宿主固定允许的 `md/html/png` 类型；保存路径应来自 OS 保存对话框的授权结果；拒绝最终符号链接，并使用临时文件 + 原子替换。

### P1-09 Backup 导出可截断任意目标文件

位置：`src-tauri/src/lib.rs:1215-1219`、`core/crates/studypulse-ffi/src/lib.rs:2810-2828`、`core/crates/studypulse-workspace/src/backup.rs:900-907`。

`archive_path` 从 IPC 直接传入，最终执行 `File::create(archive_path)`，没有扩展名、目标授权、最终 symlink 或“不可覆盖已有文件”保护。恶意或受污染 WebView 可将路径指向其他用户文件并截断。

建议：使用保存对话框授权路径；限制备份扩展名；拒绝 symlink/已有目标，或写入同目录临时文件后原子 rename；对 Core/FFI 入口也重复验证。

### P1-10 Agent Notebook 首次加载期间可覆盖全部历史

位置：`frontend/src/app/App.tsx:504-545,616-629`、`core/crates/studypulse-workspace/src/workspace.rs:1524-1542`。

`notebooksQuery.data` 未加载时被当成 `[]`，但“新建 Notebook”和“Run Agent”操作仍可用。此时 `persist([newNotebook, ...notebooks])` 会调用整文件替换，可能把已有 `Agent/notebooks.json` 覆盖为只含新 Notebook 的列表。

建议：查询首次成功前禁用所有 Notebook 写操作；区分 loading/error 与空列表；更稳妥的是提供按 ID upsert 或版本化 compare-and-swap。

### P1-11 Agent 结构化结果字段命名不一致

位置：`core/crates/studypulse-agent/src/lib.rs:390-400,1966-1973`、`frontend/src/app/App.tsx:593-605`。

Rust `TurnResult` 使用 camelCase 序列化，事件 payload 字段为 `outputJson`；React 只读取 `output_json`。因此 QuestionLab/Visualize 的结构化输出无法进入 `structuredOutput`，页面只能退化为文本/原始 JSON。

建议：统一 wire contract，或前端兼容 `outputJson`；增加 Rust→Tauri→React 的真实事件 payload 契约测试。

### P1-12 考试自由答题逐字写入导致乱序覆盖

位置：`frontend/src/app/P2Pages.tsx:158-177`。

`textarea.onChange` 每个按键都基于当前 React Query 快照构造完整 simulation，并异步 `upsertExamSimulation`。快速输入时旧快照写入可能晚于新快照，最后完成的写入会覆盖更新答案，导致丢字或事件乱序。

建议：使用本地答案状态；按题目 debounce 保存；对 mutation 串行化，或由 Core 提供带版本的条件更新。

### P1-13 考试超时自动提交可能重复触发评分

位置：`frontend/src/app/P2Pages.tsx:161-177`。

倒计时 effect 的闭包捕获旧 `busy=false`；到零后 interval 每秒继续调用 `submit(true)`，直到查询刷新为 grading。评分尚未完成期间可并发发起多次提交/AI 评分。

建议：使用按 simulation ID 的一次性 `submittedRef`，到零立即清理 interval；Core 层增加 grading 状态的幂等转换和唯一约束。

### P1-14 Exam AI 展示/讨论的记录没有按当前考试筛选

位置：`frontend/src/app/P3Pages.tsx:68-84`。

`prediction` 和 `autopsy` 都取各自 collection 的全局最新记录，而不是按当前 `examChoice` 的 `payload.examId` 筛选。用户选择考试 A 时可能看到、讨论或应用考试 B 的预测/复盘结果。

建议：按记录中的 `examId` 与当前选择过滤；无匹配记录时隐藏操作区；为单科/综合考试分别测试。

### P1-15 Release Workflow 在每次 main push 上覆盖同一版本

位置：`.github/workflows/release.yml:3-6,91-130`。

Workflow 监听所有 `main` push，并从 `package.json` 读取固定版本生成 tag；若 Release 已存在就执行 `gh release upload --clobber`。普通修复提交也会重建并覆盖已发布版本的 DMG/EXE 和 Release Notes。

建议：改为版本 tag 或手动发布触发；发布前校验 tag 与三处版本号；已存在 Release 默认失败，禁止无条件 `--clobber`。

## P2 发现

| 位置 | 问题与影响 | 建议 |
|---|---|---|
| `src-tauri/src/lib.rs:294-338,421-515,1047-1061,1159-1170` | 多个 async command 直接调用 keyring、文件 I/O 或同步 Core，未统一进入 `spawn_blocking`；慢磁盘/Keychain/大 payload 会阻塞 Tauri runtime。 | 所有同步边界统一通过 `core_call` 或独立 blocking wrapper。 |
| `core/crates/studypulse-model-client/src/lib.rs:169-186,742-751,1495-1508`；`src-tauri/src/lib.rs:100-106` | URL、Provider 错误 message 和内部错误字符串可能原样穿过 FFI/Tauri/UI，存在 userinfo/token 泄漏风险。 | 使用稳定错误码；对 URL userinfo、token、API key 和 provider 原文统一脱敏。 |
| `src-tauri/src/lib.rs:1055-1061` | 报告图片先完整 Base64 解码，再检查 20 MiB，超大输入可先消耗内存/CPU。 | 先按 Base64 长度拒绝，使用有上限的解码并放入 blocking pool。 |
| `src-tauri/src/lib.rs:1159-1170` | `metadata` 检查与 `fs::read` 之间存在 TOCTOU，且两次操作都可能跟随 symlink，1 MiB 导入限制可被并发替换绕过。 | 一次性打开文件，拒绝 symlink，对句柄做 bounded read/fstat。 |
| `src-tauri/src/lib.rs:533-565`；`core/crates/studypulse-agent/src/lib.rs:817-826,841-872,2088-2090` | Agent goal/history 入口缺少总字节数、条数和单条消息限制；checkpoint 写入错误被忽略。 | 在 Tauri/Core 双层限制输入；checkpoint 失败要显式失败或进入可见错误状态。 |
| `core/crates/studypulse-runner/src/main.rs:201-220` | Runner 用 `wait_with_output` 完整缓存 stdout/stderr 后才截断，恶意代码可造成 OOM。 | 实时读取并限流/截断，必要时终止进程。 |
| `core/crates/studypulse-agent/src/lib.rs:828-835`；`core/crates/studypulse-tools/src/lib.rs:898-945` | Agent 把空 `source_paths` 转成 `Some(&[])`，导致 list/search 返回空库；仓库约定空选择应表示全库。 | 空 selection 映射为 `None`/全库；`read_source` 仍要求显式选源。 |
| `core/crates/studypulse-ffi/src/lib.rs:1480-1497`；`core/crates/studypulse-workspace/src/features.rs:905-925` | Phase 3 JSON 记录、Home Ask 历史和单条 payload 没有大小/条数上限，可无限增长并触发全量重写和备份膨胀。 | 限制 JSON、payload、消息和历史条数；超过上限时保留可恢复错误。 |
| `frontend/src/app/P3Pages.tsx:87-94` | Exam Autopsy 先写入图片，AI 失败时只提示错误，没有清理接口，反复失败会产生孤立媒体。 | 使用临时附件或增加删除/回收机制。 |
| `frontend/src/app/P3Pages.tsx:87-93`；`core/crates/studypulse-ffi/src/ai.rs:1305-1343` | Autopsy 允许零附件执行，模型可能在无题图时生成臆测结果。 | UI 与 Core 同时要求至少一个图片附件。 |
| `frontend/src/app/App.tsx:303-355`；`frontend/src/app/P3Pages.tsx:41,73` | 任务/成绩/P3 写入只失效资源自身 query，Today 等派生 query 可能持续显示旧数据。 | 建立统一的资源依赖 invalidation 图，至少同步刷新 today/trends/due-mistakes。 |
| `frontend/src/app/P3Pages.tsx:37-44,68-75` | Phase 3 查询不处理 loading/error，失败会伪装成“没有数据”。 | 增加统一 loading/error/retry UI，并禁用依赖失败数据的生成操作。 |
| `frontend/src/app/P2Pages.tsx:133-152,158-179` | Reverse Planner 的 grades/mistakes/tasks 或 Exam Simulation 的 query error 被当成空数据继续工作，AI 可能基于不完整上下文生成结果。 | 聚合所有关键 query 的 error 状态并阻断生成。 |
| `frontend/src/i18n.tsx:272-281` | `zh-TW`/`ja`/`ko` 没有合并 `p2En`，且 fallback 的 `en` 基础字典也不含这些 key，P2 页面会显示原始 key。 | 五语言补齐 P2 字典，并增加 key completeness 测试。 |
| `.github/workflows/release.yml:3-60` | CI 没有 `pull_request` 触发器，也没有 Rust test/fmt/clippy 门禁。 | 增加 PR workflow，并执行 Core/Tauri Rust 检查。 |
| `core/crates/studypulse-workspace/src/workspace.rs:1370-1377` | 通用 JSONL 读取只校验 envelope/UUID，不调用 Grade、StudySession、Routine、TimeInvestment 等业务 `validate()`。 | 为关键模型补齐 validate，并在读写入口统一调用。 |
| `core/crates/studypulse-workspace/src/workspace.rs:638-660` | `tasks.jsonl` 专用读取检查 envelope/value ID，但不检查重复 UUID。 | 增加 HashSet 重复检测并返回 `MalformedData`。 |
| `core/crates/studypulse-workspace/src/analytics.rs:517-575` | Time investment 的 `direct_seconds` 聚合包含子任务时长，并与 total 使用同一值，报告分解错误。 | 分离 direct predicate 与 descendant-inclusive predicate。 |
| `core/crates/studypulse-ffi/src/lib.rs:1555-1560`；`core/crates/studypulse-ffi/src/ai.rs:664-769` | Phase 3 action/item ID 未强制唯一，重复 ID 会造成 React key 冲突且应用时 `.find()` 只处理第一项。 | 归一化时用 HashSet 拒绝重复 ID。 |
| `core/crates/studypulse-ffi/src/lib.rs:1320-1453` | AI feature 同步 facade 没有整体 deadline/cancel；多轮模型调用可能让单个 IPC 请求长时间等待。 | 增加总 deadline，超时自动 cancel，并在 UI 提供取消状态。 |
| `core/crates/studypulse-workspace/src/workspace.rs:1058-1192` | Coach data 的 read-modify-write 在锁外读取，多个提案/消息更新可能互相覆盖。 | 将 Coach RMW 放进同一 write lock。 |
| `core/crates/studypulse-agent/src/lib.rs:1017-1022,1938-1940` | cancel 与 Completed/Failed 的终态转换缺少 CAS/终态锁，可能产生取消后完成或重复终端事件。 | 使用原子终态转换或统一状态锁。 |
| `core/crates/studypulse-agent/src/lib.rs:1964,2076-2090`；`workspace.rs:457-483` | Agent run log/checkpoint 持久化错误被静默吞掉；run log append 非 atomic replacement，可能与内存状态不一致。 | 保留并上报持久化错误；设计可靠 append/recovery 方案。 |
| `core/crates/studypulse-workspace/src/workspace.rs:568-572,1627` | 导入源的唯一命名在获取 write lock 前用 `.exists()`，并发导入可能选中同名并互相覆盖。 | 在锁内命名，使用 `create_new`/重试提交。 |

## P3 发现

`core/crates/studypulse-workspace/src/analytics.rs:409-438` 的 `apply_srs` 会把非法 quality clamp 后继续走分支；其中 quality=2 会按 Easy 处理。当前 FFI 入口已有校验，因此优先级较低，但建议让纯函数直接拒绝非法质量值，避免其他 Rust 调用方误用。

## 已确认的正向控制

- Cloud/BYOK 凭据路径总体使用系统 keyring，未发现写入 Workspace、localStorage 或 ProviderStatus 的 confirmed 路径。
- Media、Agent artifact、备份导入的 Core 路径校验、大小限制、ZIP 校验和多数 symlink 检查已存在；未发现已确认的普通 `..` 路径穿越。
- Agent 的 `prepare` 与 `execute` 分离，未发现 `create_task` 在 prepare 阶段产生写副作用。
- Agent 前端 cursor 使用事件的 `sequence`，没有把数组长度当作 cursor。
- Markdown 走 `rehypeSanitize`，HTML 可视化使用空 sandbox iframe；本次未确认直接 XSS，但仍缺少 hostile payload 回归测试。

## 测试缺口

当前测试通过，但没有覆盖本报告的关键触发条件：

1. Agent 固定目录 symlink、事件乱序、取消/完成竞态、checkpoint/run-log 写失败；
2. JSONL/数组/Coach/导入源的并发读改写；
3. Backup Merge 缺失决议与未知决议；
4. Timer 的 Workspace 切换、无 Workspace finish、持久化失败；
5. Notebook 首次加载竞态和整文件覆盖；
6. Rust→Tauri→React 的 `outputJson` 契约、多语言 key completeness；
7. Exam Simulation 逐字保存、超时重复提交、考试记录过滤；
8. Phase 3 无附件、AI 失败清理、payload/action ID 限制；
9. Tauri IPC 路径、symlink、TOCTOU、Base64 超限和敏感错误脱敏。

## 建议修复顺序

1. 先修复 Agent/Workspace symlink 边界、外部路径写入、OAuth 深链和 Backup Merge 冲突决议；
2. 统一 Workspace 的完整 RMW 锁，并处理 timer、Coach、Notebook 的跨状态一致性；
3. 修复考试答题/超时/Exam AI 过滤，以及 Agent `outputJson` wire contract；
4. 修复 Agent 事件序列/终态竞态和持久化错误处理；
5. 补齐 PR Rust CI、错误/加载态、i18n 和派生 Query invalidation；
6. 为每个 P1 增加最小回归测试后再进行下一轮发布构建。
