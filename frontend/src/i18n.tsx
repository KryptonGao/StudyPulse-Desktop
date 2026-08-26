/* eslint-disable react-refresh/only-export-components */
import { createContext, useContext, useMemo, useState, type ReactNode } from "react";

export type Language = "zh-CN" | "zh-TW" | "en" | "ja" | "ko";
export type Translate = (key: string, variables?: Record<string, string | number>) => string;

// Language preference is app-local UI state. It is persisted separately from
// workspace records so opening another workspace does not change the language.
const LANGUAGE_STORAGE_KEY = "studypulse.language";

// The option order is also the stable order used by the Settings select; keep
// values aligned with the dictionary keys and browser-language detection.
export const languageOptions: { value: Language; label: string }[] = [
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
];

function detectLanguage(): Language {
  // Detection prefers an explicit local choice, then maps browser locale
  // prefixes. Server/Node rendering has no browser and safely defaults to en.
  if (typeof window === "undefined") return "en";
  const saved = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
  if (languageOptions.some((option) => option.value === saved)) return saved as Language;
  const browserLanguage = navigator.language.toLowerCase();
  if (browserLanguage.startsWith("zh-tw") || browserLanguage.startsWith("zh-hk") || browserLanguage.startsWith("zh-mo")) return "zh-TW";
  if (browserLanguage.startsWith("zh")) return "zh-CN";
  if (browserLanguage.startsWith("ja")) return "ja";
  if (browserLanguage.startsWith("ko")) return "ko";
  return "en";
}

// `en` is the fallback dictionary and also documents the complete base key
// space. Feature dictionaries below are merged into it by language.
const en: Record<string, string> = {
  "nav.today": "Today", "nav.agent": "Agent", "nav.tasks": "Tasks", "nav.subjects": "Subjects & Grades", "nav.exams": "Exams", "nav.mistakes": "Mistakes", "nav.timer": "Study Timer", "nav.investment": "Time Investment", "nav.library": "Library", "nav.settings": "Settings", "nav.aria": "StudyPulse sections",
  "brand.localWorkspace": "Local-first workspace", "workspace.default": "Workspace", "local.stored": "Stored on this Mac",
  "top.learningRhythm": "Your learning rhythm", "top.greeting": "Good to see you.", "top.importBackup": "Import backup", "top.exportBackup": "Export backup",
  "loading.opening": "Opening your local workspace…", "loading.failed": "StudyPulse could not start", "common.retry": "Retry", "error.generic": "Something went wrong.", "error.section": "Could not load this section",
  "welcome.eyebrow": "A modern local-first learning workbench", "welcome.title": "Your workspace, right on your Mac.", "welcome.copy": "StudyPulse keeps your data in a local Workspace. Open an existing one or create a new place for tasks, notes, sessions and Agent work.", "welcome.create": "Create workspace", "welcome.open": "Open workspace", "welcome.localTitle": "Local-first by default", "welcome.localCopy": "Cloud AI is optional. Your Workspace stays on this Mac.",
  "backup.conflicts": "{count} conflicts need review.", "backup.ready": "Backup schema {schema} is ready. {records} new records.", "backup.applyReplace": "Apply Replace restore now?", "backup.exported": "Backup exported successfully.",
  "today.greeting": "Today Overview", "today.quickAsk": "Ask anything about your study materials…", "today.quickAskButton": "Send", "today.heroTitle": "Today", "today.heroEmphasis": "", "today.heroCopy": "View your open tasks, reviews, and study rhythm.", "today.openTasks": "Open tasks", "today.nothingUrgent": "Nothing urgent", "today.studyTime": "Study time", "today.streak": "{count}-day streak", "today.dueMistakes": "Due mistakes", "today.readyReview": "Ready for review", "today.upcomingExams": "Upcoming exams", "today.noExamsSoon": "No exams soon", "today.nextTitle": "Tasks", "today.nextDescription": "Focus on what matters today.", "today.general": "General", "today.due": "Due {date}", "today.high": "High", "today.focus": "Focus", "today.clearTitle": "No tasks yet", "today.clearCopy": "Create a task above to start planning.", "today.noteTitle": "Today's Notes", "today.noteDescription": "A gentle space to capture insights.", "today.quote": "A clear desk and focused mind.",
  "tasks.title": "Tasks", "tasks.description": "A manageable queue for the work that matters.", "tasks.addPlaceholder": "Add a task…", "tasks.saving": "Saving…", "common.add": "Add", "tasks.noTasks": "No tasks yet", "tasks.noTasksCopy": "Add one small next step above.", "tasks.validation": "Please enter a task title.", "tasks.markComplete": "Mark complete", "tasks.markIncomplete": "Mark incomplete", "tasks.delete": "Delete task", "tasks.deleteConfirm": "Delete \"{title}\"? This cannot be undone.", "tasks.priorityLabel": "Priority {count}",
  "subjects.title": "Subjects & Grades", "subjects.description": "Keep the shape of your academic progress visible.", "subjects.subjectsTitle": "Subjects", "subjects.subjectsDescription": "A stable vocabulary for your study data.", "subjects.newSubject": "New subject", "subjects.fullScore": "Full score", "subjects.addSubject": "Add subject", "subjects.active": "Active", "subjects.off": "Off", "subjects.none": "No subjects yet", "subjects.noneCopy": "Add the first subject to start tracking grades.", "subjects.gradesTitle": "Grades", "subjects.gradesDescription": "Capture a result without leaving the local workspace.", "subjects.examOptional": "Exam name (optional)", "subjects.score": "Score", "subjects.addGrade": "Add grade", "subjects.grade": "Grade", "subjects.noneGrades": "No grades yet", "subjects.noneGradesCopy": "Record a score to make progress visible.", "subjects.validationName": "Please enter a subject name.", "subjects.validationGrade": "Please enter a subject for this grade.",
  "exams.title": "Exams", "exams.description": "Upcoming checkpoints, without the noise.", "exams.name": "Exam name", "exams.subject": "Subject", "exams.importance": "Importance", "exams.add": "Add", "exams.remove": "Remove", "exams.none": "No exams yet", "exams.noneCopy": "Add the next checkpoint above.", "exams.validationName": "Please enter an exam name.", "exams.validationDate": "Please choose an exam date.",
  "investment.title": "Time Investment", "investment.description": "See where your attention is actually going.", "investment.newSubject": "New investment subject", "investment.theme": "Theme", "investment.started": "Started", "investment.add": "Add", "investment.remove": "Remove", "investment.none": "No investment subjects yet", "investment.noneCopy": "Create a subject to connect timer blocks to a direction.", "investment.validation": "Please enter an investment subject name.", "theme.ocean": "Ocean", "theme.coral": "Coral", "theme.violet": "Violet", "theme.sunshine": "Sunshine", "theme.mint": "Mint",
  "mistakes.title": "Mistakes", "mistakes.description": "Review the edges where understanding can become stronger.", "mistakes.add": "+ Add mistake", "mistakes.close": "Close", "mistakes.titlePlaceholder": "Title", "mistakes.subjectPlaceholder": "Subject", "mistakes.questionPlaceholder": "What was the question?", "mistakes.reasonPlaceholder": "Why did it go wrong?", "mistakes.save": "Save mistake", "mistakes.again": "Again", "mistakes.hard": "Hard", "mistakes.gotIt": "Got it", "mistakes.mastery": "mastery", "mistakes.untitled": "Untitled mistake", "mistakes.noQuestion": "No question text recorded.", "mistakes.none": "No mistakes recorded", "mistakes.noneCopy": "Add mistakes to track points that need review.", "mistakes.validation": "Please enter a mistake title.",
  "timer.title": "Study Timer", "timer.description": "Give one block of attention a clear beginning and end.", "timer.minutes": "Minutes", "timer.intensity": "Intensity", "timer.start": "Start focus block", "timer.focus": "Focus", "timer.elapsed": "{duration} elapsed", "timer.pause": "Pause", "timer.resume": "Resume", "timer.finish": "Finish", "timer.cancel": "Cancel", "timer.idle": "Idle", "timer.running": "Running", "timer.paused": "Paused", "intensity.peak": "Peak", "intensity.deepFocus": "Deep focus", "intensity.steady": "Steady", "intensity.light": "Light", "intensity.recovery": "Recovery",
  "library.title": "Library", "library.description": "The local source material your Agent can work with.", "library.add": "Add sources", "library.searchPlaceholder": "Search your library…", "library.results": "Search results", "library.line": "Line {line}", "library.noMatches": "No matches yet.", "library.empty": "Your library is empty", "library.emptyCopy": "Add a text or Markdown source to give the Agent context.",
  "agent.notebooks": "Notebooks", "agent.sources": "{count} sources", "agent.messages": "{count} messages", "agent.noNotebook": "Create a notebook to keep a thread of work together.", "agent.newNotebook": "New notebook", "agent.untitledNotebook": "Untitled Notebook {count}", "agent.promptTitle": "What are you working through?", "agent.emptyTitle": "A capable second brain, with boundaries.", "agent.emptyCopy": "Ask about a source, turn a goal into steps, or work through a hard question. Every write or execution request will ask first.", "agent.noProvider": "Connect Cloud AI or configure BYOK in Settings first.", "agent.starting": "Starting…", "agent.completed": "Completed", "agent.permission": "Permission requested", "agent.toolFallback": "The Agent wants to use a tool.", "agent.deny": "Deny", "agent.allowOnce": "Allow once", "agent.inputRequired": "The Agent needs your input", "agent.inputFallback": "What should it know?", "agent.answerPlaceholder": "Your answer", "agent.send": "Send", "agent.askPlaceholder": "Ask the Agent anything about your learning…", "agent.moreLibrary": "+{count} more in Library", "agent.shortcut": "⌘ Enter to run", "agent.cancel": "Cancel", "agent.run": "Run Agent", "agent.context": "Context", "agent.timeline": "Timeline", "agent.timelineDescription": "Visible work, not hidden thought.", "agent.noEvents": "Agent events will appear here as work progresses.",
  "settings.title": "Settings", "settings.description": "Personalize and manage your StudyPulse workspace.", "settings.appearance": "Appearance", "settings.appearanceTitle": "Personalization", "settings.theme": "Theme", "settings.themeDescription": "Choose between light and dark workspace themes.", "settings.preset": "Color Palette", "settings.presetDescription": "Select a cohesive color scheme.", "settings.presetOpenai": "OpenAI Blue", "settings.presetOcean": "Ocean Blue", "settings.presetViolet": "Violet Plum", "settings.fontScale": "Typography Scale", "settings.fontScaleDescription": "Adjust the base UI text size.", "settings.scaleCompact": "Compact (90%)", "settings.scaleDefault": "Default (100%)", "settings.scaleLarge": "Large (110%)", "settings.scaleExtra": "Extra (120%)", "settings.customColors": "Advanced Color Customization", "settings.customColorsDescription": "Override accent, background, and text colors with custom hex codes.", "settings.accentColor": "Accent Color", "settings.backgroundColor": "Background Color", "settings.textColor": "Text Color", "settings.resetDefaults": "Reset Defaults", "settings.resetConfirm": "Reset all appearance settings to default values?", "settings.sidebarToggle": "Toggle Sidebar", "settings.language": "Language", "settings.languageDescription": "Choose the global display language for StudyPulse.", "settings.provider": "AI Provider", "settings.providerTitle": "AI Models & Services", "settings.providerDescription": "The local workspace works fully without AI. Connect to unlock intelligent assistance.", "settings.cloudConnected": "Cloud AI Connected", "settings.byokConnected": "BYOK Connected", "settings.notConnected": "Not Connected", "settings.cloudName": "StudyPulse Cloud AI", "settings.cloudCopy": "Sign in securely via browser. Tokens stay in macOS Keychain.", "settings.signIn": "Sign In", "settings.working": "Working…", "settings.byokName": "BYOK · OpenAI-Compatible", "settings.byokCopy": "Use your own API key and endpoint. Stored securely in macOS Keychain.", "settings.baseUrl": "Base URL", "settings.model": "Model", "settings.savedKey": "Saved key · leave blank to keep", "settings.apiKey": "API Key", "settings.saveByok": "Save BYOK", "settings.saving": "Saving…", "settings.disconnect": "Disconnect and remove saved credentials", "settings.workspaceActions": "Data & Backups", "settings.backupTitle": "Backup Archive", "settings.backupDescription": "Export a complete workspace snapshot or restore an existing backup.", "settings.privacy": "Privacy", "settings.privacyTitle": "Local-First Architecture", "settings.privacyCopy": "Workspace records, Agent runs and Notebook history stay in your local directory. StudyPulse does not expose a browser localhost service.", "language.label": "Language", "switcher.placeholder": "Search or quick jump…", "switcher.trigger": "Search or jump…", "common.saved": "Saved successfully", "common.confirm": "Confirm",
  "duration.hours": "{count}h", "duration.minutes": "{count}m", "theme.light": "Light", "theme.dark": "Dark", "taskType.homework": "Homework", "taskType.reading": "Reading", "event.started": "Started", "event.statusChanged": "Status changed", "event.textDelta": "Text delta", "event.toolRequested": "Tool requested", "event.toolCompleted": "Tool completed", "event.confirmationRequired": "Confirmation required", "event.stageStarted": "Stage started", "event.stageProgress": "Stage progress", "event.stageCompleted": "Stage completed", "event.inputRequired": "Input required", "event.artifactCreated": "Artifact created", "event.failed": "Failed", "event.cancelled": "Cancelled", "event.completed": "Completed", "mode.chat": "Chat", "mode.deepSolve": "Deep solve", "mode.mastery": "Mastery", "mode.deepResearch": "Deep research", "mode.questionLab": "Question lab", "mode.visualize": "Visualize",
  "dialog.openWorkspace": "Open StudyPulse Workspace", "dialog.createWorkspace": "Create StudyPulse Workspace", "dialog.addSources": "Add Notebook Sources", "dialog.inspectBackup": "Inspect StudyPulse Backup", "dialog.exportBackup": "Export StudyPulse Backup",
};

