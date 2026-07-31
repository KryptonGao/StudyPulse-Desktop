# StudyPulse Desktop 相比 iOS 的功能缺口

对比日期：2026-07-31  
对比对象：

- Desktop：`/Users/chenkaigao/Documents/Program/StudyPulse Desktop`
- iOS：`/Users/chenkaigao/Documents/Program/Swift/StudyPulse`

## 结论

Desktop 当前更像“Workspace + 通用学习 Agent”的 MVP，而不是 iOS StudyPulse 的 macOS 功能平移版。Desktop 已经具备 Workspace、Cloud AI Agent、Notebook、文字资料库、基础任务列表和 iOS 备份导入，但 iOS 的主要学习数据域与学习闭环尚未在 Desktop 上提供可用的页面和操作。

这里的“缺少”按以下标准判断：iOS 中存在明确的用户流程，而 Desktop 没有对应的用户入口、读写接口或可完成的操作。仅仅在备份格式或 Rust 数据结构中保留字段，不视为功能已经实现。

## 缺口总表

| 优先级 | 功能域 | Desktop 状态 | iOS 已有能力 | Desktop 需要补充 |
|---|---|---|---|---|
| P0 | Today / Dashboard | **缺少**；Today 仍是占位页 | 今日计划、统计卡、趋势、即将考试、学习建议、热力图、快捷入口 | 聚合今日任务、考试、学习时长、错题复习和建议的首页 |
| P0 | 成绩、科目与阶段 | **缺少** | 成绩新增/编辑/删除、科目管理、分数趋势、阶段筛选 | `Grade`、`Subject`、`StudyPhase` 的 Desktop 数据域、Repository、UI |
| P0 | 考试管理 | **缺少** | 考试/综合考试、列表/月历、详情编辑、清单、复盘、成绩预测、错题关联 | 考试 CRUD、月历、预测、复盘和与任务/错题的关联 |
| P0 | 错题本 | **缺少** | 错题集、详情编辑、搜索/标签、图片、OCR、手写、录音、PDF、错题模式分析 | 错题数据模型、富内容存储、错题列表/详情及维护操作 |
| P0 | 待办完整生命周期 | **部分已有**；只能展示/筛选任务，主要由 Agent 创建 | 作业、阅读、考试、例程的新增、编辑、完成、删除、历史、日历、阶段筛选、提醒同步 | Desktop 任务新增/编辑/完成/删除、例程、考试项、日历和提醒 |
| P0 | 学习计时 | **缺少** | Pomodoro、自定义时长、暂停/完成、会话复盘、强度/难度标注、心率记录 | macOS 计时器、学习会话持久化、历史和复盘 |
| P0 | Time Investment | **缺少** | 科目/子项目学习投入、手动记录、累计时长、连续学习、奖励 | 学习投入层级、手动记时、统计、奖励目标 |
| P1 | 学习日记与情绪 | **缺少** | 每日心情/精力、文字日记、日历、心情趋势、日记设置 | 日记实体、编辑/删除、日历和情绪趋势 |
| P1 | Trends / 学习分析 | **缺少** | 90 天热力图、科目成绩卡、排名/分数模式、需关注科目、掌握度雷达、学习时长摘要 | 趋势数据聚合、Charts/热力图、科目详情和筛选 |
| P1 | 闪卡与间隔重复 | **缺少** | 基于错题的到期队列、SM-2 复习、标签队列、手写作答、复习总结、计算器 | 复习队列、评分、间隔重复状态和复习会话 |
| P1 | AI Coach / 领域 AI | **部分已有但不等价** | 目标、预测分析、教练计划、提案审核、教练历史、独立对话、基于成绩/错题/考试/健康数据的建议 | 将 Agent 接入 iOS 领域数据；补目标、计划、分析、提案和历史模型 |
| P1 | AI 考场人格模拟器 | **缺少** | 角色化考试模拟及行为记录 | 模拟会话、角色配置、结果和行为记录 |
| P1 | AI Exam Reverse Planner | **缺少** | 从考试目标反推科目计划、阶段目标和任务 | 目标编辑、计划生成、计划结果和任务落地 |
| P1 | 周报/月报与分享 | **缺少** | 周/月学习报告、图表、AI 总结、PNG/JPEG 导出分享、自定义时间范围 | 报告聚合、渲染、导出和分享 |
| P1 | Health / Recovery | **缺少** | Apple Health HRV、Recovery Radar、身体状态、学习准备度、心率数据 | macOS 可用的数据源或明确的跨设备同步方案，以及恢复评分 UI |
| P2 | 成就、连续学习、植物与主题 | **缺少** | Streak、Achievements、Plant、奖励、主题商店、卡片皮肤、计时动画 | 成就规则、连续学习、植物状态、奖励和主题系统 |
| P2 | Profile / Onboarding / Settings | **缺少** | 首次资料填写、个人资料/头像、外观、语言、首页布局、图表、健康、报告、LLM、数据管理、FAQ | macOS 设置页、偏好持久化、用户资料和 onboarding |
| P2 | 数据管理与导出 | **部分已有**；Desktop 只有 iOS 备份检查/导入 | 完整备份导出、验证、Replace/Merge 恢复、成绩/错题/考试/任务 CSV 导入导出、日志导出、缓存管理、数据管理 | Desktop 备份导出、CSV 导入导出、验证工具、日志/缓存管理 |
| P2 | 系统集成 | **缺少** | UserNotifications、Calendar/Reminders、WidgetKit、Live Activity、App Intents、通知点击导航 | 对应 macOS 通知、日历/提醒、Widget 和快捷入口；或明确声明不做平台等价 |
| P2 | 多语言 | **缺少/未见实现**；Desktop UI 文案为英文硬编码 | 英文、简体中文、繁体中文、日文、韩文本地化资源 | macOS 本地化资源和语言偏好 |

