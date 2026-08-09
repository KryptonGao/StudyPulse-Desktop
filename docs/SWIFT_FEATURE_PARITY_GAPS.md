# StudyPulse Swift → StudyPulseClient 功能迁移差距清单

> 对比日期：2026-08-08
>
> Swift 参考端：`/Users/chenkaigao/Documents/Program/Swift/StudyPulse`
>
> 桌面端：`/Users/chenkaigao/Documents/Program/StudyPulseClient`

## 结论摘要

当前 `StudyPulseClient` 已经不是 Swift 端的空壳移植：基础学习记录闭环、Diary/Trends/文字闪卡 SRS、AI Coach、Reverse Planner、Exam Simulator、学习报告、Workspace/Library/Agent 都已经有桌面端实现。

尚未搬完整的部分主要有两类：

1. Swift 端的“深度学习工作流”还没有在桌面端形成可用闭环，尤其是错题工作台、考试详情与复盘、例程/阶段、完整 Time Investment、成就/植物和专项 AI 工具。
2. Swift 端依赖 Apple 平台能力的部分没有对应桌面实现，包括 HealthKit、EventKit/Reminders、WidgetKit、ActivityKit、App Intents/Shortcuts 和本地通知。这些不应直接当作普通 React 页面搬运，而应先确定桌面替代方案或明确为平台差异。

本报告的判断标准是：

- **已迁移**：桌面端存在从 UI → Tauri → Rust Core/Workspace 的可用链路。
- **部分迁移**：有数据模型、FFI、存储或一个窄 UI，但 Swift 端的主要用户流程没有闭环。
- **未发现**：在桌面端当前 UI、TypeScript bridge、Rust Core/FFI 中没有找到对应的专用实现。通用 Agent 能力不等于该 Swift 专项功能已经迁移。
- **平台差异**：功能本身存在，但原实现依赖 iOS/Apple 框架，是否移植应单独做产品决策。

## 已迁移或基本对齐的范围

| 领域 | 当前桌面端状态 | 证据与说明 |
|---|---|---|
| Workspace / 本地优先存储 | 已迁移 | `core/crates/studypulse-workspace/src/workspace.rs` 提供 Workspace 创建、打开、JSONL、原子写入、路径安全和备份边界。 |
| 科目、成绩 | 基本迁移 | `frontend/src/app/App.tsx` 的 `SubjectsPage` + `models.rs` / FFI CRUD；目前仍是较简化的录入与列表界面。 |
| 任务 | 基本迁移 | `TasksPage` 能新增和完成任务；完整编辑、任务详情、统一待办聚合仍未对齐。 |
| 普通考试 / 综合考试数据 | 基本迁移 | `ExamsPage` 有普通考试和综合考试创建、列表、删除；考试深度工作流见后文。 |
| 错题基本记录 | 基本迁移 | `MistakesPage` 能手动新增、列出、加入 SRS 并复习；完整错题编辑工作台见后文。 |
| 学习日记 | 基本迁移 | `frontend/src/app/P1Pages.tsx` 提供 CRUD、日期、心情、精力、标签和 Markdown；日历与提醒未完全迁移。 |
| 趋势分析 | 基本迁移 | Rust `analytics.rs` 提供 90 天趋势、连续天数、科目趋势、心情/精力和 SRS 汇总。 |
| 文字闪卡 / SRS | 基本迁移 | 桌面端使用 Rust SM-2 兼容逻辑，质量值保持 Again=1、Hard=3、Good=4、Easy=5；当前为文字卡片。 |
| 学习计时器 | 基本迁移 | `TimerPage` 支持开始、暂停、继续、完成、取消和强度；会话详情、标注、心率和目标绑定尚未形成 UI 闭环。 |
| AI Coach | 基本迁移 | `P2Pages.tsx` 支持目标、分析、风险、提案、批准/拒绝和对话，且通过 Cloud AI/BYOK 调 Agent。健康驱动刷新、后台任务和通知未迁移。 |
| Reverse Planner | 基本迁移 | 有目标、成绩/错题/SRS/开放任务上下文、计划、弱点和每日任务；与 Swift 端的考试计划核心流程已对齐。 |
| Exam Simulator | 基本迁移 | 桌面端保留 10 题、计时、答题记录、自动保存/恢复、评分和行为分析流程。 |
| 学习报告 | 基本迁移 | Rust 计算 7/30 天报告；桌面端导出 Markdown/HTML/PNG，并提供打印/PDF 路径和分享。定时周报与更丰富的原生渲染仍缺失。 |
| Cloud AI / BYOK | 已迁移但架构不同 | 桌面端通过 Tauri/keyring 保存凭据，支持 Cloud AI 与 OpenAI-compatible BYOK；它不是 Swift 端直接复用 `UserDefaults` API Key 的实现。 |
| Library / Notebook / Agent | 桌面端新增能力 | 这是桌面端独有的 Workspace 资料库、Notebook 和受权限控制的 Agent 工具链，不属于 Swift 端“待搬功能”。 |
| 备份基础能力 | 基本迁移 | Workspace backup 支持校验、校验和、媒体和 schema 3/4 兼容；当前桌面 UI 主要暴露 Replace，Merge/逐冲突解决没有完整暴露。 |