const zhCN: Record<string, string> = {
  "nav.today": "今日", "nav.agent": "智能助手", "nav.tasks": "任务", "nav.subjects": "科目与成绩", "nav.exams": "考试", "nav.mistakes": "错题", "nav.timer": "学习计时器", "nav.investment": "时间投入", "nav.library": "资料库", "nav.settings": "设置", "nav.aria": "StudyPulse 分区",
  "brand.localWorkspace": "本地工作区", "workspace.default": "工作区", "local.stored": "储存在这台 Mac 上", "top.learningRhythm": "学习节奏", "top.greeting": "你好", "top.importBackup": "导入备份", "top.exportBackup": "导出备份", "loading.opening": "正在打开本地工作区…", "loading.failed": "StudyPulse 无法启动", "common.retry": "重试", "error.generic": "出了点问题。", "error.section": "无法加载此分区",
  "welcome.eyebrow": "现代本地优先学习工作台", "welcome.title": "随时开启你的专属学习空间。", "welcome.copy": "StudyPulse 将数据完整保存在本地工作区。打开已有目录或创建新工作区，开始管理任务、错题、学习计时和智能助手对话。", "welcome.create": "创建工作区", "welcome.open": "打开工作区", "welcome.localTitle": "默认本地优先", "welcome.localCopy": "云端 AI 属于可选功能，你的全部学习数据始终保存在这台 Mac 上。", "backup.conflicts": "有 {count} 个冲突需要检查。", "backup.ready": "备份架构 {schema} 已就绪，包含 {records} 条新记录。", "backup.applyReplace": "现在应用替换恢复吗？", "backup.exported": "备份已成功导出。",
  "today.greeting": "今日概览", "today.quickAsk": "输入问题或指令，让 Agent 协助你学习...", "today.quickAskButton": "发送", "today.heroTitle": "今日学习", "today.heroEmphasis": "", "today.heroCopy": "在此查看今日待办、复习任务与学习节奏。", "today.openTasks": "未完成任务", "today.nothingUrgent": "暂无紧急事项", "today.studyTime": "学习时长", "today.streak": "连续 {count} 天", "today.dueMistakes": "待复习错题", "today.readyReview": "可以开始复习", "today.upcomingExams": "即将考试", "today.noExamsSoon": "近期没有考试", "today.nextTitle": "待办任务", "today.nextDescription": "按优先级推进今日的重要事项。", "today.general": "常规", "today.due": "截止 {date}", "today.high": "高优先级", "today.focus": "重点", "today.clearTitle": "暂无待办任务", "today.clearCopy": "点击上方添加任务开始规划。", "today.noteTitle": "今日学习心得", "today.noteDescription": "记录今日的复盘与思考。", "today.quote": "心境澄明，专注当下。",
  "tasks.title": "任务", "tasks.description": "为真正重要的事情保留清晰可控的待办清单。", "tasks.addPlaceholder": "添加任务…", "tasks.saving": "保存中…", "common.add": "添加", "tasks.noTasks": "还没有任务", "tasks.noTasksCopy": "在上方添加今日待办事项。", "tasks.validation": "请输入任务标题。", "tasks.markComplete": "标记为已完成", "tasks.markIncomplete": "标记为未完成", "tasks.delete": "删除任务", "tasks.deleteConfirm": "确定删除“{title}”？此操作无法撤销。", "tasks.priorityLabel": "优先级 {count}", "subjects.title": "科目与成绩", "subjects.description": "让学业进展的轮廓清晰可见。", "subjects.subjectsTitle": "科目", "subjects.subjectsDescription": "为学习数据建立稳定的科目体系。", "subjects.newSubject": "新科目", "subjects.fullScore": "满分", "subjects.addSubject": "添加科目", "subjects.active": "启用", "subjects.off": "关闭", "subjects.none": "还没有科目", "subjects.noneCopy": "添加第一个科目，开始记录成绩。", "subjects.gradesTitle": "成绩", "subjects.gradesDescription": "在本地工作区记录每次测验与考试结果。", "subjects.examOptional": "考试名称（可选）", "subjects.score": "得分", "subjects.addGrade": "添加成绩", "subjects.grade": "成绩", "subjects.noneGrades": "还没有成绩", "subjects.noneGradesCopy": "记录一次得分，让进步清晰可见。", "subjects.validationName": "请输入科目名称。", "subjects.validationGrade": "请输入这条成绩对应的科目。",
  "exams.title": "考试", "exams.description": "规划即将到来的考试节点与备考安排。", "exams.name": "考试名称", "exams.subject": "科目", "exams.importance": "重要程度", "exams.add": "添加", "exams.remove": "移除", "exams.none": "还没有考试", "exams.noneCopy": "在上方添加下一个考试节点。", "exams.validationName": "请输入考试名称。", "exams.validationDate": "请选择考试日期。", "investment.title": "时间投入", "investment.description": "追踪统计注意力与时间的实际分布。", "investment.newSubject": "新投入主题", "investment.theme": "主题", "investment.started": "开始于", "investment.add": "添加", "investment.remove": "移除", "investment.none": "还没有投入主题", "investment.noneCopy": "创建一个主题，将专注计时与目标建立关联。", "investment.validation": "请输入投入主题名称。", "theme.ocean": "海洋", "theme.coral": "珊瑚", "theme.violet": "紫罗兰", "theme.sunshine": "阳光", "theme.mint": "薄荷",
  "mistakes.title": "错题本", "mistakes.description": "系统化复盘薄弱知识点，强化掌握度。", "mistakes.add": "+ 添加错题", "mistakes.close": "关闭", "mistakes.titlePlaceholder": "标题", "mistakes.subjectPlaceholder": "科目", "mistakes.questionPlaceholder": "题目内容与考点", "mistakes.reasonPlaceholder": "错误原因与解析", "mistakes.save": "保存错题", "mistakes.again": "重来 (Again)", "mistakes.hard": "较难 (Hard)", "mistakes.gotIt": "掌握 (Good)", "mistakes.mastery": "掌握度", "mistakes.untitled": "未命名错题", "mistakes.noQuestion": "未记录题目内容。", "mistakes.none": "暂无错题记录", "mistakes.noneCopy": "录入做错的题目以建立 SRS 复习队列。", "mistakes.validation": "请输入错题标题。",
  "timer.title": "学习计时器", "timer.description": "为每次专注学习划定清晰的时间区块。", "timer.minutes": "分钟", "timer.intensity": "专注强度", "timer.start": "开始专注", "timer.focus": "专注", "timer.elapsed": "已用时 {duration}", "timer.pause": "暂停", "timer.resume": "继续", "timer.finish": "完成", "timer.cancel": "取消", "timer.idle": "空闲", "timer.running": "进行中", "timer.paused": "已暂停", "intensity.peak": "高峰", "intensity.deepFocus": "深度专注", "intensity.steady": "稳定", "intensity.light": "轻量", "intensity.recovery": "恢复",
  "library.title": "资料库", "library.description": "智能助手可以读取和分析的本地文档与资料。", "library.add": "添加资料", "library.searchPlaceholder": "搜索资料库…", "library.results": "搜索结果", "library.line": "第 {line} 行", "library.noMatches": "未找到匹配资料。", "library.empty": "资料库为空", "library.emptyCopy": "导入 Markdown 或文本文件，为智能助手提供上下文背景。",
  "agent.notebooks": "对话列表", "agent.sources": "{count} 份资料", "agent.messages": "{count} 条对话", "agent.noNotebook": "创建一个笔记本以开启新的对话线索。", "agent.newNotebook": "新建对话", "agent.untitledNotebook": "未命名对话 {count}", "agent.promptTitle": "今天想探讨或学习什么？", "agent.emptyTitle": "强大且有边界的学习助手", "agent.emptyCopy": "围绕资料提问、拆解学习目标，或推导复杂题解。任何文件写入或执行都会事先征得你的许可。", "agent.noProvider": "请先在设置中配置 AI 服务商或 OpenAI 兼容端点。", "agent.starting": "正在启动…", "agent.completed": "已完成", "agent.permission": "权限确认", "agent.toolFallback": "Agent 申请调用工具。", "agent.deny": "拒绝", "agent.allowOnce": "允许执行", "agent.inputRequired": "Agent 需要你的输入", "agent.inputFallback": "请输入所需信息：", "agent.answerPlaceholder": "输入回复内容...", "agent.send": "提交", "agent.askPlaceholder": "输入问题或指令，按 Enter 发送，Shift+Enter 换行…", "agent.moreLibrary": "资料库中还有 {count} 份", "agent.shortcut": "Enter 发送", "agent.cancel": "停止运行", "agent.run": "发送", "agent.context": "上下文", "agent.timeline": "执行时间线", "agent.timelineDescription": "透明展示 Agent 的工具调用与推理进程。", "agent.noEvents": "Agent 运行事件将按时间顺序展示在这里。",
  "settings.title": "设置", "settings.description": "个性化配置与管理你的 StudyPulse 工作区。", "settings.appearance": "外观", "settings.appearanceTitle": "个性化设置", "settings.theme": "主题模式", "settings.themeDescription": "选择浅色模式或深色模式。", "settings.preset": "配色方案", "settings.presetDescription": "选择适合专注与阅读的界面调色板。", "settings.presetOpenai": "OpenAI 蓝调", "settings.presetOcean": "晨曦远海", "settings.presetViolet": "暮光雅紫", "settings.fontScale": "界面字号", "settings.fontScaleDescription": "调整全局基础文字显示比例。", "settings.scaleCompact": "紧凑 (90%)", "settings.scaleDefault": "标准 (100%)", "settings.scaleLarge": "放大 (110%)", "settings.scaleExtra": "特大 (120%)", "settings.customColors": "高级色彩定制", "settings.customColorsDescription": "自定义主色调、背景与文本色彩 Hex 编码。", "settings.accentColor": "强调色", "settings.backgroundColor": "背景色", "settings.textColor": "文本色", "settings.resetDefaults": "恢复默认设置", "settings.resetConfirm": "确定要将外观设置重置为默认值吗？", "settings.sidebarToggle": "切换侧边栏", "settings.language": "界面语言", "settings.languageDescription": "选择全局显示语言。", "settings.provider": "智能助手服务", "settings.providerTitle": "AI 模型与服务商", "settings.providerDescription": "本地工作区无需 AI 即可完整使用，连接后可解锁智能辅助能力。", "settings.cloudConnected": "Cloud AI 已连接", "settings.byokConnected": "BYOK 已连接", "settings.notConnected": "未连接", "settings.cloudName": "StudyPulse Cloud AI", "settings.cloudCopy": "通过安全网页登录，授权令牌安全保存在 macOS 钥匙串中。", "settings.signIn": "登录", "settings.working": "处理中…", "settings.byokName": "自定义 OpenAI 兼容端点 (BYOK)", "settings.byokCopy": "使用你自有的 API 密钥与模型端点，密钥安全保存在本地钥匙串中。", "settings.baseUrl": "API 基础 URL", "settings.model": "模型名称", "settings.savedKey": "已有密钥 · 留空以保持不变", "settings.apiKey": "API 密钥", "settings.saveByok": "保存端点配置", "settings.saving": "保存中…", "settings.disconnect": "断开连接并移除已保存凭据", "settings.workspaceActions": "工作区与数据", "settings.backupTitle": "备份与归档", "settings.backupDescription": "导出完整工作区快照或恢复历史备份。", "settings.privacy": "隐私与安全", "settings.privacyTitle": "本地优先架构", "settings.privacyCopy": "学习记录、错题、Agent 对话与文档均保存在所选本地目录中，绝不对外暴露本地端口。", "language.label": "语言", "switcher.placeholder": "搜索或快速跳转...", "switcher.trigger": "搜索或快速跳转...", "common.saved": "设置已保存", "common.confirm": "确认",
  "duration.hours": "{count} 小时", "duration.minutes": "{count} 分钟", "theme.light": "浅色", "theme.dark": "深色", "taskType.homework": "作业", "taskType.reading": "阅读", "event.started": "已启动", "event.statusChanged": "状态更新", "event.textDelta": "生成中", "event.toolRequested": "请求调用工具", "event.toolCompleted": "工具调用完成", "event.confirmationRequired": "等待确认", "event.stageStarted": "阶段开始", "event.stageProgress": "阶段推进", "event.stageCompleted": "阶段完成", "event.inputRequired": "等待用户输入", "event.artifactCreated": "已生成产物", "event.failed": "执行失败", "event.cancelled": "已取消", "event.completed": "已完成", "mode.chat": "对话", "mode.deepSolve": "深度解题", "mode.mastery": "掌握训练", "mode.deepResearch": "深度研究", "mode.questionLab": "问题工坊", "mode.visualize": "数据可视化",
  "dialog.openWorkspace": "打开 StudyPulse 工作区", "dialog.createWorkspace": "创建 StudyPulse 工作区", "dialog.addSources": "添加资料文件", "dialog.inspectBackup": "检查备份文件", "dialog.exportBackup": "导出工作区备份",
};

const zhTW: Record<string, string> = {
  ...zhCN,
  "today.greeting": "今日概覽", "today.quickAsk": "輸入問題或指令，讓 Agent 協助你學習...", "today.quickAskButton": "發送",
  "tasks.markComplete": "標記為已完成", "tasks.markIncomplete": "標記為未完成", "tasks.delete": "刪除任務", "tasks.deleteConfirm": "確定刪除「{title}」？此操作無法復原。", "tasks.priorityLabel": "優先級 {count}",
};