## 关键差异说明

### 1. Desktop 的 Today 还没有形成首页功能

Desktop 的导航已经声明了 `Today`，但实际渲染的是 `ContentUnavailableView("Today is coming soon")`，并明确提示当前 MVP 只使用 Agent、Tasks 和 Library。对应代码：

- Desktop：`macOS/StudyPulseMac/Views/RootView.swift:168-186`
- iOS：`StudyPulse/Views/Home/HomeView.swift:37-90`、`StudyPulse/Models/HomeLayoutPreference.swift:15-104`

iOS 首页不是单一统计页，而是一个可配置的学习控制台，至少包含每日计划、最低可行计划、计时器、Recovery Radar、闪卡、记忆气候、趋势、即将考试、成绩、热力图、日记、脑力使用、AI 提问和两个考试 AI 功能入口。

### 2. Desktop 的任务是“只读展示 + Agent 创建”，不是完整 Todo

Desktop `TasksView` 只有类型筛选、完成项显示开关和刷新；任务行没有完成切换、编辑、删除或新增入口。Core 的 Tool Registry 也只提供 `get_tasks` 和需要确认的 `create_task`。

- Desktop UI：`macOS/StudyPulseMac/Views/TasksView.swift:17-52`
- Desktop Agent tools：`core/crates/studypulse-tools/src/lib.rs:224-233`
- iOS Todo：`StudyPulse/Views/Todo/TodoView.swift:114-185`、`StudyPulse/Views/Todo/TodoView.swift:558-595`

iOS Todo 还把考试、作业、阅读、例程合并到同一套列表/日历，并支持阶段筛选、过去项目、提醒完成态同步、详情页和维护操作。这些都尚未迁移。

### 3. Desktop 有通用 Agent，但没有 iOS 的领域数据 Agent

Desktop Agent 的能力集中在 Notebook 资料、Workspace 文件、Agent memory、网页/论文搜索、隔离代码执行、生成 artifact 和任务读写。当前 Tool Registry 没有成绩、错题、考试、日记、学习会话、健康状态或教练目标的查询工具。

iOS 的 Coach 和 Home Ask 则直接围绕成绩、错题、考试、趋势、学习时间和身体状态生成分析，并保存目标、会话、分析和提案。因此，Desktop 的 `Agent` 不能直接视为已经实现了 iOS 的 `Coach`。

- Desktop tools：`core/crates/studypulse-tools/src/lib.rs:172-233`
- iOS Coach：`StudyPulse/Views/Coach/CoachView.swift:96-185`
- iOS Home Ask：`StudyPulse/Views/LLM/HomeAskSheet.swift:26-70`、`StudyPulse/Views/LLM/HomeAskSheet.swift:140-154`

### 4. 备份兼容性不等于功能兼容性