## 主要未迁移功能矩阵

### 1. 错题深度工作台：最高优先级的可移植缺口

Swift 端的错题不是简单列表，而是一个包含采集、编辑、复习、分析和导出的工作台。桌面端目前在 `MistakesPage` 中只展示少量字段，新增表单只写入标题、科目、原题和出错原因；`MistakeNote` 中虽然保留了部分兼容字段，但用户没有对应的编辑入口。

| Swift 能力 | 桌面端状态 | 说明 |
|---|---|---|
| 错题集合 / 详情 / 编辑 | 部分迁移 | 有基础记录和 SRS，没有 Swift 端的分块编辑、详情页、搜索、按科目子页和完整字段编辑。 |
| 原题、错解、正解、错因的完整 Markdown 内容 | 部分迁移 | DTO 有字段，但当前桌面新增表单只覆盖原题和错因，错解/正解没有编辑 UI。 |
| 图片题目、图片错因、图片错解、图片正解 | 未迁移到 UI | Rust Workspace/FFI 有 `Media` 读写和图片字段，但前端没有图片选择、预览、删除和关联流程。 |
| 手写答题记录 | 未迁移到 UI | `HandwritingAnswerEntry`/base64 兼容字段已在 Core DTO 中保留；没有 PencilKit 等桌面输入替代。 |
| 语音备忘录 / 播放 | 未迁移 | Swift 的 `VoiceMemoManager`、录音 sheet 和播放视图没有桌面对应实现；客户端只有 `audio_file_name`/媒体边界。 |
| OCR 录入 | 未迁移 | Swift 使用 Vision；桌面端未发现 OCR 入口。可考虑 macOS Vision 或导入图片后离线识别。 |
| 错题 AI 解析 | 未迁移 | Swift 的 `MistakeAIAnalysisLLM` / `MistakeAIAnalysisSheet` 没有专用桌面流程；通用 Agent 不能算字段级迁移。 |
| AI 相似题 | 未迁移 | Swift `AISimilarQuestionFlowView`、生成/答题/批改流程未在桌面端出现。 |
| AI 自测题 | 未迁移 | Swift `AIQuizModels` 与 Quiz 三件套未在客户端 `types.ts`、`P2Pages.tsx` 或 Core feature 中出现。 |
| AI 自动思维导图 | 未迁移 | Swift `AutoMindMapLLM`/`AutoMindMapView` 没有对应的专用模型、节点渲染和持久化。 |
| 错题辩论 / 深入探讨 | 未迁移 | Swift `MistakeDebateSheet`/`AIDiscussionSheet` 的多轮上下文流程未迁移。 |
| 知识断层线与修复任务 | 未迁移 | Swift `KnowledgeFaultLine*`、`KnowledgeRepairTaskService` 没有桌面端模型或页面。 |
| 错题模式分析 / 保质期 | 未迁移 | Swift `MistakePattern*`、`MistakeShelfLife` 没有桌面端等价分析。 |
| 错题纠正计划 | 未迁移 | Swift `MistakeCorrectionPlan*` 没有桌面端对应。 |
| 标签图谱 | 未迁移 | Swift `TagGraphView`/布局/边模型没有桌面端页面。 |
| 错题 PDF 导出 | 未迁移 | Swift 有 A4 多页 `MistakePDFRenderer`；桌面端当前 PDF 只是报告页的打印路径，不是错题 PDF 导出。 |