const ja: Record<string, string> = {
  ...en,
  "nav.today": "今日", "nav.agent": "Agent", "nav.tasks": "タスク", "nav.subjects": "科目と成績", "nav.exams": "試験", "nav.mistakes": "間違い", "nav.timer": "学習タイマー", "nav.investment": "時間投資", "nav.library": "ライブラリ", "nav.settings": "設定", "nav.aria": "StudyPulse セクション", "brand.localWorkspace": "ローカル優先ワークスペース", "workspace.default": "ワークスペース", "local.stored": "この Mac に保存", "top.learningRhythm": "あなたの学習リズム", "top.greeting": "こんにちは", "top.importBackup": "バックアップを読み込む", "top.exportBackup": "バックアップを書き出す", "loading.opening": "ローカルワークスペースを開いています…", "loading.failed": "StudyPulse を起動できませんでした", "common.retry": "再試行", "error.generic": "問題が発生しました。", "error.section": "このセクションを読み込めませんでした",
  "today.greeting": "今日の概要", "today.quickAsk": "学習に関する質問を何でも聞いてください…", "today.quickAskButton": "送信", "today.heroTitle": "今日の学習", "today.heroEmphasis": "", "today.heroCopy": "未完了タスクと学習リズムを確認できます。", "today.openTasks": "未完了タスク", "today.nothingUrgent": "急ぎのものはありません", "today.studyTime": "学習時間", "today.streak": "{count}日連続", "today.dueMistakes": "復習する間違い", "today.readyReview": "復習の準備ができています", "today.upcomingExams": "今後の試験", "today.noExamsSoon": "近い試験はありません", "today.nextTitle": "タスク", "today.nextDescription": "今日大切なことに集中しましょう。", "today.clearTitle": "タスクはありません", "today.clearCopy": "タスクを追加して計画を始めましょう。", "today.noteTitle": "今日のメモ", "today.noteDescription": "今日の振り返りと気づきを記録します。", "today.quote": "澄んだ心で、今に集中。", "tasks.markComplete": "完了にする", "tasks.markIncomplete": "未完了に戻す", "tasks.delete": "タスクを削除", "tasks.deleteConfirm": "「{title}」を削除しますか？この操作は取り消せません。", "tasks.priorityLabel": "優先度 {count}",
  "settings.title": "設定", "settings.description": "StudyPulse ワークスペースの環境設定を管理します。", "settings.appearance": "外観", "settings.appearanceTitle": "個人設定", "settings.theme": "テーマ", "settings.themeDescription": "ライトモードまたはダークモードを選択します。", "settings.preset": "配色パターン", "settings.presetDescription": "調和の取れたカラーパレットを選択します。", "settings.fontScale": "文字サイズ", "settings.fontScaleDescription": "UI 全体の文字サイズを調整します。", "settings.provider": "AI プロバイダー", "settings.providerTitle": "AI モデルとサービス", "settings.providerDescription": "AI に接続しなくてもローカルワークスペースは完全に機能します。", "settings.cloudConnected": "Cloud AI 接続済み", "settings.byokConnected": "BYOK 接続済み", "settings.notConnected": "未接続", "settings.workspaceActions": "データとバックアップ", "settings.backupTitle": "バックアップと復元", "settings.backupDescription": "ワークスペース全体のバックアップや復元を行います。",
};

const ko: Record<string, string> = {
  ...en,
  "nav.today": "오늘", "nav.agent": "Agent", "nav.tasks": "할 일", "nav.subjects": "과목 및 성적", "nav.exams": "시험", "nav.mistakes": "오답", "nav.timer": "학습 타이머", "nav.investment": "시간 투자", "nav.library": "라이브러리", "nav.settings": "설정", "nav.aria": "StudyPulse 섹션", "brand.localWorkspace": "로컬 우선 워크스페이스", "workspace.default": "워크스페이스", "local.stored": "이 Mac에 저장됨", "top.learningRhythm": "학습 리듬", "top.greeting": "안녕하세요", "top.importBackup": "백업 가져오기", "top.exportBackup": "백업 내보내기", "loading.opening": "로컬 워크스페이스를 여는 중…", "loading.failed": "StudyPulse를 시작할 수 없습니다", "common.retry": "다시 시도", "error.generic": "문제가 발생했습니다.", "error.section": "이 섹션을 불러올 수 없습니다",
  "today.greeting": "오늘 개요", "today.quickAsk": "학습에 관해 질문하거나 지시를 입력하세요…", "today.quickAskButton": "전송", "today.heroTitle": "오늘의 학습", "today.heroEmphasis": "", "today.heroCopy": "오늘의 할 일과 복습, 학습 리듬을 확인하세요.", "today.openTasks": "미완료 할 일", "today.nothingUrgent": "급한 일이 없습니다", "today.studyTime": "학습 시간", "today.streak": "{count}일 연속", "today.dueMistakes": "복습할 오답", "today.readyReview": "복습할 준비가 됐어요", "today.upcomingExams": "다가오는 시험", "today.noExamsSoon": "가까운 시험이 없습니다", "today.nextTitle": "할 일", "today.nextDescription": "오늘 중요한 일에 집중하세요.", "today.clearTitle": "할 일이 없습니다", "today.clearCopy": "할 일을 추가해 계획을 시작하세요.", "today.noteTitle": "오늘의 메모", "today.noteDescription": "오늘의 생각과 정리를 기록합니다.", "today.quote": "맑은 마음으로 지금에 집중.", "tasks.markComplete": "완료로 표시", "tasks.markIncomplete": "미완료로 표시", "tasks.delete": "할 일 삭제", "tasks.deleteConfirm": "“{title}”을(를) 삭제할까요? 되돌릴 수 없습니다.", "tasks.priorityLabel": "우선순위 {count}",
  "settings.title": "설정", "settings.description": "StudyPulse 워크스페이스를 맞춤 설정합니다.", "settings.appearance": "모양", "settings.appearanceTitle": "개인 설정", "settings.theme": "테마", "settings.themeDescription": "라이트 또는 다크 모드를 선택하세요.", "settings.preset": "색상 구성표", "settings.presetDescription": "작업 공간에 어울리는 색상을 선택하세요.", "settings.fontScale": "글꼴 크기", "settings.fontScaleDescription": "기본 글꼴 크기 비율을 조정합니다.", "settings.provider": "AI 제공자", "settings.providerTitle": "AI 모델 및 서비스", "settings.providerDescription": "AI 연결 없이도 로컬 워크스페이스를 완전히 사용할 수 있습니다.", "settings.cloudConnected": "Cloud AI 연결됨", "settings.byokConnected": "BYOK 연결됨", "settings.notConnected": "연결 안 됨", "settings.workspaceActions": "데이터 및 백업", "settings.backupTitle": "백업 및 복원", "settings.backupDescription": "워크스페이스 백업을 내보내거나 복원합니다.",
};

const mistakeEditorTranslations: Record<Language, Record<string, string>> = {
  en: {
    "mistakes.edit": "Edit mistake", "mistakes.cancel": "Cancel", "mistakes.saveChanges": "Save changes", "mistakes.originalQuestion": "Question", "mistakes.errorReason": "Why it went wrong", "mistakes.wrongSolution": "Your wrong solution", "mistakes.correctSolution": "Correct solution", "mistakes.tags": "Tags", "mistakes.tagsPlaceholder": "e.g. sign error, factoring", "mistakes.aiAnalyze": "Analyze with AI", "mistakes.aiAnalyzing": "Analyzing…", "mistakes.aiDescription": "AI can suggest a diagnosis and corrected solution. Review it before saving.", "mistakes.aiProviderRequired": "Connect Cloud AI or BYOK in Settings before analyzing a mistake.", "mistakes.aiQuestionRequired": "Add the question text before analyzing.", "mistakes.aiDraft": "AI suggestions stay in this draft until you save.", "mistakes.aiEvidence": "Evidence", "mistakes.aiConfidence": "{count}% confidence", "mistakes.aiNoEvidence": "No evidence returned.", "mistakes.workbench": "Mistake AI workbench", "mistakes.aiBoundary": "AI output is a reviewable draft. Nothing is written without your action.", "mistakes.addImage": "Attach image", "mistakes.imagePreview": "Attached mistake", "mistakes.imageReady": "Image ready for AI", "mistakes.applyAndSave": "Apply and save", "mistakes.analysisHint": "Start with a diagnosis, then decide which fields to apply.", "mistakes.selectFirst": "Open or create a mistake first.", "mistakes.staleResult": "The provider returned a cached fallback. Review it carefully before applying.", "mistakes.operation.analysis": "Analysis", "mistakes.operation.questions": "Similar", "mistakes.operation.selfTest": "Self-test", "mistakes.operation.mindMap": "Mind map", "mistakes.operation.debate": "Debate", "mistakes.operation.faultLine": "Fault line", "mistakes.operation.image": "Image / OCR", "mistakes.multipleChoice": "Multiple choice", "mistakes.fillBlank": "Fill in", "mistakes.difficulty": "Level {count}", "mistakes.generateSimilar": "Generate similar questions", "mistakes.saveQuestionSet": "Save question set", "mistakes.similarHint": "Generate a new set that targets this misconception.", "mistakes.generateSelfTest": "Generate self-test", "mistakes.submitTest": "Submit answers", "mistakes.saveTestResult": "Save result", "mistakes.answerPlaceholder": "Your answer", "mistakes.selfTestHint": "Generate questions, answer them, and ask AI to grade the attempt.", "mistakes.generateMindMap": "Build mind map", "mistakes.saveMap": "Save map", "mistakes.mindMapHint": "Turn the mistake into a small concept map.", "mistakes.you": "You", "mistakes.tutor": "Tutor", "mistakes.debateHint": "Defend your reasoning one step at a time.", "mistakes.debatePlaceholder": "Explain or defend your step…", "mistakes.sendDebate": "Send", "mistakes.saveDebate": "Save debate", "mistakes.findFaultLine": "Find knowledge fault line", "mistakes.createRepairTasks": "Create repair tasks", "mistakes.faultLineHint": "Compare your recorded mistakes to find a repeated concept gap.", "mistakes.confirmRepairTasks": "Create {count} repair tasks in your local task list?", "mistakes.imageHint": "Attach a photo of a mistake. AI OCR and recognition remain previews until you insert or save them.", "mistakes.recognizeImage": "Recognize mistake", "mistakes.runOcr": "Run AI OCR", "mistakes.insertOcr": "Insert OCR text", "mistakes.ocrConfidence": "OCR confidence {count}%", "mistakes.imageTooLarge": "Choose an image smaller than 8 MiB.", "mistakes.savedSessions": "Saved AI sessions ({count})",
  },
  "zh-CN": {
    "mistakes.edit": "编辑错题", "mistakes.cancel": "取消", "mistakes.saveChanges": "保存修改", "mistakes.originalQuestion": "题目", "mistakes.errorReason": "出错原因", "mistakes.wrongSolution": "你的错误解法", "mistakes.correctSolution": "正确解法", "mistakes.tags": "标签", "mistakes.tagsPlaceholder": "例如：符号错误、因式分解", "mistakes.aiAnalyze": "用 AI 分析", "mistakes.aiAnalyzing": "分析中…", "mistakes.aiDescription": "AI 可以建议错因和正确解法。请在保存前检查内容。", "mistakes.aiProviderRequired": "请先在设置中连接 Cloud AI 或 BYOK，再分析错题。", "mistakes.aiQuestionRequired": "请先填写题目内容。", "mistakes.aiDraft": "AI 建议会留在草稿中，保存后才会写入。", "mistakes.aiEvidence": "依据", "mistakes.aiConfidence": "置信度 {count}%", "mistakes.aiNoEvidence": "没有返回依据。", "mistakes.workbench": "错题 AI 工作台", "mistakes.aiBoundary": "AI 输出只是待审核草稿，未经你的操作不会写入。", "mistakes.addImage": "附加图片", "mistakes.imagePreview": "已附加的错题图片", "mistakes.imageReady": "图片已准备好供 AI 使用", "mistakes.applyAndSave": "应用并保存", "mistakes.analysisHint": "先生成诊断，再决定要应用哪些字段。", "mistakes.selectFirst": "请先打开或创建一条错题。", "mistakes.staleResult": "服务商返回了缓存的旧结果，请仔细检查后再应用。", "mistakes.operation.analysis": "错因分析", "mistakes.operation.questions": "相似题", "mistakes.operation.selfTest": "AI 自测", "mistakes.operation.mindMap": "思维导图", "mistakes.operation.debate": "错题辩论", "mistakes.operation.faultLine": "知识断层", "mistakes.operation.image": "图片 / OCR", "mistakes.multipleChoice": "选择题", "mistakes.fillBlank": "填空题", "mistakes.difficulty": "难度 {count}", "mistakes.generateSimilar": "生成相似题", "mistakes.saveQuestionSet": "保存题组", "mistakes.similarHint": "生成针对这次认知误区的新题组。", "mistakes.generateSelfTest": "生成自测题", "mistakes.submitTest": "提交答案", "mistakes.saveTestResult": "保存结果", "mistakes.answerPlaceholder": "你的答案", "mistakes.selfTestHint": "生成题目、完成作答，再让 AI 批改。", "mistakes.generateMindMap": "生成思维导图", "mistakes.saveMap": "保存导图", "mistakes.mindMapHint": "把错题整理成一张小型知识图谱。", "mistakes.you": "你", "mistakes.tutor": "导师", "mistakes.debateHint": "一步一步为你的推理辩护。", "mistakes.debatePlaceholder": "解释或辩护你的步骤…", "mistakes.sendDebate": "发送", "mistakes.saveDebate": "保存辩论", "mistakes.findFaultLine": "查找知识断层", "mistakes.createRepairTasks": "创建修复任务", "mistakes.faultLineHint": "比较已有错题，找出反复出现的知识缺口。", "mistakes.confirmRepairTasks": "要把 {count} 个修复任务加入本地任务列表吗？", "mistakes.imageHint": "附加错题图片。AI OCR 和识别结果在插入或保存前都会保持为预览。", "mistakes.recognizeImage": "识别错题", "mistakes.runOcr": "运行 AI OCR", "mistakes.insertOcr": "插入 OCR 文本", "mistakes.ocrConfidence": "OCR 置信度 {count}%", "mistakes.imageTooLarge": "请选择小于 8 MiB 的图片。", "mistakes.savedSessions": "已保存 AI 会话（{count}）",
  },
  "zh-TW": {
    "mistakes.edit": "編輯錯題", "mistakes.cancel": "取消", "mistakes.saveChanges": "儲存修改", "mistakes.originalQuestion": "題目", "mistakes.errorReason": "出錯原因", "mistakes.wrongSolution": "你的錯誤解法", "mistakes.correctSolution": "正確解法", "mistakes.tags": "標籤", "mistakes.tagsPlaceholder": "例如：符號錯誤、因式分解", "mistakes.aiAnalyze": "用 AI 分析", "mistakes.aiAnalyzing": "分析中…", "mistakes.aiDescription": "AI 可以建議錯因和正確解法。請在儲存前檢查內容。", "mistakes.aiProviderRequired": "請先在設定中連接 Cloud AI 或 BYOK，再分析錯題。", "mistakes.aiQuestionRequired": "請先填寫題目內容。", "mistakes.aiDraft": "AI 建議會留在草稿中，儲存後才會寫入。", "mistakes.aiEvidence": "依據", "mistakes.aiConfidence": "信心度 {count}%", "mistakes.aiNoEvidence": "沒有返回依據。",
  },
  ja: {
    "mistakes.edit": "間違いを編集", "mistakes.cancel": "キャンセル", "mistakes.saveChanges": "変更を保存", "mistakes.originalQuestion": "問題", "mistakes.errorReason": "間違えた理由", "mistakes.wrongSolution": "あなたの誤った解法", "mistakes.correctSolution": "正しい解法", "mistakes.tags": "タグ", "mistakes.tagsPlaceholder": "例：符号ミス、因数分解", "mistakes.aiAnalyze": "AI で分析", "mistakes.aiAnalyzing": "分析中…", "mistakes.aiDescription": "AI が原因と正しい解法を提案します。保存前に確認してください。", "mistakes.aiProviderRequired": "間違いを分析する前に、設定で Cloud AI または BYOK を接続してください。", "mistakes.aiQuestionRequired": "分析する前に問題文を入力してください。", "mistakes.aiDraft": "AI の提案は保存するまでこの下書きに残ります。", "mistakes.aiEvidence": "根拠", "mistakes.aiConfidence": "信頼度 {count}%", "mistakes.aiNoEvidence": "根拠は返されませんでした。",
  },
  ko: {
    "mistakes.edit": "오답 편집", "mistakes.cancel": "취소", "mistakes.saveChanges": "변경 저장", "mistakes.originalQuestion": "문제", "mistakes.errorReason": "틀린 이유", "mistakes.wrongSolution": "나의 잘못된 풀이", "mistakes.correctSolution": "올바른 풀이", "mistakes.tags": "태그", "mistakes.tagsPlaceholder": "예: 부호 오류, 인수분해", "mistakes.aiAnalyze": "AI로 분석", "mistakes.aiAnalyzing": "분석 중…", "mistakes.aiDescription": "AI가 원인과 올바른 풀이를 제안합니다. 저장하기 전에 확인하세요.", "mistakes.aiProviderRequired": "오답을 분석하기 전에 설정에서 Cloud AI 또는 BYOK를 연결하세요.", "mistakes.aiQuestionRequired": "분석하기 전에 문제 내용을 입력하세요.", "mistakes.aiDraft": "AI 제안은 저장할 때까지 이 초안에만 남습니다.", "mistakes.aiEvidence": "근거", "mistakes.aiConfidence": "신뢰도 {count}%", "mistakes.aiNoEvidence": "반환된 근거가 없습니다.",
  },
};