Desktop 有 `Import iOS Backup`，并支持 Merge/Replace 与冲突解决；Core 数据结构也保留了 `MistakeNote`、`Exam` 等 iOS 记录的兼容字段。但 Desktop 当前对外的 Core/Swift 接口主要是 `getTasks`、Library、Notebook、Agent 和备份操作，没有对应的错题/考试/成绩查询与编辑接口。

- Desktop 备份界面：`macOS/StudyPulseMac/Views/BackupImportSheet.swift:4-12`、`macOS/StudyPulseMac/Views/BackupImportSheet.swift:27-77`
- Desktop Swift Core service：`macOS/StudyPulseMac/Services/CoreService.swift:34-50`
- Desktop FFI 对外查询接口：`core/crates/studypulse-ffi/src/lib.rs:503-574`
- Desktop 兼容数据结构：`core/crates/studypulse-workspace/src/models.rs:151-191`
- iOS 完整备份：`StudyPulse/Views/Settings/BackupRestoreView.swift:22-84`

所以，导入后的错题/考试等数据目前更接近“被保存在 Workspace 中”，而不是“用户可以在 Desktop 中继续使用这些功能”。

### 5. iOS 的学习计时和投入统计是独立的数据闭环

iOS 的 Study Timer 不只是一个倒计时器：它会保存学习会话、在完成后进行心率/难度复盘，并与 Time Investment 的科目、子项目、奖励和统计关联。Desktop 目前没有计时器、学习会话或投入统计页面。

- iOS Timer：`StudyPulse/Views/StudyTimer/StudyTimerView.swift:18-161`
- iOS Time Investment：`StudyPulse/Views/TimeInvestment/TimeInvestmentView.swift:23-127`

### 6. iOS 的错题功能是最明显的产品级缺口

iOS 错题详情页提供快速复习、AI 解析、深入讨论、同类题、错题辩论、知识深度和纠错计划；详情编辑还支持标签、图片/相机、OCR、录音和手写。结合 `FlashcardStudyView` 后，它还形成了从记录错题到间隔复习的闭环。

- iOS 错题详情：`StudyPulse/Views/Mistake/MistakeSetDetailView.swift:33-110`、`StudyPulse/Views/Mistake/MistakeSetDetailView.swift:168-201`
- iOS 错题编辑：`StudyPulse/Views/Mistake/MistakeDetailEditView.swift:204-387`
- iOS 闪卡：`StudyPulse/Views/Flashcard/FlashcardStudyView.swift:65-168`

Desktop 当前没有错题入口，也没有相应的读写 Tool 或 FFI API。

## 已有能力（不应重复建设）

以下能力 Desktop 已经具备，报告没有把它们列为缺口：

- Workspace 创建、打开、关闭、安全作用域访问和恢复上次 Workspace。
- Notebook 创建、删除、重命名、资料源选择和多轮 Agent 对话。
- Cloud AI 登录、Keychain 凭据保存、刷新和退出。
- Agent 的 Chat、Deep Solve、Mastery、Deep Research、Question Lab、Visualize 六种模式。
- Agent 事件流、阶段进度、取消、一次性写入确认和用户输入暂停。
- Documents/Notes 文字资料导入、文件列表和全文搜索。
- 基础任务读取、作业/阅读筛选、完成项显示和刷新。
- iOS 备份检查、冲突预览、Merge/Replace 导入。

这些能力与 iOS 的对应功能并不完全同构；尤其是 Desktop Agent/Library 是 Workspace 产品模型，iOS 则是 SwiftData 学习数据模型。

## 建议实现顺序

1. **P0：先补数据闭环**：`Subject/Phase/Grade`、`Exam`、`Mistake`、任务完整 CRUD、Study Timer/StudySession。
2. **P0/P1：再补首页和分析**：Today、Trends、Diary、Flashcard/SRS、Time Investment。
3. **P1：补 AI 领域能力**：让 Agent 能安全读取并写入成绩、错题、考试、任务和学习会话，再实现 Coach、Exam Simulator、Reverse Planner。
4. **P1/P2：补产品化能力**：周报/月报、分享、备份导出、CSV、设置、用户资料、语言和主题。
5. **P2：处理平台差异**：通知、提醒、Widget、Live Activity、HealthKit 的 macOS 等价方案；如果不做，应在产品范围中明确排除。

## 结论边界

本报告是基于两个当前工作树的源码静态对比，不代表 Desktop 已通过完整编译、运行或 UI 验收。两个仓库在本次检查时都存在未提交改动；报告按当前文件内容判断，没有清理、回退或覆盖这些改动。