**判断**：错题基础数据已经为部分扩展做好了兼容准备，但用户实际可用能力仍明显低于 Swift 端。这里应先做“详情编辑 + 媒体 + 错题 AI 解析/相似题/自测”基础闭环，再考虑图谱、知识断层线和 PDF。

### 2. 考试与成绩工作流：数据在，深度页面不足

桌面端 `ExamsPage` 当前是创建、列表和删除；Swift 端有考试日历、详情编辑、清单、关联错题、考试复盘、分数预测以及综合考试详情。

| Swift 能力 | 桌面端状态 | 说明 |
|---|---|---|
| 考试月历 / 日历日单元格 | 未迁移 | 客户端没有 `ExamCalendarView` 等日历视图。 |
| 考试详情与编辑 | 部分迁移 | 数据结构有时间段、地点、座位、清单和复盘字段，但 `ExamsPage` 不提供编辑和详情页。 |
| 考试清单 | 部分迁移 | `ExamChecklistItem` 在 Core 模型中存在，桌面 UI 未提供勾选与维护入口。 |
| 关联错题 | 未迁移 | Swift `RelatedMistakeCard` / `LinkedMistakesListView` 没有桌面对应。 |
| 考试复盘 / Exam Autopsy | 未迁移 | Swift 有 `ExamAutopsyModels`、Repository、ViewModel 和页面；客户端 `features.rs` 没有该领域。 |
| 单科 / 综合考试分数预测 | 未迁移 | Swift 有 `ScorePredictionEngine`、单科/综合预测和讨论入口；桌面报告/趋势不是等价实现。 |
| 成绩图表 / 关注科目聚合 | 部分迁移 | 客户端提供趋势数值，但没有 Swift 端的 `GradeChartView`、可选科目图表和相同的预测交互。 |
| 考试准备 / 复习通知 | 平台差异，未迁移 | Swift 是本地通知调度；桌面端没有通知调度层。 |

**判断**：先补“考试详情 + 清单 + 关联错题 + 复盘”，再接入成绩预测。否则桌面端虽然能保存考试数据，但很难支撑考试前后的完整学习流程。

### 3. 任务、例程和 Phase：Core 兼容，前端没有闭环

客户端 Core 已经有 `StudyPhase`、`Routine`、`RoutineInstance`、任务提醒字段和例程存储；FFI 也暴露了例程相关方法。但 `frontend/src/lib/core.ts` 没有例程 CRUD bridge，当前导航和页面也没有 Phase/例程管理入口。

| Swift 能力 | 桌面端状态 | 说明 |
|---|---|---|
| 普通任务完整编辑 / 详情 | 部分迁移 | 客户端只有快速新增和完成切换，未提供 Swift 的详情编辑、备注、类型、截止时间等完整表单。 |
| 统一 Todo 聚合 | 部分迁移 | Swift 把考试、综合考试、任务和例程实例聚合为 Todo；客户端 Today/Tasks 分开读取。 |
| Routine 模板 | Core 已有、UI 未迁移 | `routines.jsonl` 与 Core CRUD 存在，但没有例程编辑 sheet。 |
| Routine 实例生成 | Core 已有、UI/调度未迁移 | Swift `RoutineSpawner` 的按日幂等物化和过期清理没有桌面调度器。 |
| Phase 创建、激活、归档、跨域过滤 | 部分迁移 | Core 有 `get_phases` 和 `phase_id` 字段，但当前 UI 未发现 Phase 选择器、管理页或跨资源过滤。 |
| 任务同步到系统日历/提醒事项 | 平台差异，未迁移 | Swift 使用 EventKit；桌面端没有等价系统集成。 |