const mistakeCardTranslations: Record<Language, Record<string, string>> = {
  en: {
    "mistakes.delete": "Delete mistake", "mistakes.deleteConfirm": "Delete \"{title}\"? This cannot be undone.", "mistakes.expand": "Expand question", "mistakes.collapse": "Collapse question", "mistakes.review": "Review", "mistakes.hideReview": "Hide review choices",
  },
  "zh-CN": {
    "mistakes.delete": "删除错题", "mistakes.deleteConfirm": "确定删除“{title}”？此操作无法撤销。", "mistakes.expand": "展开题目", "mistakes.collapse": "收起题目", "mistakes.review": "复习", "mistakes.hideReview": "收起复习选项",
  },
  "zh-TW": {
    "mistakes.delete": "刪除錯題", "mistakes.deleteConfirm": "確定刪除「{title}」？此操作無法復原。", "mistakes.expand": "展開題目", "mistakes.collapse": "收起題目", "mistakes.review": "複習", "mistakes.hideReview": "收起複習選項",
  },
  ja: {
    "mistakes.delete": "間違いを削除", "mistakes.deleteConfirm": "「{title}」を削除しますか？この操作は取り消せません。", "mistakes.expand": "問題を展開", "mistakes.collapse": "問題を折りたたむ", "mistakes.review": "復習", "mistakes.hideReview": "復習の選択肢を隠す",
  },
  ko: {
    "mistakes.delete": "오답 삭제", "mistakes.deleteConfirm": "“{title}”을(를) 삭제할까요? 되돌릴 수 없습니다.", "mistakes.expand": "문제 펼치기", "mistakes.collapse": "문제 접기", "mistakes.review": "복습", "mistakes.hideReview": "복습 선택지 숨기기",
  },
};

const mistakeSessionTranslations: Record<Language, Record<string, string>> = {
  en: { "mistakes.saveBeforeSession": "Save this mistake before storing an AI session." },
  "zh-CN": { "mistakes.saveBeforeSession": "请先保存错题，再保存 AI 会话。" },
  "zh-TW": { "mistakes.saveBeforeSession": "請先儲存錯題，再儲存 AI 會話。" },
  ja: { "mistakes.saveBeforeSession": "AI セッションを保存する前に、間違いを保存してください。" },
  ko: { "mistakes.saveBeforeSession": "AI 세션을 저장하기 전에 오답을 먼저 저장하세요." },
};

// The workbench is intentionally complete in every supported language; these
// bundles avoid the old P2 fallback gap for the three languages that previously
// inherited most feature labels from English.
const mistakeWorkbenchTranslations: Record<Language, Record<string, string>> = {
  en: {},
  "zh-CN": {},
  "zh-TW": {
    "mistakes.workbench": "錯題 AI 工作台", "mistakes.aiBoundary": "AI 輸出只是待審核草稿，未經你的操作不會寫入。", "mistakes.addImage": "附加圖片", "mistakes.imagePreview": "已附加的錯題圖片", "mistakes.imageReady": "圖片已準備好供 AI 使用", "mistakes.applyAndSave": "套用並儲存", "mistakes.analysisHint": "先產生診斷，再決定要套用哪些欄位。", "mistakes.selectFirst": "請先開啟或建立一筆錯題。", "mistakes.staleResult": "服務商回傳了快取的舊結果，請仔細檢查後再套用。", "mistakes.operation.analysis": "錯因分析", "mistakes.operation.questions": "相似題", "mistakes.operation.selfTest": "AI 自測", "mistakes.operation.mindMap": "心智圖", "mistakes.operation.debate": "錯題辯論", "mistakes.operation.faultLine": "知識斷層", "mistakes.operation.image": "圖片 / OCR", "mistakes.multipleChoice": "選擇題", "mistakes.fillBlank": "填空題", "mistakes.difficulty": "難度 {count}", "mistakes.generateSimilar": "產生相似題", "mistakes.saveQuestionSet": "儲存題組", "mistakes.similarHint": "產生針對這次認知誤區的新題組。", "mistakes.generateSelfTest": "產生自測題", "mistakes.submitTest": "提交答案", "mistakes.saveTestResult": "儲存結果", "mistakes.answerPlaceholder": "你的答案", "mistakes.selfTestHint": "產生題目、完成作答，再讓 AI 批改。", "mistakes.generateMindMap": "產生心智圖", "mistakes.saveMap": "儲存導圖", "mistakes.mindMapHint": "把錯題整理成一張小型知識圖譜。", "mistakes.you": "你", "mistakes.tutor": "導師", "mistakes.debateHint": "一步一步為你的推理辯護。", "mistakes.debatePlaceholder": "解釋或辯護你的步驟…", "mistakes.sendDebate": "傳送", "mistakes.saveDebate": "儲存辯論", "mistakes.findFaultLine": "尋找知識斷層", "mistakes.createRepairTasks": "建立修復任務", "mistakes.faultLineHint": "比較已有錯題，找出反覆出現的知識缺口。", "mistakes.confirmRepairTasks": "要把 {count} 個修復任務加入本機任務列表嗎？", "mistakes.imageHint": "附加錯題圖片。AI OCR 和辨識結果在插入或儲存前都會保持為預覽。", "mistakes.recognizeImage": "辨識錯題", "mistakes.runOcr": "執行 AI OCR", "mistakes.insertOcr": "插入 OCR 文字", "mistakes.ocrConfidence": "OCR 信心度 {count}%", "mistakes.imageTooLarge": "請選擇小於 8 MiB 的圖片。", "mistakes.savedSessions": "已儲存 AI 會話（{count}）",
  },
  ja: {
    "mistakes.workbench": "間違い AI ワークベンチ", "mistakes.aiBoundary": "AI の出力は確認用の下書きです。操作するまで保存されません。", "mistakes.addImage": "画像を添付", "mistakes.imagePreview": "添付した問題画像", "mistakes.imageReady": "AI 用の画像を準備しました", "mistakes.applyAndSave": "適用して保存", "mistakes.analysisHint": "診断を生成し、適用する項目を選びます。", "mistakes.selectFirst": "間違いを開くか作成してください。", "mistakes.staleResult": "プロバイダーがキャッシュされた結果を返しました。適用前に確認してください。", "mistakes.operation.analysis": "分析", "mistakes.operation.questions": "類題", "mistakes.operation.selfTest": "セルフテスト", "mistakes.operation.mindMap": "マインドマップ", "mistakes.operation.debate": "ディベート", "mistakes.operation.faultLine": "知識の断層", "mistakes.operation.image": "画像 / OCR", "mistakes.multipleChoice": "選択式", "mistakes.fillBlank": "穴埋め", "mistakes.difficulty": "レベル {count}", "mistakes.generateSimilar": "類題を生成", "mistakes.saveQuestionSet": "問題セットを保存", "mistakes.similarHint": "この誤解を狙った新しい問題セットを生成します。", "mistakes.generateSelfTest": "セルフテストを生成", "mistakes.submitTest": "回答を提出", "mistakes.saveTestResult": "結果を保存", "mistakes.answerPlaceholder": "回答", "mistakes.selfTestHint": "問題を生成して回答し、AI に採点させます。", "mistakes.generateMindMap": "マインドマップを生成", "mistakes.saveMap": "マップを保存", "mistakes.mindMapHint": "間違いを小さな概念マップに整理します。", "mistakes.you": "あなた", "mistakes.tutor": "講師", "mistakes.debateHint": "推論を一つずつ説明して守りましょう。", "mistakes.debatePlaceholder": "手順を説明または弁護してください…", "mistakes.sendDebate": "送信", "mistakes.saveDebate": "ディベートを保存", "mistakes.findFaultLine": "知識の断層を探す", "mistakes.createRepairTasks": "修復タスクを作成", "mistakes.faultLineHint": "記録した間違いから繰り返す概念の穴を探します。", "mistakes.confirmRepairTasks": "{count} 件の修復タスクをローカル一覧に作成しますか？", "mistakes.imageHint": "間違いの写真を添付します。AI OCR と認識結果は挿入または保存まで下書きです。", "mistakes.recognizeImage": "間違いを認識", "mistakes.runOcr": "AI OCR を実行", "mistakes.insertOcr": "OCR テキストを挿入", "mistakes.ocrConfidence": "OCR 信頼度 {count}%", "mistakes.imageTooLarge": "8 MiB 未満の画像を選択してください。", "mistakes.savedSessions": "保存済み AI セッション（{count}）",
  },
  ko: {
    "mistakes.workbench": "오답 AI 워크벤치", "mistakes.aiBoundary": "AI 출력은 검토용 초안입니다. 사용자가 실행하기 전에는 저장되지 않습니다.", "mistakes.addImage": "이미지 첨부", "mistakes.imagePreview": "첨부한 오답 이미지", "mistakes.imageReady": "AI용 이미지가 준비되었습니다", "mistakes.applyAndSave": "적용하고 저장", "mistakes.analysisHint": "진단을 생성한 뒤 적용할 항목을 선택하세요.", "mistakes.selectFirst": "오답을 열거나 먼저 만드세요.", "mistakes.staleResult": "제공자가 캐시된 결과를 반환했습니다. 적용하기 전에 확인하세요.", "mistakes.operation.analysis": "분석", "mistakes.operation.questions": "유사 문제", "mistakes.operation.selfTest": "자가 테스트", "mistakes.operation.mindMap": "마인드맵", "mistakes.operation.debate": "오답 토론", "mistakes.operation.faultLine": "지식 단절", "mistakes.operation.image": "이미지 / OCR", "mistakes.multipleChoice": "객관식", "mistakes.fillBlank": "빈칸 채우기", "mistakes.difficulty": "난이도 {count}", "mistakes.generateSimilar": "유사 문제 생성", "mistakes.saveQuestionSet": "문제 세트 저장", "mistakes.similarHint": "이번 오개념을 겨냥한 새 문제 세트를 생성합니다.", "mistakes.generateSelfTest": "자가 테스트 생성", "mistakes.submitTest": "답안 제출", "mistakes.saveTestResult": "결과 저장", "mistakes.answerPlaceholder": "나의 답", "mistakes.selfTestHint": "문제를 만들고 답한 뒤 AI에게 채점을 맡기세요.", "mistakes.generateMindMap": "마인드맵 생성", "mistakes.saveMap": "맵 저장", "mistakes.mindMapHint": "오답을 작은 개념 지도로 정리합니다.", "mistakes.you": "나", "mistakes.tutor": "튜터", "mistakes.debateHint": "추론을 한 단계씩 설명하고 방어해 보세요.", "mistakes.debatePlaceholder": "단계를 설명하거나 방어하세요…", "mistakes.sendDebate": "보내기", "mistakes.saveDebate": "토론 저장", "mistakes.findFaultLine": "지식 단절 찾기", "mistakes.createRepairTasks": "복구 할 일 만들기", "mistakes.faultLineHint": "기록한 오답을 비교해 반복되는 개념의 빈틈을 찾습니다.", "mistakes.confirmRepairTasks": "로컬 할 일 목록에 복구 할 일 {count}개를 만들까요?", "mistakes.imageHint": "오답 사진을 첨부하세요. AI OCR과 인식 결과는 삽입하거나 저장하기 전까지 초안으로 유지됩니다.", "mistakes.recognizeImage": "오답 인식", "mistakes.runOcr": "AI OCR 실행", "mistakes.insertOcr": "OCR 텍스트 삽입", "mistakes.ocrConfidence": "OCR 신뢰도 {count}%", "mistakes.imageTooLarge": "8 MiB보다 작은 이미지를 선택하세요.", "mistakes.savedSessions": "저장된 AI 세션 ({count})",
  },
};

