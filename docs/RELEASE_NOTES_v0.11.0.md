#### StudyPulse Desktop v0.11.0

> 将 StudyPulse 从一组学习功能，整理成一个更完整、更像桌面应用的本地优先学习工作台。

**版本号：** `0.11.0`  
**更新范围：** `v0.10.1` → `v0.11.0`  
**支持平台：** macOS（Apple Silicon）、Windows（x86_64）

#### 版本亮点

- 全新桌面应用壳：原生窗口材质、透明标题栏、侧栏导航和工作区分组。
- 学习、复习、洞察、资料库和 Agent 页面统一到一套可持续扩展的交互系统中。
- Notebook、Agent 事件流、确认、用户输入、恢复和 Markdown/数学公式展示更加完整。
- 主题、字号、颜色、语言、备份和 AI provider 设置集中到新的 Settings 页面。

#### 桌面体验重构

##### 原生窗口壳

- macOS 使用 Sidebar vibrancy、透明窗口和 Overlay title bar，让应用更贴近系统桌面质感。
- 集成标题栏拖拽区域与窗口控制能力，保留 Tauri 桌面窗口的原生行为。
- 重新组织主窗口、侧栏、顶部栏和内容区域，减少页面之间的视觉跳变。

##### 工作区导航

- 将页面归并为 Today、Agent、Study、Review、Insights 和 Library 六个工作区。
- 新增侧栏折叠、顶部面包屑、工作区页签和统一的页面跳转逻辑。
- 新增 Quick Switcher，可通过 `⌘K` 搜索页面并快速跳转。
- 统一任务行、按钮、空状态、加载状态、错误卡片、确认对话框和 Toast 反馈。

#### 学习工作流完整化

##### Today 与 Study

- Today 集中展示未完成任务、学习时长、连续学习、待复习错题和近期考试。
- Study 工作区整理任务、科目与成绩、单科考试、综合考试和学习计时。
- Today 提供进入 Agent 的快速提问入口，减少从概览到行动的切换成本。

##### Review 与 Insights

- 错题工作台支持展开题目、编辑内容、删除记录、查看复习选项和直接进入 SRS 复习。
- Diary、Trends 和 Flashcards 继续沿用本地 Workspace 数据，并接入统一的保存、确认和反馈交互。
- Trends 展示学习热力图、学习时长、SRS 摘要、科目成绩趋势和需要关注的科目。
- Insights 新增时间投入页面，用于维护科目级学习投入目标和统计。

##### Library 与 Settings

- Library 支持导入文本/Markdown 资料、搜索资料，并将选中的来源作为 Notebook 上下文。
- Settings 集中管理外观、语言、AI provider、Workspace 路径以及备份导入/导出。
- 备份操作和破坏性操作使用统一的确认流程，结果通过 Toast 反馈。

#### Agent 与 Notebook 工作流

##### 持久化与恢复

- Agent 页面拆分为 Notebook 列表、对话区和上下文区，支持创建对话、选择来源和查看历史消息。
- Agent Turn 保留所属 Notebook 身份；启动和恢复时能回到对应的 Notebook 与上下文。
- 可恢复的检查点在恢复后会被标记为已消费，避免同一个旧检查点反复出现在恢复提示中。

##### 可见的运行过程

- 事件时间线展示阶段、状态、文本增量、工具请求、工具完成、确认、用户输入、产物、错误和最终状态。
- 写入、破坏性操作和代码执行继续通过确认流程；`ask_user` 输入会在界面中显示提示、选项和回答区域。
- Agent 运行结果可以展示来源、Artifact、Token 用量、结构化题目集和可视化内容。
- 本机 Python 代码以可复制的代码块展示，并明确提示本机执行不是安全沙箱。

#### Markdown 与数学公式

- Agent 消息支持经过清理的 Markdown 与 GitHub Flavored Markdown 展示。
- 闪卡题目、错题原因、错误解法和正确解法支持 Markdown 与 KaTeX 数学公式。
- 增加对常见 LaTeX 输入差异的兼容处理，包括 `cases` 公式和 `\\para` 等常见写法。
- 补齐 Agent 用户输入事件的解析与渲染，兼容不同宿主版本产生的嵌套 payload。

#### 主题、外观与本地化

- Settings 支持 OpenAI Neutral、Ocean Breeze 和 Violet Night 三套主题预设。
- 支持 Light/Dark 模式、90%～120% 字号比例，以及浅色/深色模式下的自定义强调色、背景色和文字色。
- OpenAI 主题调整为白色画布、近黑色文字与操作色，并保留克制的语义色彩。
- 新增的工作区、导航、Agent、设置和反馈文案继续覆盖 English、简体中文、繁體中文、日本語和 한국어。

#### 工程与版本同步

- 应用版本统一升级至 `0.11.0`，同步 `package.json`、锁文件、Rust Core、Tauri 宿主和 Tauri 配置。
- 引入 KaTeX、`remark-math`、`rehype-katex` 与 `window-vibrancy`，支持公式渲染和 macOS 原生材质。
- 增加窗口拖拽与窗口控制所需的 Tauri capability 配置。
- 补充或调整导航、主题、Agent 事件解析和页面交互相关测试。

#### 兼容性与边界

- 本次版本没有改变 Workspace 数据 schema，已有本地 Workspace 可继续使用。
- AI 仍是可选连接；未配置 Cloud AI 或 BYOK 时，本地学习记录、复习、趋势、资料库和备份功能仍可使用。
- 本机 Python 执行仍不是安全沙箱；运行不可信代码时应改用隔离的 Docker Runner。