**判断**：这是一个适合先在桌面端做纯本地替代的模块：Phase/例程/统一 Todo 不依赖 Apple 框架，应优先补齐；日历/提醒事项则另做桌面 OS 适配。

### 4. Time Investment：已有数据层，用户流程仍然缺失

客户端已经有 `time_investment_subjects.jsonl`、`time_investment_subtasks.jsonl`、`goal_rewards.jsonl`、`InvestmentTarget`、`TimeInvestmentSummary` 和 FFI 方法，但当前 `InvestmentPage` 只做项目名称/主题的新增和删除。

Swift 端提供的是独立项目体系，最多三级层级，计时目标绑定、手动记录、跨午夜切分、直接/后代汇总、连续打卡、奖励解锁和未分配旧会话处理。

尚未搬过来的具体能力：

- 子任务树的新增、编辑、归档、删除和循环校验。
- 计时开始时选择项目/子任务目标，并记住上次目标。
- 手动补录学习时间、编辑开始日期。
- 计时会话与目标绑定后的直接投入/总投入汇总，避免子任务重复计数。
- 跨午夜会话拆分、时区保存和长期历史统计。
- 全局连续学习天数、昨日宽限规则和奖励阈值计算。
- 奖励解锁的永久状态、奖励庆祝和奖励配置页。
- 旧会话未分配 inbox。

**判断**：这是当前最明确的“Core 已铺路但 UI 未完成”模块，适合单独做一个完整 vertical slice，而不是继续扩大项目 CRUD。

### 5. Diary / Trends / Flashcard 的剩余细节

这三个 P1 模块已经完成基础闭环，但还没有完全达到 Swift 端的交互深度。

| 模块 | 已有 | 仍缺 |
|---|---|---|
| Diary | 多条日记、心情、精力、标签、Markdown、历史 | 日历总览、按日心情/精力色块、独立编辑页体验、设置页提醒时间、原生晚间提醒。 |
| Trends | 90 天活动点、学习时长、心情/精力、科目趋势、SRS 概览 | GitHub 风格热力图的完整交互、掌握曲线、更多图表类型、Phase 过滤、与学习建议/时间投入的联动。 |
| Flashcard | 文字错题卡、翻面、Again/Hard/Good/Easy、进度和总结 | 手写答题卡、计算器辅助、更多复习会话细节，以及 Swift 端更丰富的卡片内容编辑。 |

### 6. Health / Readiness / Brain Usage / Memory Climate / Habit Insight

这些是 Swift 端非常有辨识度的一组功能，桌面端当前没有对应专用实现：

- HealthKit HRV（SDNN）、14/30 天基线、Body Status、睡眠/呼吸/心率/锻炼等多维健康数据。
- `StudyReadinessAlgorithm` 的准备度等级、学习强度和重点建议。
- Brain Usage 的五小时/七天配额、动态配额和相关通知。
- Memory Climate 的当前状态、历史和详情。
- Habit Insight 的 90 天学习时段分析、最佳学习窗口和 AI 洞察。
- 上述数据驱动的 Home 卡片、Coach 刷新和通知联动。

这组功能不能简单按 Swift 文件逐个复制：HealthKit 在 Windows 不存在，macOS 也需要另行评估授权与数据来源。建议拆成两层：

1. 先迁移不依赖健康设备的纯函数能力，例如学习时段洞察、记忆气候的本地推导和可解释的准备度输入模型。
2. 再决定 macOS/Windows 是否接入平台健康数据，或提供手动输入/外部导入，而不是让桌面端假装拥有 HealthKit 数据。

### 7. 成就、连续打卡、虚拟植物和 Theme Shop