// P1 keys belong to Diary, Trends, and Flashcards. They are merged after base
// dictionaries so feature-specific wording can override a shared key.
const p1En: Record<string, string> = {
  "nav.diary": "Diary", "nav.trends": "Trends", "nav.flashcards": "Flashcards",
  "diary.title": "Learning Diary", "diary.description": "A small daily record for mood, energy and what you learned.", "diary.new": "New entry", "diary.edit": "Edit entry", "diary.entryTitle": "Daily check-in", "diary.entryDescription": "Multiple entries on the same day are supported.", "diary.date": "Date", "diary.mood": "Mood", "diary.energy": "Energy", "diary.tag": "Energy tag", "diary.tagPlaceholder": "e.g. focused, tired", "diary.content": "Markdown note", "diary.contentPlaceholder": "What happened today?", "diary.update": "Update entry", "diary.save": "Save entry", "diary.cancel": "Cancel", "diary.trendTitle": "Recent rhythm", "diary.trendDescription": "Your daily mood and energy over the last 30 days.", "diary.activeDays": "active days", "diary.studyMinutes": "study minutes", "diary.energyShort": "energy", "diary.history": "Diary history", "diary.historyDescription": "Your entries, kept local and easy to revisit.", "diary.delete": "Delete", "diary.noContent": "No note content.", "diary.empty": "No diary entries yet", "diary.emptyCopy": "Start with a quick mood and a sentence about today.", "diary.validationDate": "Please choose a date.", "diary.confirmDelete": "Delete this diary entry?",
  "trends.title": "Trends", "trends.description": "See how study time, results, wellbeing and review connect.", "trends.score": "Score", "trends.ranking": "Ranking", "trends.heatmapTitle": "90-day activity", "trends.heatmapDescription": "Study minutes + reviews + grades × 5.", "trends.activeDays": "active days", "trends.points": "activity points", "trends.streak": "day streak", "trends.studyTime": "study time", "trends.range": "{start} – {end}", "trends.averageMood": "Average mood", "trends.averageEnergy": "Average energy", "trends.outOfFive": "/ 5", "trends.dueReviews": "Due reviews", "trends.upcomingReviews": "Next 7 days", "trends.studyChartTitle": "Study rhythm", "trends.studyChartDescription": "Completed study minutes by day.", "trends.srsTitle": "Review queue", "trends.srsDescription": "Mistakes enrolled in text flashcard review.", "trends.enrolled": "enrolled", "trends.due": "due now", "trends.nextSeven": "next 7 days", "trends.flashcardHint": "Open Flashcards to review the due queue.", "trends.subjectTitle": "Subject performance", "trends.subjectDescription": "Recent grades, mistakes and direction by subject.", "trends.grades": "grades", "trends.mistakes": "mistakes", "trends.latest": "latest", "trends.average": "average", "trends.dueMistakes": "due mistakes", "trends.latestRanking": "latest rank", "trends.averageRanking": "average rank", "trends.needsAttention": "Needs attention", "trends.noSubjects": "No subject trends yet", "trends.noSubjectsCopy": "Add grades or mistakes to see subject direction.", "trends.rising": "Rising", "trends.falling": "Falling", "trends.steady": "Steady",
  "flashcards.title": "Flashcards", "flashcards.description": "Turn enrolled mistakes into a focused text-only SRS session.", "flashcards.enrolled": "enrolled", "flashcards.summaryTitle": "Session complete", "flashcards.summaryCopy": "A small review session is still a useful return to the material.", "flashcards.reviewAgain": "Review again", "flashcards.progress": "{current} of {total}", "flashcards.question": "Question", "flashcards.answer": "Answer", "flashcards.reason": "Why it went wrong", "flashcards.wrongSolution": "Wrong solution", "flashcards.correctSolution": "Correct solution", "flashcards.clickQuestion": "Click to reveal the answer", "flashcards.clickAnswer": "Click to return to the question", "flashcards.again": "Again", "flashcards.againHint": "Requeue soon", "flashcards.hard": "Hard", "flashcards.hardHint": "Short interval", "flashcards.good": "Good", "flashcards.goodHint": "Normal interval", "flashcards.easy": "Easy", "flashcards.easyHint": "Long interval", "flashcards.empty": "Your review queue is clear", "flashcards.emptyDue": "There are no due cards right now.", "flashcards.emptyEnroll": "Enroll mistakes from the Mistakes page to build the queue.",
  "mistakes.enroll": "Add to review queue", "mistakes.easy": "Easy",
};

const p1ZhCN: Record<string, string> = {
  "nav.diary": "学习日记", "nav.trends": "趋势", "nav.flashcards": "闪卡",
  "diary.title": "学习日记", "diary.description": "记录心情、精力和今天学到的东西。", "diary.new": "新建记录", "diary.edit": "编辑记录", "diary.entryTitle": "今日状态", "diary.entryDescription": "同一天可以添加多条记录。", "diary.date": "日期", "diary.mood": "心情", "diary.energy": "精力", "diary.tag": "精力标签", "diary.tagPlaceholder": "例如：专注、疲惫", "diary.content": "Markdown 笔记", "diary.contentPlaceholder": "今天发生了什么？", "diary.update": "更新记录", "diary.save": "保存记录", "diary.cancel": "取消", "diary.trendTitle": "近期节奏", "diary.trendDescription": "最近 30 天的每日心情和精力。", "diary.activeDays": "活跃天数", "diary.studyMinutes": "学习分钟", "diary.energyShort": "精力", "diary.history": "日记历史", "diary.historyDescription": "记录保存在本地，随时可以回看。", "diary.delete": "删除", "diary.noContent": "没有笔记内容。", "diary.empty": "还没有日记记录", "diary.emptyCopy": "从一个心情和一句今日记录开始。", "diary.validationDate": "请选择日期。", "diary.confirmDelete": "删除这条日记记录吗？",
  "trends.title": "趋势", "trends.description": "查看学习时长、成绩、状态和复习之间的联系。", "trends.score": "分数", "trends.ranking": "排名", "trends.heatmapTitle": "90 天活动", "trends.heatmapDescription": "学习分钟 + 复习次数 + 成绩次数 × 5。", "trends.activeDays": "活跃天数", "trends.points": "活动点", "trends.streak": "连续天数", "trends.studyTime": "学习时长", "trends.range": "{start} – {end}", "trends.averageMood": "平均心情", "trends.averageEnergy": "平均精力", "trends.outOfFive": "/ 5", "trends.dueReviews": "到期复习", "trends.upcomingReviews": "未来 7 天", "trends.studyChartTitle": "学习节奏", "trends.studyChartDescription": "按天统计已完成的学习分钟。", "trends.srsTitle": "复习队列", "trends.srsDescription": "已加入文字闪卡复习的错题。", "trends.enrolled": "已入队", "trends.due": "当前到期", "trends.nextSeven": "未来 7 天", "trends.flashcardHint": "打开闪卡开始复习到期队列。", "trends.subjectTitle": "科目表现", "trends.subjectDescription": "按科目查看近期成绩、错题和方向。", "trends.grades": "成绩", "trends.mistakes": "错题", "trends.latest": "最近", "trends.average": "平均", "trends.dueMistakes": "到期错题", "trends.latestRanking": "最近排名", "trends.averageRanking": "平均排名", "trends.needsAttention": "需要关注", "trends.noSubjects": "还没有科目趋势", "trends.noSubjectsCopy": "添加成绩或错题后，这里会显示科目方向。", "trends.rising": "上升", "trends.falling": "下降", "trends.steady": "稳定",
  "flashcards.title": "闪卡", "flashcards.description": "把已入队错题变成专注的文字 SRS 复习。", "flashcards.enrolled": "已入队", "flashcards.summaryTitle": "本次复习完成", "flashcards.summaryCopy": "短暂复习也是一次回到材料的有效练习。", "flashcards.reviewAgain": "再次复习", "flashcards.progress": "{current} / {total}", "flashcards.question": "问题", "flashcards.answer": "答案", "flashcards.reason": "出错原因", "flashcards.wrongSolution": "错误解法", "flashcards.correctSolution": "正确解法", "flashcards.clickQuestion": "点击查看答案", "flashcards.clickAnswer": "点击返回问题", "flashcards.again": "再来一次", "flashcards.againHint": "很快重新出现", "flashcards.hard": "困难", "flashcards.hardHint": "较短间隔", "flashcards.good": "良好", "flashcards.goodHint": "正常间隔", "flashcards.easy": "简单", "flashcards.easyHint": "较长间隔", "flashcards.empty": "复习队列已清空", "flashcards.emptyDue": "现在没有到期闪卡。", "flashcards.emptyEnroll": "在错题页面将错题加入队列。",
  "mistakes.enroll": "加入复习队列", "mistakes.easy": "简单",
};

const p1ZhTW: Record<string, string> = {
  "nav.diary": "學習日記", "nav.trends": "趨勢", "nav.flashcards": "閃卡",
  "diary.title": "學習日記", "diary.description": "記錄心情、精力與今天學到的事。", "diary.new": "新增記錄", "diary.edit": "編輯記錄", "diary.entryTitle": "今日狀態", "diary.entryDescription": "同一天可以新增多筆記錄。", "diary.date": "日期", "diary.mood": "心情", "diary.energy": "精力", "diary.tag": "精力標籤", "diary.tagPlaceholder": "例如：專注、疲憊", "diary.content": "Markdown 筆記", "diary.contentPlaceholder": "今天發生了什麼？", "diary.update": "更新記錄", "diary.save": "儲存記錄", "diary.cancel": "取消", "diary.trendTitle": "近期節奏", "diary.trendDescription": "最近 30 天的每日心情與精力。", "diary.activeDays": "活躍天數", "diary.studyMinutes": "學習分鐘", "diary.energyShort": "精力", "diary.history": "日記歷史", "diary.historyDescription": "記錄保存在本機，隨時可以回看。", "diary.delete": "刪除", "diary.noContent": "沒有筆記內容。", "diary.empty": "還沒有日記記錄", "diary.emptyCopy": "從一個心情和一句今日記錄開始。", "diary.validationDate": "請選擇日期。", "diary.confirmDelete": "要刪除這筆日記記錄嗎？",
  "trends.title": "趨勢", "trends.description": "查看學習時長、成績、狀態與複習之間的關聯。", "trends.score": "分數", "trends.ranking": "排名", "trends.heatmapTitle": "90 天活動", "trends.heatmapDescription": "學習分鐘 + 複習次數 + 成績次數 × 5。", "trends.activeDays": "活躍天數", "trends.points": "活動點", "trends.streak": "連續天數", "trends.studyTime": "學習時長", "trends.range": "最近 90 天", "trends.averageMood": "平均心情", "trends.averageEnergy": "平均精力", "trends.outOfFive": "/ 5", "trends.dueReviews": "到期複習", "trends.upcomingReviews": "未來 7 天", "trends.studyChartTitle": "學習節奏", "trends.studyChartDescription": "按天統計已完成的學習分鐘。", "trends.srsTitle": "複習佇列", "trends.srsDescription": "已加入文字閃卡複習的錯題。", "trends.enrolled": "已入隊", "trends.due": "目前到期", "trends.nextSeven": "未來 7 天", "trends.flashcardHint": "開啟閃卡開始複習到期佇列。", "trends.subjectTitle": "科目表現", "trends.subjectDescription": "按科目查看近期成績、錯題與方向。", "trends.grades": "成績", "trends.mistakes": "錯題", "trends.latest": "最近", "trends.average": "平均", "trends.dueMistakes": "到期錯題", "trends.latestRanking": "最近排名", "trends.averageRanking": "平均排名", "trends.needsAttention": "需要關注", "trends.noSubjects": "還沒有科目趨勢", "trends.noSubjectsCopy": "新增成績或錯題後，這裡會顯示科目方向。", "trends.rising": "上升", "trends.falling": "下降", "trends.steady": "穩定",
  "flashcards.title": "閃卡", "flashcards.description": "把已入隊錯題變成專注的文字 SRS 複習。", "flashcards.enrolled": "已入隊", "flashcards.summaryTitle": "本次複習完成", "flashcards.summaryCopy": "短暫複習也是一次回到材料的有效練習。", "flashcards.reviewAgain": "再次複習", "flashcards.progress": "{current} / {total}", "flashcards.question": "問題", "flashcards.answer": "答案", "flashcards.reason": "出錯原因", "flashcards.wrongSolution": "錯誤解法", "flashcards.correctSolution": "正確解法", "flashcards.clickQuestion": "點擊查看答案", "flashcards.clickAnswer": "點擊返回問題", "flashcards.again": "再來一次", "flashcards.againHint": "很快重新出現", "flashcards.hard": "困難", "flashcards.hardHint": "較短間隔", "flashcards.good": "良好", "flashcards.goodHint": "正常間隔", "flashcards.easy": "簡單", "flashcards.easyHint": "較長間隔", "flashcards.empty": "複習佇列已清空", "flashcards.emptyDue": "現在沒有到期閃卡。", "flashcards.emptyEnroll": "在錯題頁面將錯題加入佇列。",
  "mistakes.enroll": "加入複習佇列", "mistakes.easy": "簡單",
};

