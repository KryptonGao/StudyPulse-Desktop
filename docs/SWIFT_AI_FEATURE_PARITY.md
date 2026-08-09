# StudyPulse Swift → StudyPulseClient AI 功能迁移清单

> 对比日期：2026-08-08
>
> 参考端：`/Users/chenkaigao/Documents/Program/Swift/StudyPulse`
>
> 桌面端：`/Users/chenkaigao/Documents/Program/StudyPulseClient`

## 总结

桌面端已经具备一套比 Swift 端更通用的 Agent 架构：Provider-neutral `ModelClient`、Cloud AI/BYOK、Agent mode、工具权限确认、事件时间线、Notebook、资料库和代码执行。但这不等于 Swift 端的专项 AI 功能都已迁移。

Swift 端的 AI 功能大多是“固定业务上下文 → 结构化输出 → 回写本地模型 → 专用 UI”的闭环；桌面端目前主要有三种状态：

- **已迁移**：Coach、Reverse Planner、Exam Simulator 有独立页面和本地持久化。
- **部分迁移**：Coach 的核心流程已迁移，但健康驱动刷新、后台刷新、通知和更丰富的数据上下文没有迁移。
- **未迁移**：错题、主页、准备度、预测、思维导图、自测题等 Swift 专项 AI caller 尚未在桌面端形成对应闭环。

## 已迁移的桌面 AI 能力

| 能力 | 状态 | 当前实现 |
|---|---|---|
| Cloud AI / BYOK | 已迁移 | Tauri 宿主负责登录、深链和系统安全凭据；BYOK 使用 OpenAI-compatible endpoint。API key 不进入 Workspace、localStorage 或前端返回值。 |
| 通用 Agent | 已迁移 | `studypulse-agent` 提供 Chat、DeepSolve、Mastery、DeepResearch、QuestionLab、Visualize 等模式，以及阶段、循环上限、取消、输入等待和权限确认。 |
| Agent 资料上下文 | 已迁移 | Notebook 可选择 Library 来源，Agent 工具受来源范围、路径安全和文件大小限制。 |
| AI Coach | 基本迁移 | `P2Pages.tsx` 支持目标、科目权重、分析、风险、Proposal、批准/拒绝和对话，并保存本地 Coach 数据。 |
| Reverse Planner | 基本迁移 | 使用成绩、错题、SRS 到期数量和开放任务生成考试目标、弱点、阶段和每日任务。 |
| Exam Simulator | 基本迁移 | 生成 10 道题、20 分钟计时、答题记录、自动保存/恢复、评分和行为分析。 |
| 报告导出 | 部分迁移 | 报告统计本身是本地 Core 计算；桌面端可导出 Markdown/HTML/PNG，并打印/PDF。Swift 端的周报 AI 总结和自动生成设置尚未迁移。 |

## Swift 专项 AI 功能差距

### 1. 主页与学习建议

| Swift 能力 | 状态 | 缺口 |
|---|---|---|
| Home Ask | 未迁移 | Swift 根据用户问题自动选择成绩、错题、趋势或准备度上下文，再调用专用 prompt；桌面 Agent 可泛化回答，但没有这个上下文路由和 Home Ask UI。 |
| Study Suggestions | 未迁移 | Swift 有固定的学习建议模型、优先级和本地数据上下文；桌面 Today 只有 Core suggestions 展示，没有对应的专项 LLM caller。 |
| Daily Plan AI | 未迁移 | Swift 的每日计划会综合考试、SRS、例程和任务；桌面端没有相同的专项建议生成和计划解释链路。 |
| Habit Insight AI | 未迁移 | Swift 先用 90 天会话计算学习窗口，再可调用 AI 优化洞察；桌面端没有对应模型、缓存和通知。 |

### 2. 错题 AI 工作台

这是当前最大的可移植 AI 缺口。桌面端已有错题基础 CRUD、文字 SRS 和通用 Agent，但没有 Swift 端以错题字段为中心的专用流程。

| Swift 能力 | 状态 | 缺口 |
|---|---|---|
| 错题 AI 解析 | 未迁移 | `MistakeAIAnalysisLLM` / `MistakeAIAnalysisSheet` 的错因分析、正确思路、相似题建议和结构化回写未实现。 |
| AI 相似题 | 未迁移 | 没有生成题组、答题、批改和结果回写流程。 |
| AI 自测题 | 未迁移 | 没有选择题/填空题模型、答题界面、批改和结果页面。 |
| 自动思维导图 | 未迁移 | 没有结构化节点 JSON、树形渲染和错题上下文生成。 |
| 错题辩论 / 深入探讨 | 未迁移 | 没有以错解为上下文的多轮反思对话，也没有专用会话状态。 |
| 错题图片识别 | 未迁移 | Swift 有 `MistakeImageRecognitionLLM` 和图像附件；桌面端当前没有图片选择、附件编码、识别和回写 UI。 |
| OCR + AI 组合 | 未迁移 | Swift 使用 Vision OCR 后进入错题/AI 分析；桌面端没有 OCR 入口。 |
| 知识断层线 / 修复任务 | 未迁移 | `KnowledgeFaultLineAIProvider`、断层线分析和修复任务服务没有桌面端等价实现。 |