| Swift 能力 | 桌面端状态 | 说明 |
|---|---|---|
| 连续学习天数 | 部分迁移 | Rust analytics 有基础 `current_streak` 和 Today/Trends 展示；没有 Swift 的成就事件体系。 |
| Achievement Catalog / 每日目标 / 解锁记录 | Core 预留不足、UI 未迁移 | 客户端初始化 `achievements.json`，但未发现对应 `AchievementCatalog`、事件管理器和成就页。 |
| 虚拟植物八阶段状态机 | 未迁移 | 客户端只初始化 `plant_state.json`，没有对应模型、推导服务、管理器和 Canvas 页面。 |
| 主题商店 | 未迁移 | 客户端只有 light/dark 和 Anthropic/Apple 两种界面语言；没有按成就解锁的主色、卡片皮肤、计时器动画目录。 |
| 主页卡片排序/启用设置 | 未迁移 | Swift 有 17 类 Home card 和布局偏好；桌面端 Today 是固定布局。 |

### 8. Swift 专项 AI 能力

本节已单独整理为：[SWIFT_AI_FEATURE_PARITY.md](SWIFT_AI_FEATURE_PARITY.md)。下面保留摘要，便于从总清单直接看到 AI 缺口。

桌面端有更强的通用 Agent，因此“能问问题”已经存在；但 Swift 端的多个 AI caller 是带固定上下文、结构化输出、字段回写和专用 UI 的产品功能，当前没有等价迁移：

- Home Ask：根据成绩、错题、趋势、准备度自动选择上下文。
- Study Suggestions / Daily Plan：基于当天数据生成可解释的建议。
- Body Radar：健康维度雷达与建议。
- Mistake AI Analysis：错因、正确思路和相似题建议的结构化回写。
- AI Quiz、AI Similar Question、Auto Mind Map、Mistake Debate。
- Score Prediction / Comprehensive Score Prediction 的专用 prompt、解析器和讨论入口。
- Weekly Report 的 AI 总结和周/月自动生成。
- LLM Response Cache、caller 级调试信息和失败时的上次成功输出兜底。

其中 Coach、Reverse Planner、Exam Simulator 已有桌面专用流程，不应重复列为完全缺失；但 Coach 的健康变化触发、后台刷新、通知和更丰富的 Swift 上下文仍属于部分迁移。

## Apple 平台能力：需要做“桌面替代”而非直接搬运

### 通知、日历和提醒事项

Swift 当前有以下调度器或系统连接：

- `SRSReviewNotifications`：到期错题复习。
- `ExamReviewNotifications` / `ExamPrepareNotifications`：考试准备和复盘。
- `DiaryReminderNotifications`：日记提醒。
- `DailyGoalReminder`：每日目标提醒。
- `HabitInsightNotifications`：最佳学习窗口提醒。
- `CoachNotifications`：Coach 目标提醒。
- `CalendarManager`：EventKit 日历和 Reminders。

桌面端没有对应通知服务。建议先设计跨平台的 `NotificationScheduler` 抽象，再分别实现 macOS UserNotifications、Windows Toast 或纯应用内提醒；不要把 Swift 的 EventKit 字段当成已完成的桌面同步。

### Widgets、Live Activity、App Intents / Shortcuts

未发现桌面端对应实现：

- Exam Widget：即将到来的考试。
- Trend Widget：科目趋势。
- HRV Widget：准备度。
- Study Timer Live Activity：锁屏/Dynamic Island 计时。
- Routine Live Activity：例程执行状态。
- App Intents：添加成绩、记录错题、查看考试、查看身体状态、查看准备度、查看科目平均分、打开 Coach。

这些能力是 iOS 生态扩展，不应作为桌面核心学习闭环的阻塞项。桌面端可以另行规划菜单栏小组件、系统托盘、快捷键和通知中心入口。

## 设置、资料、备份和启动流程的差距

### Profile / Onboarding

Swift 有版本化欢迎流程、六页基础资料表单、用户画像、教育体系配置、默认科目和头像；桌面端目前主要是“创建/打开 Workspace”的 Welcome 页面，以及设置中的语言、主题、界面风格和 AI provider。

尚缺：