const p1Ja: Record<string, string> = {
  "nav.diary": "学習日記", "nav.trends": "トレンド", "nav.flashcards": "フラッシュカード",
  "diary.title": "学習日記", "diary.description": "気分、エネルギー、今日学んだことを小さく記録します。", "diary.new": "新しい記録", "diary.edit": "記録を編集", "diary.entryTitle": "今日のチェックイン", "diary.entryDescription": "同じ日に複数の記録を追加できます。", "diary.date": "日付", "diary.mood": "気分", "diary.energy": "エネルギー", "diary.tag": "エネルギータグ", "diary.tagPlaceholder": "例：集中、疲れ", "diary.content": "Markdown メモ", "diary.contentPlaceholder": "今日はどうでしたか？", "diary.update": "記録を更新", "diary.save": "記録を保存", "diary.cancel": "キャンセル", "diary.trendTitle": "最近のリズム", "diary.trendDescription": "過去 30 日間の気分とエネルギー。", "diary.activeDays": "活動日", "diary.studyMinutes": "学習分", "diary.energyShort": "エネルギー", "diary.history": "日記の履歴", "diary.historyDescription": "記録はローカルに保存され、いつでも振り返れます。", "diary.delete": "削除", "diary.noContent": "メモはありません。", "diary.empty": "日記はまだありません", "diary.emptyCopy": "気分と今日の一文から始めましょう。", "diary.validationDate": "日付を選択してください。", "diary.confirmDelete": "この日記を削除しますか？",
  "trends.title": "トレンド", "trends.description": "学習時間、結果、状態、復習のつながりを確認します。", "trends.score": "得点", "trends.ranking": "順位", "trends.heatmapTitle": "90 日間の活動", "trends.heatmapDescription": "学習分 + 復習回数 + 成績回数 × 5。", "trends.activeDays": "活動日", "trends.points": "活動ポイント", "trends.streak": "日連続", "trends.studyTime": "学習時間", "trends.range": "過去 90 日", "trends.averageMood": "平均気分", "trends.averageEnergy": "平均エネルギー", "trends.outOfFive": "/ 5", "trends.dueReviews": "期限の復習", "trends.upcomingReviews": "次の 7 日間", "trends.studyChartTitle": "学習リズム", "trends.studyChartDescription": "完了した学習分を日ごとに表示します。", "trends.srsTitle": "復習キュー", "trends.srsDescription": "テキストフラッシュカードに登録した間違い。", "trends.enrolled": "登録済み", "trends.due": "今日期限", "trends.nextSeven": "次の 7 日", "trends.flashcardHint": "フラッシュカードを開いて期限のカードを復習しましょう。", "trends.subjectTitle": "科目の成績", "trends.subjectDescription": "科目ごとの最近の成績、間違い、方向性。", "trends.grades": "成績", "trends.mistakes": "間違い", "trends.latest": "最新", "trends.average": "平均", "trends.dueMistakes": "期限の間違い", "trends.latestRanking": "最新順位", "trends.averageRanking": "平均順位", "trends.needsAttention": "要確認", "trends.noSubjects": "科目トレンドはまだありません", "trends.noSubjectsCopy": "成績や間違いを追加すると科目の方向性が表示されます。", "trends.rising": "上昇", "trends.falling": "下降", "trends.steady": "安定",
  "flashcards.title": "フラッシュカード", "flashcards.description": "登録した間違いを集中したテキスト SRS セッションにします。", "flashcards.enrolled": "登録済み", "flashcards.summaryTitle": "セッション完了", "flashcards.summaryCopy": "短い復習でも、教材に戻る大切な一歩です。", "flashcards.reviewAgain": "もう一度復習", "flashcards.progress": "{current} / {total}", "flashcards.question": "問題", "flashcards.answer": "答え", "flashcards.reason": "間違えた理由", "flashcards.wrongSolution": "誤った解法", "flashcards.correctSolution": "正しい解法", "flashcards.clickQuestion": "クリックして問題に戻る", "flashcards.clickAnswer": "クリックして答えを見る", "flashcards.again": "もう一度", "flashcards.againHint": "すぐに再出題", "flashcards.hard": "難しい", "flashcards.hardHint": "短い間隔", "flashcards.good": "良い", "flashcards.goodHint": "通常の間隔", "flashcards.easy": "簡単", "flashcards.easyHint": "長い間隔", "flashcards.empty": "復習キューは空です", "flashcards.emptyDue": "今日期限のカードはありません。", "flashcards.emptyEnroll": "間違いページからカードをキューに登録してください。",
  "mistakes.enroll": "復習キューに追加", "mistakes.easy": "簡単",
};

const p1Ko: Record<string, string> = {
  "nav.diary": "학습 일기", "nav.trends": "추세", "nav.flashcards": "플래시카드",
  "diary.title": "학습 일기", "diary.description": "기분, 에너지와 오늘 배운 내용을 작게 기록하세요.", "diary.new": "새 기록", "diary.edit": "기록 편집", "diary.entryTitle": "오늘의 체크인", "diary.entryDescription": "같은 날에도 여러 기록을 추가할 수 있습니다.", "diary.date": "날짜", "diary.mood": "기분", "diary.energy": "에너지", "diary.tag": "에너지 태그", "diary.tagPlaceholder": "예: 집중, 피곤함", "diary.content": "Markdown 메모", "diary.contentPlaceholder": "오늘은 어땠나요?", "diary.update": "기록 업데이트", "diary.save": "기록 저장", "diary.cancel": "취소", "diary.trendTitle": "최근 리듬", "diary.trendDescription": "최근 30일의 기분과 에너지입니다.", "diary.activeDays": "활동 일수", "diary.studyMinutes": "학습 분", "diary.energyShort": "에너지", "diary.history": "일기 기록", "diary.historyDescription": "기록은 로컬에 저장되어 언제든 돌아볼 수 있습니다.", "diary.delete": "삭제", "diary.noContent": "메모 내용이 없습니다.", "diary.empty": "아직 일기가 없습니다", "diary.emptyCopy": "기분과 오늘의 한 문장으로 시작해 보세요.", "diary.validationDate": "날짜를 선택하세요.", "diary.confirmDelete": "이 일기 기록을 삭제할까요?",
  "trends.title": "추세", "trends.description": "학습 시간, 결과, 상태와 복습의 연결을 확인하세요.", "trends.score": "점수", "trends.ranking": "순위", "trends.heatmapTitle": "90일 활동", "trends.heatmapDescription": "학습 분 + 복습 횟수 + 성적 횟수 × 5.", "trends.activeDays": "활동 일수", "trends.points": "활동 포인트", "trends.streak": "일 연속", "trends.studyTime": "학습 시간", "trends.range": "최근 90일", "trends.averageMood": "평균 기분", "trends.averageEnergy": "평균 에너지", "trends.outOfFive": "/ 5", "trends.dueReviews": "복습 예정", "trends.upcomingReviews": "다음 7일", "trends.studyChartTitle": "학습 리듬", "trends.studyChartDescription": "완료한 학습 분을 날짜별로 보여 줍니다.", "trends.srsTitle": "복습 대기열", "trends.srsDescription": "텍스트 플래시카드 복습에 등록된 오답입니다.", "trends.enrolled": "등록됨", "trends.due": "현재 예정", "trends.nextSeven": "다음 7일", "trends.flashcardHint": "플래시카드를 열어 예정된 대기열을 복습하세요.", "trends.subjectTitle": "과목 성과", "trends.subjectDescription": "과목별 최근 성적, 오답과 방향입니다.", "trends.grades": "성적", "trends.mistakes": "오답", "trends.latest": "최근", "trends.average": "평균", "trends.dueMistakes": "복습할 오답", "trends.latestRanking": "최근 순위", "trends.averageRanking": "평균 순위", "trends.needsAttention": "관심 필요", "trends.noSubjects": "아직 과목 추세가 없습니다", "trends.noSubjectsCopy": "성적이나 오답을 추가하면 과목 방향이 표시됩니다.", "trends.rising": "상승", "trends.falling": "하락", "trends.steady": "안정",
  "flashcards.title": "플래시카드", "flashcards.description": "등록한 오답을 집중적인 텍스트 SRS 세션으로 바꿉니다.", "flashcards.enrolled": "등록됨", "flashcards.summaryTitle": "세션 완료", "flashcards.summaryCopy": "짧은 복습도 학습 내용으로 돌아가는 유용한 시간입니다.", "flashcards.reviewAgain": "다시 복습", "flashcards.progress": "{current} / {total}", "flashcards.question": "문제", "flashcards.answer": "답", "flashcards.reason": "틀린 이유", "flashcards.wrongSolution": "잘못된 풀이", "flashcards.correctSolution": "올바른 풀이", "flashcards.clickQuestion": "클릭해 문제로 돌아가기", "flashcards.clickAnswer": "클릭해 답 보기", "flashcards.again": "다시", "flashcards.againHint": "곧 다시 출제", "flashcards.hard": "어려움", "flashcards.hardHint": "짧은 간격", "flashcards.good": "좋음", "flashcards.goodHint": "일반 간격", "flashcards.easy": "쉬움", "flashcards.easyHint": "긴 간격", "flashcards.empty": "복습 대기열이 비었습니다", "flashcards.emptyDue": "지금 복습할 카드가 없습니다.", "flashcards.emptyEnroll": "오답 페이지에서 오답을 대기열에 등록하세요.",
  "mistakes.enroll": "복습 대기열에 추가", "mistakes.easy": "쉬움",
};

// P2 keys cover Coach, Exam Simulation, Reverse Planner, and Reports. The
// current dictionary map below intentionally exposes only the existing keys.
const p2En: Record<string, string> = {
  "exams.comprehensive": "Comprehensive exams", "exams.comprehensiveDescription": "Plan a combined exam across multiple subjects.", "exams.subjectsComma": "Subjects, comma separated", "exams.comprehensiveEmpty": "No comprehensive exams yet.",
  "nav.coach": "AI Coach", "nav.simulation": "Exam Simulator", "nav.planner": "Reverse Planner", "nav.reports": "Reports",
  "feature.providerRequired": "Connect Cloud AI or BYOK in Settings before generating or changing AI feature data.", "feature.saved": "Saved", "feature.draft": "Draft", "feature.working": "Working…", "common.send": "Send",
  "coach.title": "AI Coach", "coach.description": "Turn a score goal into evidence, risks and reviewable task proposals.", "coach.goalTitle": "Goal title", "coach.subject": "Subject", "coach.baseline": "Baseline score", "coach.target": "Target score", "coach.fullScore": "Full score", "coach.weight": "Subject weight", "coach.minutes": "Daily minutes", "coach.purpose": "Purpose", "coach.constraints": "Constraints", "coach.saveGoal": "Save goal", "coach.generate": "Analyze & propose", "coach.prediction": "Predicted score", "coach.probability": "Success probability", "coach.risks": "Risks", "coach.reject": "Reject", "coach.approve": "Approve tasks", "coach.chat": "Coach conversation", "coach.chatDescription": "Conversation is kept with the Coach goal.", "coach.chatPlaceholder": "Ask your coach…",
  "simulation.title": "Exam Simulator", "simulation.description": "A ten-question, twenty-minute session with autosave and behavior analysis.", "simulation.generate": "Generate 10 questions", "simulation.questions": "questions", "simulation.empty": "Generate a simulation to begin.", "simulation.start": "Start exam", "simulation.resume": "Resume exam", "simulation.previous": "Previous", "simulation.next": "Next", "simulation.submit": "Submit", "simulation.score": "Score", "simulation.answerPlaceholder": "Write your answer…",
  "planner.title": "Reverse Planner", "planner.description": "Start with the target score and work backward through weak points and daily routes.", "planner.examName": "Exam name", "planner.currentScore": "Current score", "planner.targetScore": "Target score", "planner.saveGoal": "Save exam goal", "planner.generate": "Generate route", "planner.savePlan": "Save plan", "planner.delete": "Delete", "planner.confirmDelete": "Delete this exam goal and its plan?", "planner.weakPoints": "Weak points", "planner.dailyTasks": "Daily route", "planner.empty": "Generate a plan to see weak points and daily tasks.", "planner.noGoal": "No exam goal yet", "planner.noGoalCopy": "Add an exam target above to build a route.",
  "reports.title": "Learning Reports", "reports.export": "Export StudyPulse report", "reports.studyTime": "Study time", "reports.sessions": "Sessions", "reports.score": "Score rate", "reports.mood": "Mood", "reports.share": "Share / open file",
  "mode.coach": "AI Coach", "mode.examSimulation": "Exam simulation", "mode.reversePlanner": "Reverse planner",
};

const p2ZhCN: Record<string, string> = {
  "exams.comprehensive": "综合考试", "exams.comprehensiveDescription": "为多个科目组合的综合考试建立计划。", "exams.subjectsComma": "科目，用逗号分隔", "exams.comprehensiveEmpty": "还没有综合考试。",
  "nav.coach": "AI 教练", "nav.simulation": "考试模拟", "nav.planner": "Reverse Planner", "nav.reports": "报告",
  "feature.providerRequired": "请先在设置中连接 Cloud AI 或 BYOK，才能生成或修改 AI 功能数据。", "feature.saved": "已保存", "feature.draft": "草稿", "feature.working": "处理中…", "common.send": "发送",
  "coach.title": "AI 教练", "coach.description": "把分数目标转成证据、风险和待审核任务建议。", "coach.goalTitle": "目标名称", "coach.subject": "科目", "coach.baseline": "基线分数", "coach.target": "目标分数", "coach.fullScore": "满分", "coach.weight": "科目权重", "coach.minutes": "每日分钟", "coach.purpose": "目的", "coach.constraints": "约束", "coach.saveGoal": "保存目标", "coach.generate": "分析并生成建议", "coach.prediction": "预测分数", "coach.probability": "达成概率", "coach.risks": "风险", "coach.reject": "拒绝", "coach.approve": "批准任务", "coach.chat": "教练对话", "coach.chatDescription": "对话会随 Coach 目标保存在本地。", "coach.chatPlaceholder": "问问你的教练…",
  "simulation.title": "考试模拟器", "simulation.description": "10 道题、20 分钟，自动保存并分析考试行为。", "simulation.generate": "生成 10 道题", "simulation.questions": "道题", "simulation.empty": "生成一次模拟考试开始。", "simulation.start": "开始考试", "simulation.resume": "恢复考试", "simulation.previous": "上一题", "simulation.next": "下一题", "simulation.submit": "交卷", "simulation.score": "得分", "simulation.answerPlaceholder": "写下你的答案…",
  "planner.title": "Reverse Planner", "planner.description": "从目标分数倒推薄弱点、阶段路线和每日任务。", "planner.examName": "考试名称", "planner.currentScore": "当前分数", "planner.targetScore": "目标分数", "planner.saveGoal": "保存考试目标", "planner.generate": "生成路线", "planner.savePlan": "保存计划", "planner.delete": "删除", "planner.confirmDelete": "删除这个考试目标和计划吗？", "planner.weakPoints": "薄弱点", "planner.dailyTasks": "每日路线", "planner.empty": "生成计划后会显示薄弱点和每日任务。", "planner.noGoal": "还没有考试目标", "planner.noGoalCopy": "先在上方添加考试目标。",
  "reports.title": "学习报告", "reports.export": "导出 StudyPulse 报告", "reports.studyTime": "学习时长", "reports.sessions": "学习 session", "reports.score": "成绩率", "reports.mood": "心情", "reports.share": "分享 / 打开文件",
  "mode.coach": "AI 教练", "mode.examSimulation": "考试模拟", "mode.reversePlanner": "Reverse Planner",
};