### 3. 考试、分数与复盘 AI

| Swift 能力 | 状态 | 缺口 |
|---|---|---|
| 单科分数预测 | 未迁移 | Swift 有 `ScorePredictionEngine`、专用预测 prompt、置信度/讨论入口；桌面趋势和报告不是等价功能。 |
| 综合考试分数预测 | 未迁移 | 没有综合科目预测和综合结果讨论流程。 |
| Exam Readiness | 未迁移 | Swift 有考试准备度与预测相关模型；桌面端只有 Reverse Planner 和 Exam Simulator，未发现独立 readiness caller。 |
| Exam Autopsy | 未迁移 | Swift 有 `ExamAutopsyLLM`、模型、Repository、ViewModel 和页面；桌面 `features.rs` 没有该领域。 |
| Exam Role 分析 | 基本迁移 | Exam Simulator 已有角色分析字段和模型输出；需继续核对 Swift 端的策略解释、稳定性展示和失败兜底是否完全一致。 |

### 4. Health / Brain / Memory 相关 AI

| Swift 能力 | 状态 | 缺口 |
|---|---|---|
| Body Radar AI | 未迁移 | Swift 的 `BodyRadarLLM` 根据 HRV、身体状态和学习数据生成雷达建议；桌面端没有 HealthKit 输入或对应 prompt。 |
| Study Readiness AI | 未迁移 | Swift 将 HRV、睡眠、心率、呼吸、锻炼等信号合成为准备度建议；桌面端没有同等输入源。 |
| Brain Usage Quota AI | 未迁移 | Swift `BrainUsageQuotaLLM` 用于动态配额；桌面端没有 Brain Usage 模型或调度。 |
| Memory Climate | 未迁移 | Swift 有本地 Memory Climate 计算、历史和详情；桌面端没有对应模型、趋势或 AI 增强。 |
| Health-driven Coach refresh | 未迁移 | Swift 在健康状态显著变化后刷新 Coach；桌面 Coach 只由页面操作/Agent 调用触发。 |

这组功能需要先解决数据源问题。HealthKit 不能直接搬到 Windows；桌面端应在“手动输入、外部导入、macOS 健康数据或明确不支持”之间做产品选择，而不是生成虚假的健康数据。

## AI 基础设施差距

### Provider 与运行时

两端的总体目标不同：Swift 是以 caller 为中心的单体 LLM 服务，桌面端是 Agent runtime + provider-neutral ModelClient。

| Swift 机制 | 桌面端情况 | 判断 |
|---|---|---|
| `LLMConfig` / `LLMClient` | `CloudModelClient` / `OpenAICompatibleModelClient` | 已有等价 provider 层，但配置和错误分类模型不同。 |
| URLSession JSON/SSE | Rust async ModelClient/SSE | 已有等价流式通道。 |
| caller-specific `LLMRequestBuilder` | Agent mode + 页面内拼接 prompt | 部分迁移；桌面尚未建立统一的专项 caller/prompt 层。 |
| `LLMResponseParser` 固定标题解析 | Coach/Planner/Simulator 使用结构化 JSON | 部分迁移；不同功能的 schema 约束还没有统一成可复用契约。 |
| `LLMResponseCache` actor LRU/TTL | 未发现相同的 caller 级缓存 | 未迁移；重复请求、离线兜底和成本控制能力不足。 |
| 图像附件 | Swift 有 `LLMImageAttachment` | 桌面 Agent/ModelClient 当前没有对应的图像输入闭环。 |
| DEBUG caller 级调用记录 | Agent 有事件时间线 | 部分迁移；Agent 事件不是 Swift 的 caller、prompt、耗时、缓存命中调试记录。 |
| 失败时使用上次成功结果 | 未发现统一等价机制 | 未迁移；专项 AI 需要明确是否允许 stale result 兜底。 |

### 结构化输出与安全边界

桌面端当前 P2 页面会把模型输出当作 JSON，再做有限的字段归一化；Core 负责验证和持久化。这条边界是正确方向，但后续专项 AI 不能继续把大段 prompt 和 JSON 转换散落在 React 页面里。

建议统一增加：

- 每个 AI caller 的输入 DTO、输出 DTO、版本号和 schema 校验。
- 统一的 `ModelOutputError`：JSON 无效、字段缺失、模型拒绝、超时、配额耗尽和网络失败分开处理。
- 把 prompt 构建和响应解析放在 Rust feature/service 层，React 只负责表单、进度和结果展示。
- 结构化输出保存原始 provider 元数据，但不保存 API key、token 或未脱敏请求头。
- 对图像、错题内容和 Workspace 资料设置大小上限、来源范围和明确的用户确认提示。
- 对 AI 生成的任务、计划和知识修复动作继续使用“预览 → 用户确认 → 写入”的流程，不允许模型直接执行写操作。

## 四阶段迁移路线

### Phase 1：AI 基础层与安全闭环

目标：把当前散落在 React 页面里的模型调用收敛为可复用、可验证、可恢复的桌面 AI 基础设施。