- 用户昵称、年级/年龄、教育体系和画像信息。
- 默认科目选择与科目展示名称配置。
- Phase 管理、当前 Phase 切换和归档。
- 头像、个人资料和启动后的 profile 编辑。
- 基于版本的 onboarding/welcome 重现策略。

### Settings / Data Management

Swift 设置页还包括健康、周/月报告、LLM 高级参数、数据管理、CSV 导出、恢复示例数据、日志导出、Phase 管理、成就、每日目标、图表类型、主页布局、Theme Shop、FAQ、用户协议和贡献说明。

桌面端 `SettingsPage` 当前主要覆盖：

- light/dark。
- Anthropic/Apple 界面风格。
- 五语言切换。
- Cloud AI 登录、BYOK 保存/断开。
- 隐私说明。

因此设置页是明显的产品深度缺口，而不是单纯视觉差异。

### Backup / Export

桌面端 Core 的备份基础设施已经比较完整，且会保留未知 `Data` 文件、媒体和 schema 兼容性；但与 Swift 端相比仍应核对：

- 桌面 UI 目前主要走 Replace，Merge 和逐冲突解决没有完整暴露。
- Swift 端备份还覆盖 SwiftData 实体、UserDefaults、App Group 数据、成就、健康历史等；桌面端 `profile.json`、`plant_state.json`、`achievements.json`、`preferences.json` 目前更多是 Workspace 初始化文件，未见完整业务读写链路。
- Swift 有 CSV 导出和错题 PDF/报告 PNG 文档包装；桌面端目前是报告文件/资产导出。
- 恢复前后的 recovery point、导入冲突展示和恢复失败回滚应在 UI 上继续做验收，而不能只以 Core 测试通过代替用户流程完成。

## 数据层与前端层的“已铺路但没用起来”清单

这部分容易被误判为“已迁移”，因为客户端代码里已经出现兼容类型或文件：

| 已存在的桌面代码 | 仍缺的用户能力 |
|---|---|
| `HeartRateSample`、`DifficultyAnnotation` | 心率采集/展示、学习会话标注编辑、历史详情页。 |
| `HandwritingAnswerEntry`、手写历史字段 | 手写输入控件、图片/画布存储和回看。 |
| `audio_file_name`、`Media/images`、`Media/audio` | 选择、上传到 Workspace、预览、播放和删除。 |
| `Routine` / `RoutineInstance` 的 Workspace/FFI 方法 | 前端 bridge、例程编辑、按日生成和完成交互。 |
| `SubTask`、`GoalReward`、`InvestmentTarget`、汇总 DTO | Time Investment 完整页面和计时目标选择。 |
| `get_phases`、各种 `phase_id` | Phase 管理、激活、归档和全局筛选。 |
| `plant_state.json`、`achievements.json` 初始化文件 | Plant/Achievement 模型、事件入口、状态推导和页面。 |
| `includes_derived_health_data` 备份选项 | 实际健康数据来源和健康历史存储；参数存在不等于功能存在。 |
| `ExamChecklistItem`、`ExamReview` 字段 | 考试详情、清单勾选和复盘 UI。 |

## 建议的迁移顺序

### P0：补齐可跨平台的学习闭环

1. **错题详情工作台**：完整编辑四块内容、搜索/筛选、媒体关联、错题详情、SRS 入队。
2. **考试详情与复盘**：日历、详情编辑、清单、关联错题、考试复盘；随后接成绩预测。
3. **Phase + Routine + 统一 Todo**：先实现本地调度和过滤，不依赖 Apple 日历。
4. **Time Investment 完整闭环**：子任务树、目标绑定、手动补录、汇总、连续天数和奖励。
5. **日报/趋势/闪卡细节**：日历型 Diary、热力图、Phase 过滤、手写/计算器等桌面适配。

### P1：补齐产品深度