// Phase 3 stays in Today and Exams, so these labels are deliberately a small
// complete five-language map instead of adding another navigation section.
const phase3Translations: Record<Language, Record<string, string>> = {
  en: { "p3.todayTitle": "Learning AI", "p3.todayCopy": "Saved local drafts; tasks require another confirmation.", "p3.homeAsk": "Ask about today", "p3.askPlaceholder": "Ask about your study…", "p3.suggestions": "Study suggestions", "p3.dailyPlan": "Daily plan", "p3.generate": "Generate", "p3.createTasks": "Create selected tasks", "p3.applied": "applied", "p3.examTitle": "Exam AI", "p3.examCopy": "Predictions need four valid grades per subject. They are estimates, not guarantees.", "p3.selectExam": "Choose an exam", "p3.selectSingle": "Choose a single-subject exam for an autopsy.", "p3.predict": "Predict score", "p3.estimate": "Estimate", "p3.range": "Range", "p3.discussPlaceholder": "Ask about this prediction…", "p3.autopsy": "Exam Autopsy", "p3.runAutopsy": "Analyze selected images", "p3.imageTooLarge": "Each image must be no larger than 8 MiB.", "p3.question": "Question", "p3.importMistake": "Import mistake", "p3.repairTask": "Create repair task", "p3.applySelected": "Apply selected" },
  "zh-CN": { "p3.todayTitle": "学习 AI", "p3.todayCopy": "草稿保存在本地；创建任务仍需再次确认。", "p3.homeAsk": "询问今天", "p3.askPlaceholder": "问问你的学习情况…", "p3.suggestions": "学习建议", "p3.dailyPlan": "每日计划", "p3.generate": "生成", "p3.createTasks": "创建已选任务", "p3.applied": "已应用", "p3.examTitle": "考试 AI", "p3.examCopy": "每个科目至少需四条有效成绩。预测仅供参考，不是保证。", "p3.selectExam": "选择考试", "p3.selectSingle": "请为复盘选择一场单科考试。", "p3.predict": "预测分数", "p3.estimate": "估计", "p3.range": "区间", "p3.discussPlaceholder": "询问这个预测…", "p3.autopsy": "考试复盘", "p3.runAutopsy": "分析已选图片", "p3.imageTooLarge": "每张图片不能超过 8 MiB。", "p3.question": "题目", "p3.importMistake": "导入错题", "p3.repairTask": "创建修复任务", "p3.applySelected": "应用所选" },
  "zh-TW": { "p3.todayTitle": "學習 AI", "p3.todayCopy": "草稿保存在本機；建立任務仍需再次確認。", "p3.homeAsk": "詢問今天", "p3.askPlaceholder": "問問你的學習狀況…", "p3.suggestions": "學習建議", "p3.dailyPlan": "每日計畫", "p3.generate": "產生", "p3.createTasks": "建立已選任務", "p3.applied": "已套用", "p3.examTitle": "考試 AI", "p3.examCopy": "每科至少需四筆有效成績。預測僅供參考，並非保證。", "p3.selectExam": "選擇考試", "p3.selectSingle": "請為復盤選擇一場單科考試。", "p3.predict": "預測分數", "p3.estimate": "估計", "p3.range": "區間", "p3.discussPlaceholder": "詢問這個預測…", "p3.autopsy": "考試復盤", "p3.runAutopsy": "分析所選圖片", "p3.imageTooLarge": "每張圖片不得超過 8 MiB。", "p3.question": "題目", "p3.importMistake": "匯入錯題", "p3.repairTask": "建立修復任務", "p3.applySelected": "套用所選" },
  ja: { "p3.todayTitle": "学習 AI", "p3.todayCopy": "下書きはローカルに保存され、タスク作成には再確認が必要です。", "p3.homeAsk": "今日について質問", "p3.askPlaceholder": "学習について質問…", "p3.suggestions": "学習の提案", "p3.dailyPlan": "今日の計画", "p3.generate": "生成", "p3.createTasks": "選択したタスクを作成", "p3.applied": "適用済み", "p3.examTitle": "試験 AI", "p3.examCopy": "各科目には有効な成績が 4 件必要です。予測は保証ではありません。", "p3.selectExam": "試験を選択", "p3.selectSingle": "振り返り用に単一科目の試験を選択してください。", "p3.predict": "スコアを予測", "p3.estimate": "推定", "p3.range": "範囲", "p3.discussPlaceholder": "この予測について質問…", "p3.autopsy": "試験振り返り", "p3.runAutopsy": "選択画像を分析", "p3.imageTooLarge": "各画像は 8 MiB 以下にしてください。", "p3.question": "問題", "p3.importMistake": "間違いを登録", "p3.repairTask": "修復タスクを作成", "p3.applySelected": "選択を適用" },
  ko: { "p3.todayTitle": "학습 AI", "p3.todayCopy": "초안은 로컬에 저장되며, 작업 생성에는 다시 확인이 필요합니다.", "p3.homeAsk": "오늘에 대해 묻기", "p3.askPlaceholder": "학습에 관해 물어보세요…", "p3.suggestions": "학습 제안", "p3.dailyPlan": "오늘의 계획", "p3.generate": "생성", "p3.createTasks": "선택한 작업 만들기", "p3.applied": "적용됨", "p3.examTitle": "시험 AI", "p3.examCopy": "과목마다 유효한 성적 4개가 필요합니다. 예측은 보장이 아닙니다.", "p3.selectExam": "시험 선택", "p3.selectSingle": "복기를 위해 단일 과목 시험을 선택하세요.", "p3.predict": "점수 예측", "p3.estimate": "추정", "p3.range": "범위", "p3.discussPlaceholder": "이 예측에 대해 질문…", "p3.autopsy": "시험 복기", "p3.runAutopsy": "선택한 이미지 분석", "p3.imageTooLarge": "각 이미지는 8 MiB 이하여야 합니다.", "p3.question": "문제", "p3.importMistake": "오답 가져오기", "p3.repairTask": "복구 작업 만들기", "p3.applySelected": "선택 적용" },
};

const workbenchAppearanceTranslations: Record<Language, Record<string, string>> = {
  en: {
    "workspace.today": "Today",
    "workspace.agent": "Agent",
    "workspace.study": "Study",
    "workspace.review": "Review",
    "workspace.insights": "Insights",
    "workspace.library": "Library",
    "switcher.placeholder": "Search workspaces or pages… (⌘K)",
    "switcher.noResults": "No matching pages found.",
    "switcher.navigation": "Navigation",
    "switcher.hint": "Navigate with ↑ ↓, press Enter to select, Esc to dismiss",
    "switcher.trigger": "Search or jump to…",
    "settings.appearanceTitle": "Interface Theme & Customization",
    "settings.preset": "Theme Preset",
    "settings.presetOpenai": "OpenAI Blue",
    "settings.presetOcean": "Ocean Breeze",
    "settings.presetViolet": "Violet Night",
    "settings.fontScale": "Font Scale",
    "settings.scaleCompact": "Compact (90%)",
    "settings.scaleDefault": "Default (100%)",
    "settings.scaleLarge": "Large (110%)",
    "settings.scaleExtra": "Extra Large (120%)",
    "settings.customColors": "Advanced Color Customization",
    "settings.accentColor": "Accent Color",
    "settings.backgroundColor": "Background Color",
    "settings.textColor": "Text Color",
    "settings.resetDefaults": "Reset to Defaults",
    "settings.resetConfirm": "Reset all appearance preferences to default settings?",
    "settings.sidebarToggle": "Toggle Sidebar",
    "settings.workspaceActions": "Workspace & Data",
    "settings.disconnectConfirm": "Disconnect and remove saved AI provider credentials?",
    "settings.customize": "Customize",
    "settings.hideAdvanced": "Hide advanced colors",
    "settings.presetOpenaiHint": "White & Blue",
    "settings.presetOceanHint": "Navy & Cyan",
    "settings.presetVioletHint": "Plum & Purple",
    "settings.pathLabel": "Path: {path}",
    "topbar.subPages": "Workspace pages",
    "topbar.connected": "Connected: {email}",
    "topbar.aiConnected": "AI connected",
    "topbar.accountSettings": "Account and Settings",
    "common.confirm": "Confirm",
    "common.cancel": "Cancel",
    "common.close": "Close",
    "common.save": "Save",
    "common.saved": "Saved successfully",
    "toast.copied": "Copied to clipboard",
    "today.quickAsk": "Ask anything about your study…",
    "today.quickAskButton": "Ask Agent",
    "today.welcomeBack": "Welcome back",
    "agent.collapseContext": "Hide Context",
    "agent.showContext": "Show Context",
    "agent.toggleThreads": "Toggle notebooks",
    "agent.mode": "Agent mode",
    "agent.copyCode": "Copy code",
    "agent.newThread": "New Thread",
    "agent.thread": "Threads",
    "agent.codeCopied": "Code copied to clipboard",
  },
  "zh-CN": {
    "workspace.today": "今日",
    "workspace.agent": "智能助手",
    "workspace.study": "学习",
    "workspace.review": "复习",
    "workspace.insights": "洞察与分析",
    "workspace.library": "资料库",
    "switcher.placeholder": "搜索工作区或页面… (⌘K)",
    "switcher.noResults": "未找到匹配的页面。",
    "switcher.navigation": "页面导航",
    "switcher.hint": "使用 ↑ ↓ 选择，按 Enter 确认，Esc 关闭",
    "switcher.trigger": "搜索或快速跳转…",
    "settings.appearanceTitle": "界面主题与偏好",
    "settings.preset": "主题预设",
    "settings.presetOpenai": "OpenAI 蓝调",
    "settings.presetOcean": "海洋蓝调",
    "settings.presetViolet": "紫罗兰夜",
    "settings.fontScale": "字号比例",
    "settings.scaleCompact": "紧凑 (90%)",
    "settings.scaleDefault": "标准 (100%)",
    "settings.scaleLarge": "放大 (110%)",
    "settings.scaleExtra": "特大 (120%)",
    "settings.customColors": "高级色彩定制",
    "settings.accentColor": "强调色",
    "settings.backgroundColor": "背景色",
    "settings.textColor": "文字色",
    "settings.resetDefaults": "恢复默认设置",
    "settings.resetConfirm": "将所有外观偏好恢复为默认设置？",
    "settings.sidebarToggle": "展开/收起侧栏",
    "settings.workspaceActions": "工作区与数据",
    "settings.disconnectConfirm": "确定要断开连接并移除已保存的 AI 服务凭据吗？",
    "settings.customize": "自定义",
    "settings.hideAdvanced": "收起高级色彩",
    "settings.presetOpenaiHint": "纯白与蓝色",
    "settings.presetOceanHint": "深蓝与青色",
    "settings.presetVioletHint": "梅紫与紫色",
    "settings.pathLabel": "路径：{path}",
    "topbar.subPages": "工作区页面",
    "topbar.connected": "已连接：{email}",
    "topbar.aiConnected": "AI 已连接",
    "topbar.accountSettings": "账户与设置",
    "common.confirm": "确认",
    "common.cancel": "取消",
    "common.close": "关闭",
    "common.save": "保存",
    "common.saved": "保存成功",
    "toast.copied": "已复制到剪贴板",
    "today.quickAsk": "询问任何关于今日学习的事…",
    "today.quickAskButton": "向 Agent 提问",
    "today.welcomeBack": "欢迎回来",
    "agent.collapseContext": "收起上下文",
    "agent.showContext": "展开上下文",
    "agent.toggleThreads": "切换笔记本列表",
    "agent.mode": "Agent 模式",
    "agent.copyCode": "复制代码",
    "agent.newThread": "新建对话",
    "agent.thread": "对话列表",
    "agent.codeCopied": "代码已复制到剪贴板",
  },
  "zh-TW": {
    "workspace.today": "今日",
    "workspace.agent": "智慧助手",
    "workspace.study": "學習",
    "workspace.review": "複習",
    "workspace.insights": "洞察與分析",
    "workspace.library": "資料庫",
    "switcher.placeholder": "搜尋工作區或頁面… (⌘K)",
    "switcher.noResults": "找不到相符的頁面。",
    "switcher.navigation": "頁面導覽",
    "switcher.hint": "使用 ↑ ↓ 選擇，按 Enter 確認，Esc 關閉",
    "switcher.trigger": "搜尋或快速跳轉…",
    "settings.appearanceTitle": "介面主題與偏好",
    "settings.preset": "主題預設",
    "settings.presetOpenai": "OpenAI 藍調",
    "settings.presetOcean": "海洋藍調",
    "settings.presetViolet": "紫羅蘭夜",
    "settings.fontScale": "字體縮放",
    "settings.scaleCompact": "緊湊 (90%)",
    "settings.scaleDefault": "標準 (100%)",
    "settings.scaleLarge": "放大 (110%)",
    "settings.scaleExtra": "特大 (120%)",
    "settings.customColors": "進階色彩自訂",
    "settings.accentColor": "強調色",
    "settings.backgroundColor": "背景色",
    "settings.textColor": "文字色",
    "settings.resetDefaults": "恢復預設值",
    "settings.resetConfirm": "將所有外觀偏好重設為預設值嗎？",
    "settings.sidebarToggle": "展開/摺疊側欄",
    "settings.workspaceActions": "工作區與資料",
    "settings.disconnectConfirm": "確定要中斷連線並移除已儲存的 AI 服務憑據嗎？",
    "settings.customize": "自訂",
    "settings.hideAdvanced": "收起進階色彩",
    "settings.presetOpenaiHint": "純白與藍色",
    "settings.presetOceanHint": "深藍與青色",
    "settings.presetVioletHint": "梅紫與紫色",
    "settings.pathLabel": "路徑：{path}",
    "topbar.subPages": "工作區頁面",
    "topbar.connected": "已連線：{email}",
    "topbar.aiConnected": "AI 已連線",
    "topbar.accountSettings": "帳戶與設定",
    "common.confirm": "確認",
    "common.cancel": "取消",
    "common.close": "關閉",
    "common.save": "儲存",
    "common.saved": "儲存成功",
    "toast.copied": "已複製到剪貼簿",
    "today.quickAsk": "詢問任何關於今日學習的事…",
    "today.quickAskButton": "向 Agent 提問",
    "today.welcomeBack": "歡迎回來",
    "agent.collapseContext": "收起上下文",
    "agent.showContext": "展開上下文",
    "agent.toggleThreads": "切換筆記本列表",
    "agent.mode": "Agent 模式",
    "agent.copyCode": "複製程式碼",
    "agent.newThread": "新增對話",
    "agent.thread": "對話列表",
    "agent.codeCopied": "程式碼已複製到剪貼簿",
  },
  ja: {
    "workspace.today": "今日",
    "workspace.agent": "Agent",
    "workspace.study": "学習",
    "workspace.review": "復習",
    "workspace.insights": "インサイト",
    "workspace.library": "ライブラリ",
    "switcher.placeholder": "ワークスペースやページを検索… (⌘K)",
    "switcher.noResults": "一致するページが見つかりません。",
    "switcher.navigation": "ナビゲーション",
    "switcher.hint": "↑ ↓ で移動、Enter で選択、Esc で閉じる",
    "switcher.trigger": "検索または移動…",
    "settings.appearanceTitle": "外観とカスタマイズ",
    "settings.preset": "テーマプリセット",
    "settings.presetOpenai": "OpenAI ブルー",
    "settings.presetOcean": "オーシャン",
    "settings.presetViolet": "バイオレット",
    "settings.fontScale": "フォント倍率",
    "settings.scaleCompact": "コンパクト (90%)",
    "settings.scaleDefault": "標準 (100%)",
    "settings.scaleLarge": "拡大 (110%)",
    "settings.scaleExtra": "特大 (120%)",
    "settings.customColors": "高度なカラー設定",
    "settings.accentColor": "アクセントカラー",
    "settings.backgroundColor": "背景色",
    "settings.textColor": "テキスト色",
    "settings.resetDefaults": "デフォルトに戻す",
    "settings.resetConfirm": "外観設定をすべてデフォルトに戻しますか？",
    "settings.sidebarToggle": "サイドバーを切り替え",
    "settings.workspaceActions": "ワークスペースとデータ",
    "settings.disconnectConfirm": "接続を解除して保存済みの AI 認証情報を削除しますか？",
    "settings.customize": "カスタマイズ",
    "settings.hideAdvanced": "高度なカラーを隠す",
    "settings.presetOpenaiHint": "ホワイトとブルー",
    "settings.presetOceanHint": "ネイビーとシアン",
    "settings.presetVioletHint": "プラムとパープル",
    "settings.pathLabel": "パス：{path}",
    "topbar.subPages": "ワークスペースのページ",
    "topbar.connected": "接続済み：{email}",
    "topbar.aiConnected": "AI 接続済み",
    "topbar.accountSettings": "アカウントと設定",
    "common.confirm": "確認",
    "common.cancel": "キャンセル",
    "common.close": "閉じる",
    "common.save": "保存",
    "common.saved": "保存しました",
    "toast.copied": "クリップボードにコピーしました",
    "today.quickAsk": "今日の学習について何でも質問…",
    "today.quickAskButton": "Agent に質問",
    "today.welcomeBack": "おかえりなさい",
    "agent.collapseContext": "コンテキストを隠す",
    "agent.showContext": "コンテキストを表示",
    "agent.toggleThreads": "ノート一覧を切り替え",
    "agent.mode": "Agent モード",
    "agent.copyCode": "コードをコピー",
    "agent.newThread": "新しいスレッド",
    "agent.thread": "スレッド一覧",
    "agent.codeCopied": "コードをコピーしました",
  },
  ko: {
    "workspace.today": "오늘",
    "workspace.agent": "Agent",
    "workspace.study": "학습",
    "workspace.review": "복습",
    "workspace.insights": "인사이트",
    "workspace.library": "라이브러리",
    "switcher.placeholder": "워크스페이스 또는 페이지 검색… (⌘K)",
    "switcher.noResults": "일치하는 페이지가 없습니다.",
    "switcher.navigation": "탐색",
    "switcher.hint": "↑ ↓ 이동, Enter 선택, Esc 닫기",
    "switcher.trigger": "검색 또는 이동…",
    "settings.appearanceTitle": "인터페이스 테마 및 설정",
    "settings.preset": "테마 프리셋",
    "settings.presetOpenai": "OpenAI 블루",
    "settings.presetOcean": "오션",
    "settings.presetViolet": "바이올렛",
    "settings.fontScale": "글꼴 크기 비율",
    "settings.scaleCompact": "컴팩트 (90%)",
    "settings.scaleDefault": "기본 (100%)",
    "settings.scaleLarge": "크게 (110%)",
    "settings.scaleExtra": "아주 크게 (120%)",
    "settings.customColors": "고급 색상 설정",
    "settings.accentColor": "강조 색상",
    "settings.backgroundColor": "배경 색상",
    "settings.textColor": "텍스트 색상",
    "settings.resetDefaults": "기본값으로 복원",
    "settings.resetConfirm": "모든 외관 설정을 기본값으로 복원할까요?",
    "settings.sidebarToggle": "사이드바 전환",
    "settings.workspaceActions": "워크스페이스 및 데이터",
    "settings.disconnectConfirm": "연결을 해제하고 저장된 AI 자격 증명을 삭제할까요?",
    "settings.customize": "사용자 지정",
    "settings.hideAdvanced": "고급 색상 숨기기",
    "settings.presetOpenaiHint": "화이트와 블루",
    "settings.presetOceanHint": "네이비와 시안",
    "settings.presetVioletHint": "플럼과 퍼플",
    "settings.pathLabel": "경로: {path}",
    "topbar.subPages": "워크스페이스 페이지",
    "topbar.connected": "연결됨: {email}",
    "topbar.aiConnected": "AI 연결됨",
    "topbar.accountSettings": "계정 및 설정",
    "common.confirm": "확인",
    "common.cancel": "취소",
    "common.close": "닫기",
    "common.save": "저장",
    "common.saved": "저장되었습니다",
    "toast.copied": "클립보드에 복사됨",
    "today.quickAsk": "오늘 학습에 관해 무엇이든 물어보세요…",
    "today.quickAskButton": "Agent에게 질문",
    "today.welcomeBack": "다시 오신 것을 환영합니다",
    "agent.collapseContext": "컨텍스트 숨기기",
    "agent.showContext": "컨텍스트 보기",
    "agent.toggleThreads": "노트 목록 전환",
    "agent.mode": "Agent 모드",
    "agent.copyCode": "코드 복사",
    "agent.newThread": "새 대화",
    "agent.thread": "대화 목록",
    "agent.codeCopied": "코드가 클립보드에 복사되었습니다",
  },
};