1. 建立统一 `AiFeature`/caller 层，定义输入 DTO、输出 DTO、schema 版本、错误类型和事件。
2. 把 prompt 构建、结构化输出解析和字段归一化从 `P2Pages.tsx` 下沉到 Rust feature/service 层。
3. 增加 `LLMResponseCache` 等价能力、caller 级耗时/缓存命中/失败调试记录，以及超时后的 stale result 策略。
4. 建立图像、错题内容和 Workspace 资料的大小限制、来源范围和隐私边界。
5. 所有 AI 生成的任务、计划和知识修复动作统一遵循“预览 → 用户确认 → 写入”，模型不能直接执行写操作。

交付标准：Coach、Reverse Planner、Exam Simulator 继续可用；新增 caller 不再直接在 React 中拼接未经校验的 JSON。

### Phase 2：错题 AI 工作台

目标：先完成最有学习价值、且不依赖 HealthKit 的错题 AI 闭环。

1. 迁移错题 AI 解析：错因、正确思路、相似题建议，支持结构化预览和回写。
2. 迁移 AI 相似题：生成题组、答题、批改、结果保存。
3. 迁移 AI 自测题：选择题/填空题模型、答题状态、批改和结果页。
4. 迁移图片附件、OCR 和错题图片识别，打通“图片/文本 → 错题 → AI 分析”。
5. 迁移自动思维导图、错题辩论和知识断层线/修复任务。

交付标准：形成“错题详情 → AI 处理 → 用户审核 → 本地记录/SRS/任务”的完整链路，并复用 Phase 1 的 schema、缓存和权限边界。

### Phase 3：学习与考试 AI

目标：把 AI 从单条记录扩展到日常学习决策和考试复盘。

1. 迁移 Home Ask，上下文路由覆盖成绩、错题、趋势和可用的准备度数据。
2. 迁移 Study Suggestions / Daily Plan，输出可解释的今日建议，而不是只返回一段自由文本。
3. 迁移单科/综合考试分数预测，包括置信度、弱点证据和讨论入口。
4. 迁移 Exam Autopsy，关联考试结果、错题、行为记录和后续修复任务。
5. 加强现有 Coach、Reverse Planner、Exam Simulator 的历史上下文、失败恢复、任务执行结果和对话连续性。

交付标准：学习建议、考试预测和复盘结果都能落到本地模型，并能由用户确认后转成任务、计划或复习动作。

### Phase 4：健康上下文与平台化 AI

目标：在明确桌面端数据来源后，再引入 Swift 端依赖健康和后台系统的 AI 能力。

1. 先实现不依赖 HealthKit 的 Habit Insight / Memory Climate 本地计算和历史趋势。
2. 明确 macOS/Windows 的健康输入方案：手动输入、外部导入、平台健康数据，或明确不支持；禁止生成虚假的健康数据。
3. 在真实健康数据可用后，再实现 Body Radar、Study Readiness 和 Brain Usage Quota。
4. 增加健康变化触发的 Coach refresh、后台任务和通知调度。
5. 为 macOS/Windows 适配通知中心、托盘/菜单栏和快捷入口；iOS Widget/Live Activity 不作为桌面 AI 核心依赖。

交付标准：健康权限、数据来源、隐私存储、后台刷新和失败降级均有明确边界；无健康数据时仍能完整使用本地学习和前三个 Phase 的 AI 功能。

## 证据索引

### Swift 参考端

- AI 基础设施：`StudyPulse/Managers/LLM/LLMClient.swift`、`LLMConfig.swift`、`LLMRequestBuilder.swift`、`LLMResponseParser.swift`、`LLMResponseCache.swift`。
- 主页与学习建议：`StudyPulse/Managers/LLM/HomeAskDataProvider.swift`、`StudyPulse/Managers/LLM/BrainUsageQuotaLLM.swift`、`StudyPulse/Managers/Study/HabitInsightNotifications.swift`。
- 错题 AI：`StudyPulse/Managers/LLM/MistakeImageRecognitionLLM.swift`、`AutoMindMapLLM.swift`、`Views/Mistake/LLM/`、`Views/Mistake/MistakeDebateSheet.swift`。
- 考试 AI：`StudyPulse/Managers/LLM/ExamAutopsyLLM.swift`、`ExamReadinessLLM.swift`、`ExamReversePlannerLLM.swift`、`ExamRoleLLM.swift`、`Services/ExamReadinessPrediction.swift`。
- 健康与身体建议：`StudyPulse/Managers/Health/`、`Views/Components/BodyRadarChart.swift`、`HRVStatusSuggestionSection.swift`。

### 桌面端

- Agent runtime：`core/crates/studypulse-agent/src/lib.rs`。
- Provider：`core/crates/studypulse-model-client/src/lib.rs`、`src-tauri/src/lib.rs`。
- P2 页面：`frontend/src/app/P2Pages.tsx`。
- AI 数据与报告：`core/crates/studypulse-workspace/src/features.rs`、`frontend/src/lib/core.ts`、`frontend/src/types.ts`。
- AI 设置与安全边界：`frontend/src/app/App.tsx`、`src-tauri/src/lib.rs`。