6. **专项错题 AI**：AI 解析、相似题、AI 自测；模型输出继续落到结构化本地记录。
7. **成就与激励**：Achievement Catalog、每日目标、事件入口、解锁状态；再决定是否做虚拟植物和 Theme Shop。
8. **Profile/Onboarding/Settings**：教育体系、资料、主页布局、图表、数据管理、报告配置。
9. **备份 UI 完整化**：Merge、冲突解决、恢复结果、Recovery Point 和跨版本验收。
10. **通知抽象**：先做平台无关调度模型和应用内提醒，再接 macOS/Windows 原生通知。

### P2：平台能力和高级 AI

11. macOS/Windows 的菜单栏/托盘/通知中心替代 Widget/Live Activity。
12. Shortcuts/快捷键/命令面板替代 App Intents。
13. 专项健康数据接入或手动健康数据导入，明确隐私和权限边界。
14. Home Ask、习惯洞察、记忆气候、周报 AI、LLM cache/调试信息等专项 AI caller。

## 不应算作“没搬过来”的桌面端新增能力

以下能力在 Swift 参考端没有同等实现，属于客户端自己的架构和产品增量：

- Rust Workspace：路径安全、原子写入、JSONL envelope、未知字段保留、备份校验和 recovery staging。
- Agent runtime：六种能力模式、事件时间线、权限确认、输入等待、取消、工具目录和代码执行后端。
- Library/Notebook：资料导入、搜索、来源范围和本地对话历史。
- Cloud AI + BYOK provider 抽象，以及通过系统安全凭据存储隔离 token/API key。
- Tauri + React 的跨平台桌面壳，以及 Anthropic/Apple 两套可切换的界面语言。

这些能力应继续保留，不需要为了追求 Swift 文件名一一对应而回退架构。

## 证据索引

### Swift 参考端

- 总体架构与功能清单：`/Users/chenkaigao/Documents/Program/Swift/StudyPulse/AGENTS.md`。
- 根导航与跨进程入口：`StudyPulse/Views/ContentView.swift`、`StudyPulse/StudyPulseApp.swift`。
- 错题工作台：`StudyPulse/Views/Mistake/`、`StudyPulse/Managers/LLM/MistakeAIAnalysisLLM.swift`、`StudyPulse/Models/AIQuizModels.swift`。
- 考试工作流：`StudyPulse/Views/Exam/`、`StudyPulse/Views/ExamReversePlanner/`、`StudyPulse/Views/ExamSimulation/`、`StudyPulse/Services/ExamReadinessPrediction.swift`。
- Time Investment：`StudyPulse/Models/TimeInvestment.swift`、`StudyPulse/Services/TimeInvestmentEngine.swift`、`StudyPulse/Views/TimeInvestment/`。
- Health / Brain / Habit / Memory：`StudyPulse/Managers/Health/`、`StudyPulse/Managers/Study/`、`StudyPulse/Models/{HealthHistory,BrainUsage,HabitInsight,MemoryClimate}.swift`。
- 成就 / 植物 / Theme Shop：`StudyPulse/Managers/{Achievement,Plant}/`、`StudyPulse/Models/{Achievements,PlantState,ThemeShop}.swift`。
- Apple 系统集成：`StudyPulse/Intents/`、`StudyPulse/NotificationsControl/`、`StudyPulse/Managers/Widget/`、`StudyPulse/ActivityAttributes/`。

### 桌面端

- 页面与导航：`frontend/src/app/App.tsx`、`frontend/src/app/P1Pages.tsx`、`frontend/src/app/P2Pages.tsx`。
- Tauri bridge：`frontend/src/lib/core.ts`、`frontend/src/types.ts`。
- 业务模型：`core/crates/studypulse-workspace/src/models.rs`、`features.rs`、`analytics.rs`。
- 持久化与备份：`core/crates/studypulse-workspace/src/workspace.rs`、`backup.rs`。
- FFI / Host 注册：`core/crates/studypulse-ffi/src/lib.rs`、`src-tauri/src/lib.rs`。

## 工作区说明

本次只读取和比较了两个仓库，没有修改 Swift 参考端，也没有清理任一工作区已有的未跟踪文件。比较结论针对 2026-08-08 的当前源码；分支、未提交改动、发布状态和历史 PR 不作为“已迁移”的证据。