const agentSecurityTranslations: Record<Language, Record<string, string>> = {
  en: {
    "agent.localPythonWarning": "Local Python is not a security sandbox. If Docker Runner is not configured, this code runs with your account's host permissions.", "agent.recoverable": "A recoverable Agent turn is available.", "agent.resume": "Resume turn", "agent.result": "Structured result", "agent.questionSet": "Question set", "agent.checkAnswers": "Check answers", "agent.score": "Score", "agent.answer": "Answer", "agent.visualization": "Visualization", "agent.sourcesPanel": "Sources", "agent.artifacts": "Artifacts", "agent.usage": "Usage", "agent.tokens": "{count} tokens", "agent.estimated": "Estimated", "event.observation": "Observation", "event.sources": "Sources collected", "event.result": "Result ready", "event.usage": "Usage recorded", "event.turnRecovered": "Turn recovered",
  },
  "zh-CN": {
    "agent.localPythonWarning": "本机 Python 不是安全沙箱。如果没有配置 Docker Runner，这段代码会以当前用户的主机权限运行。", "agent.recoverable": "有一个可恢复的 Agent 工作。", "agent.resume": "恢复工作", "agent.result": "结构化结果", "agent.questionSet": "题集", "agent.checkAnswers": "检查答案", "agent.score": "得分", "agent.answer": "答案", "agent.visualization": "可视化", "agent.sourcesPanel": "来源", "agent.artifacts": "产物", "agent.usage": "用量", "agent.tokens": "{count} tokens", "agent.estimated": "估算", "event.observation": "观察结果", "event.sources": "已收集来源", "event.result": "结果就绪", "event.usage": "已记录用量", "event.turnRecovered": "工作已恢复",
  },
  "zh-TW": {
    "agent.localPythonWarning": "本機 Python 不是安全沙箱。如果沒有設定 Docker Runner，這段程式碼會以目前使用者的主機權限執行。", "agent.recoverable": "有一個可恢復的 Agent 工作。", "agent.resume": "恢復工作", "agent.result": "結構化結果", "agent.questionSet": "題集", "agent.checkAnswers": "檢查答案", "agent.score": "得分", "agent.answer": "答案", "agent.visualization": "視覺化", "agent.sourcesPanel": "來源", "agent.artifacts": "產物", "agent.usage": "用量", "agent.tokens": "{count} tokens", "agent.estimated": "估算", "event.observation": "觀察結果", "event.sources": "已收集來源", "event.result": "結果就緒", "event.usage": "已記錄用量", "event.turnRecovered": "工作已恢復",
  },
  ja: {
    "agent.localPythonWarning": "ローカル Python は安全なサンドボックスではありません。Docker Runner を設定していない場合、このコードは現在のユーザー権限で実行されます。", "agent.recoverable": "復元できる Agent の作業があります。", "agent.resume": "作業を再開", "agent.result": "構造化された結果", "agent.questionSet": "問題セット", "agent.checkAnswers": "答えを確認", "agent.score": "スコア", "agent.answer": "答え", "agent.visualization": "可視化", "agent.sourcesPanel": "ソース", "agent.artifacts": "成果物", "agent.usage": "使用量", "agent.tokens": "{count} トークン", "agent.estimated": "推定", "event.observation": "観測", "event.sources": "ソースを収集", "event.result": "結果の準備完了", "event.usage": "使用量を記録", "event.turnRecovered": "作業を復元",
  },
  ko: {
    "agent.localPythonWarning": "로컬 Python은 보안 샌드박스가 아닙니다. Docker Runner를 구성하지 않으면 이 코드는 현재 계정의 호스트 권한으로 실행됩니다.", "agent.recoverable": "복구할 수 있는 Agent 작업이 있습니다.", "agent.resume": "작업 재개", "agent.result": "구조화된 결과", "agent.questionSet": "문제 세트", "agent.checkAnswers": "답안 확인", "agent.score": "점수", "agent.answer": "정답", "agent.visualization": "시각화", "agent.sourcesPanel": "출처", "agent.artifacts": "결과물", "agent.usage": "사용량", "agent.tokens": "{count} 토큰", "agent.estimated": "추정", "event.observation": "관찰", "event.sources": "출처 수집됨", "event.result": "결과 준비됨", "event.usage": "사용량 기록됨", "event.turnRecovered": "작업 복구됨",
  },
};

// Merge order matters: base language, style labels, then feature group.
const dictionaries: Record<Language, Record<string, string>> = {
  en: { ...en, ...mistakeEditorTranslations.en, ...mistakeCardTranslations.en, ...mistakeWorkbenchTranslations.en, ...mistakeSessionTranslations.en, ...workbenchAppearanceTranslations.en, ...agentSecurityTranslations.en, ...phase3Translations.en, ...p1En, ...p2En },
  "zh-CN": { ...zhCN, ...mistakeEditorTranslations["zh-CN"], ...mistakeCardTranslations["zh-CN"], ...mistakeWorkbenchTranslations["zh-CN"], ...mistakeSessionTranslations["zh-CN"], ...workbenchAppearanceTranslations["zh-CN"], ...agentSecurityTranslations["zh-CN"], ...phase3Translations["zh-CN"], ...p1ZhCN, ...p2ZhCN },
  "zh-TW": { ...zhTW, ...mistakeEditorTranslations["zh-TW"], ...mistakeCardTranslations["zh-TW"], ...mistakeWorkbenchTranslations["zh-TW"], ...mistakeSessionTranslations["zh-TW"], ...workbenchAppearanceTranslations["zh-TW"], ...agentSecurityTranslations["zh-TW"], ...phase3Translations["zh-TW"], ...p1ZhTW, ...p2ZhCN, "planner.savePlan": "儲存計畫" },
  ja: { ...ja, ...mistakeEditorTranslations.ja, ...mistakeCardTranslations.ja, ...mistakeWorkbenchTranslations.ja, ...mistakeSessionTranslations.ja, ...workbenchAppearanceTranslations.ja, ...agentSecurityTranslations.ja, ...phase3Translations.ja, ...p1Ja, "planner.savePlan": "計画を保存" },
  ko: { ...ko, ...mistakeEditorTranslations.ko, ...mistakeCardTranslations.ko, ...mistakeWorkbenchTranslations.ko, ...mistakeSessionTranslations.ko, ...workbenchAppearanceTranslations.ko, ...agentSecurityTranslations.ko, ...phase3Translations.ko, ...p1Ko, "planner.savePlan": "계획 저장" },
};

function interpolate(template: string, variables?: Record<string, string | number>): string {
  // Only simple word placeholders are supported. A missing variable remains
  // visible as `{name}`, which makes incomplete call sites diagnosable.
  if (!variables) return template;
  return template.replace(/\{(\w+)\}/g, (_, key: string) => String(variables[key] ?? `{${key}}`));
}

export function languageLocale(language: Language): string {
  // Date/report APIs expect the conventional en-US locale while other app
  // languages already use the locale identifier accepted by Intl.
  return language === "en" ? "en-US" : language;
}

export function localizeEnum(t: Translate, prefix: string, value: string): string {
  // Enum labels are optional translations. Returning the original value keeps
  // new Core enum variants readable until a dictionary key is added.
  return t(`${prefix}.${value}`) === `${prefix}.${value}` ? value : t(`${prefix}.${value}`);
}

interface I18nValue {
  language: Language;
  setLanguage: (language: Language) => void;
  t: Translate;
}

const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  // The provider owns language state and persistence; consumers only receive a
  // stable translator that resolves the active dictionary on each language.
  const [language, setLanguageState] = useState<Language>(detectLanguage);
  const setLanguage = (next: Language) => {
    setLanguageState(next);
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, next);
  };
  const value = useMemo<I18nValue>(() => ({
    language,
    setLanguage,
    t: (key, variables) => interpolate(dictionaries[language][key] ?? en[key] ?? key, variables),
  }), [language]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  // Failing at the hook boundary catches incorrectly mounted pages early,
  // rather than producing untranslated keys deep inside a component tree.
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used inside I18nProvider");
  return value;
}
