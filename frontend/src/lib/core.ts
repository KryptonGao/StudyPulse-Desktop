import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  AgentEvent,
  AgentMessage,
  AgentMode,
  AgentNotebook,
  AppSnapshot,
  BackupInspection,
  DiaryEntry,
  CapabilityManifest,
  CoachAnalysis,
  CoachChat,
  CoachConversationMessage,
  CoachData,
  CoachGoal,
  CoachProposal,
  ComprehensiveExam,
  Exam,
  ExamGoal,
  ExamPlan,
  ExamSimulation,
  FileEntry,
  Grade,
  ImportReport,
  LearningReport,
  MistakeNote,
  ProviderStatus,
  SearchMatch,
  SessionIntensity,
  SrsReviewResult,
  StudyPhase,
  StudySession,
  Subject,
  Task,
  TimeInvestmentSubject,
  TimerSnapshot,
  TodaySnapshot,
  TrendsSnapshot,
  Workspace,
  RunStatus,
} from "../types";

// This is the single runtime feature check for the frontend bridge. Vite’s
// browser preview can render UI, but it must not pretend to have Tauri APIs.
export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  // Keeping the guard before `invoke` makes the non-desktop behavior fail
  // closed and gives tests a stable error contract instead of a plugin error.
  if (!isDesktop) {
    throw new Error("StudyPulse must be opened as the Tauri desktop application.");
  }
  return invoke<T>(name, args);
}

export async function chooseDirectory(title = "Open StudyPulse Workspace"): Promise<string | null> {
  // Dialog helpers deliberately return a neutral value outside Tauri so the
  // welcome and import flows remain renderable in a browser preview.
  if (!isDesktop) return null;
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}

export async function chooseWorkspaceToCreate(title = "Create StudyPulse Workspace"): Promise<string | null> {
  // The save dialog chooses a destination; workspace creation and metadata
  // initialization still happen in the Rust Core command.
  if (!isDesktop) return null;
  const result = await save({ defaultPath: "StudyPulseWorkspace", title });
  return result ?? null;
}

export async function chooseSourceFiles(title = "Add Notebook Sources"): Promise<string[]> {
  // Tauri returns either one path or an array for this dialog. The wrapper
  // normalizes both shapes so callers only handle a list.
  if (!isDesktop) return [];
  const result = await open({ multiple: true, directory: false, title });
  if (!result) return [];
  return Array.isArray(result) ? result : [result];
}

export async function chooseBackupToInspect(title = "Inspect StudyPulse Backup"): Promise<string | null> {
  // Backup selection is only a path choice; inspection and staging are Core
  // operations and are intentionally not reproduced in the frontend.
  if (!isDesktop) return null;
  const result = await open({ multiple: false, directory: false, title });
  return typeof result === "string" ? result : null;
}

export async function chooseBackupExportPath(title = "Export StudyPulse Backup"): Promise<string | null> {
  // Returning null on cancel keeps the caller from invoking an empty-path
  // command and matches the other single-file dialog helpers.
  if (!isDesktop) return null;
  return (await save({ defaultPath: "StudyPulse-Backup.studypulsebackup", title })) ?? null;
}

export async function chooseReportExportPath(defaultPath: string, title = "Export StudyPulse Report"): Promise<string | null> {
  // Report formats share one destination chooser; the extension is supplied by
  // the page and is not inferred or rewritten here.
  if (!isDesktop) return null;
  return (await save({ defaultPath, title })) ?? null;
}

// `core` is a typed facade over Tauri commands. Public method names are
// camelCase for React, while Rust-facing keys are converted only where needed.
export const core = {
  snapshot: () => command<AppSnapshot>("app_snapshot"),
  createWorkspace: (path: string) => command<Workspace>("create_workspace", { path }),
  openWorkspace: (path: string) => command<Workspace>("open_workspace", { path }),
  closeWorkspace: () => command<void>("close_workspace"),
  loginUrl: () => command<string>("cloud_ai_login_url"),
  completeCloudAuth: (callbackUrl: string) => command<ProviderStatus>("complete_cloud_ai_auth", { callbackUrl }),
  restoreAi: () => command<ProviderStatus>("restore_ai_configuration"),
  // The API key is accepted only long enough to cross into the host command;
  // ProviderStatus returned by Core is intentionally redacted.
  saveByok: (input: { apiKey: string; baseUrl: string; model: string }) =>
    command<ProviderStatus>("save_byok_configuration", { input: { api_key: input.apiKey, base_url: input.baseUrl, model: input.model } }),
  disconnectAi: () => command<ProviderStatus>("disconnect_ai"),
  capabilities: () => command<CapabilityManifest[]>("list_agent_capabilities"),
  // Agent source paths and history are serialized once here; the runtime then
  // owns tool permissions, confirmations, and event persistence.
  startAgent: (input: { mode: AgentMode; goal: string; sourcePaths: string[]; history: AgentMessage[] }) =>
    command<string>("start_agent", { input: { mode: input.mode, goal: input.goal, source_paths: input.sourcePaths, history: input.history } }),
  // `afterSequence` is the last observed monotonic event sequence. Core returns
  // only newer events, so callers must advance it with event.sequence.
  waitAgentEvents: (runId: string, afterSequence: number, timeoutMs = 1000) =>
    command<AgentEvent[]>("wait_agent_events", { runId, afterSequence, timeoutMs }),
  cancelAgent: (runId: string) => command<void>("cancel_agent", { runId }),
  submitConfirmation: (runId: string, confirmationId: string, decision: "Allow" | "Deny") =>
    command<void>("submit_confirmation", { runId, confirmationId, decision }),
  submitAgentInput: (runId: string, inputId: string, answerJson: string) =>
    command<void>("submit_agent_input", { runId, inputId, answerJson }),
  runState: (runId: string) => command<RunStatus>("get_run_state", { runId }),
  // Notebook persistence is workspace-scoped, whereas run events are process
  // state; keeping both behind this facade prevents direct filesystem access.
  notebooks: () => command<AgentNotebook[]>("get_agent_notebooks"),
  saveNotebooks: (workspaceId: string, notebooks: AgentNotebook[]) =>
    command<void>("save_agent_notebooks", { workspaceId, notebooks }),
  tasks: () => command<Task[]>("get_tasks"),
  upsertTask: (task: Task) => command<void>("upsert_task", { task }),
  setTaskCompleted: (id: string, completed: boolean) => command<void>("set_task_completed", { id, completed }),
  deleteTask: (id: string) => command<void>("delete_task", { id }),
  subjects: () => command<Subject[]>("get_subjects"),
  upsertSubject: (value: Subject) => command<void>("upsert_subject", { value }),
  deleteSubject: (id: string) => command<void>("delete_subject", { id }),
  phases: () => command<StudyPhase[]>("get_phases"),
  grades: () => command<Grade[]>("get_grades"),
  upsertGrade: (value: Grade) => command<void>("upsert_grade", { value }),
  deleteGrade: (id: string) => command<void>("delete_grade", { id }),
  mistakes: () => command<MistakeNote[]>("get_mistakes"),
  dueMistakes: () => command<MistakeNote[]>("get_due_mistakes"),
  upsertMistake: (value: MistakeNote) => command<void>("upsert_mistake", { value }),
  deleteMistake: (id: string) => command<void>("delete_mistake", { id }),
  reviewMistake: (id: string, quality: number) => command<SrsReviewResult>("review_mistake", { id, quality }),
  enrollMistake: (id: string) => command<unknown>("enroll_mistake", { id }),
  diaryEntries: () => command<DiaryEntry[]>("get_diary_entries"),
  upsertDiaryEntry: (value: DiaryEntry) => command<void>("upsert_diary_entry", { value }),
  deleteDiaryEntry: (id: string) => command<void>("delete_diary_entry", { id }),
  // This snake_case key is an intentional conversion at the Tauri command
  // edge; frontend callers keep the more readable camelCase argument name.
  learningTrends: (rangeDays: number) => command<TrendsSnapshot>("get_learning_trends", { range_days: rangeDays }),
  exams: () => command<Exam[]>("get_exams"),
  upsertExam: (value: Exam) => command<void>("upsert_exam", { value }),
  deleteExam: (id: string) => command<void>("delete_exam", { id }),
  comprehensiveExams: () => command<ComprehensiveExam[]>("get_comprehensive_exams"),
  upsertComprehensiveExam: (value: ComprehensiveExam) => command<void>("upsert_comprehensive_exam", { value }),
  deleteComprehensiveExam: (id: string) => command<void>("delete_comprehensive_exam", { id }),
  // Some legacy/P2 commands expose JSON strings. Parsing remains localized to
  // this adapter so page components receive typed objects consistently.
  coachData: async () => JSON.parse(await command<string>("get_coach_data_json")) as CoachData,
  upsertCoachGoal: (value: CoachGoal) => command<void>("upsert_coach_goal", { value_json: JSON.stringify(value) }),
  upsertCoachAnalysis: (value: CoachAnalysis) => command<void>("upsert_coach_analysis", { value_json: JSON.stringify(value) }),
  upsertCoachProposal: (value: CoachProposal) => command<void>("upsert_coach_proposal", { value_json: JSON.stringify(value) }),
  upsertCoachChat: (value: CoachChat) => command<void>("upsert_coach_chat", { value_json: JSON.stringify(value) }),
  upsertCoachMessage: (value: CoachConversationMessage) => command<void>("upsert_coach_message", { value_json: JSON.stringify(value) }),
  resolveCoachProposal: (proposalId: string, decision: "approve" | "reject", expectedGoalVersion: number) =>
    command<string[]>("resolve_coach_proposal", { proposalId, decision, expectedGoalVersion }),
  deleteCoachGoal: (id: string) => command<void>("delete_coach_goal", { id }),
  // Exam/coach JSON arrays use the same adapter pattern as their single-value
  // counterparts and do not alter the stored JSON payload in the UI.
  examGoals: async (): Promise<ExamGoal[]> => (await command<string[]>("get_exam_goals_json")).map((value) => JSON.parse(value) as ExamGoal),
  upsertExamGoal: (value: ExamGoal) => command<void>("upsert_exam_goal", { value_json: JSON.stringify(value) }),
  deleteExamGoal: (id: string) => command<void>("delete_exam_goal", { id }),
  examPlans: async (): Promise<ExamPlan[]> => (await command<string[]>("get_exam_plans_json")).map((value) => JSON.parse(value) as ExamPlan),
  upsertExamPlan: (value: ExamPlan) => command<void>("upsert_exam_plan", { value_json: JSON.stringify(value) }),
  deleteExamPlan: (id: string) => command<void>("delete_exam_plan", { id }),
  examSimulations: async (): Promise<ExamSimulation[]> => (await command<string[]>("get_exam_simulations_json")).map((value) => JSON.parse(value) as ExamSimulation),
  newExamSimulation: async (subject: string) => JSON.parse(await command<string>("new_exam_simulation", { subject })) as ExamSimulation,
  upsertExamSimulation: (value: ExamSimulation) => command<void>("upsert_exam_simulation", { value_json: JSON.stringify(value) }),
  deleteExamSimulation: (id: string) => command<void>("delete_exam_simulation", { id }),
  learningReport: async (rangeDays: number) => JSON.parse(await command<string>("get_learning_report", { range_days: rangeDays })) as LearningReport,
  writeReportFile: (path: string, extension: "md" | "html", contents: string) => command<void>("write_report_file", { path, extension, contents }),
  writeReportAsset: (path: string, contentsBase64: string) => command<void>("write_report_asset", { path, contents_base64: contentsBase64 }),
  shareReport: (path: string) => command<void>("share_report", { path }),
  studySessions: () => command<StudySession[]>("get_study_sessions"),
  investmentSubjects: () => command<TimeInvestmentSubject[]>("get_time_investment_subjects"),
  upsertInvestmentSubject: (value: TimeInvestmentSubject) => command<void>("upsert_time_investment_subject", { value }),
  deleteInvestmentSubject: (id: string) => command<void>("delete_time_investment_subject", { id }),
  today: () => command<TodaySnapshot>("get_today_snapshot"),
  timer: () => command<TimerSnapshot>("active_timer"),
  // Timer commands expose process-backed snapshots; the UI polls `timer` and
  // does not calculate elapsed seconds from render timing.
  startTimer: (intensity: SessionIntensity, targetDurationSeconds: number) =>
    command<TimerSnapshot>("start_timer", { input: { intensity, target_duration_seconds: targetDurationSeconds, investment_target: null } }),
  pauseTimer: () => command<TimerSnapshot>("pause_timer"),
  resumeTimer: () => command<TimerSnapshot>("resume_timer"),
  finishTimer: () => command<StudySession>("finish_timer"),
  cancelTimer: () => command<void>("cancel_timer"),
  library: () => command<FileEntry[]>("list_library_files"),
  searchLibrary: (query: string) => command<SearchMatch[]>("search_library", { query }),
  importLibraryFile: (path: string) => command<FileEntry>("import_library_file", { path }),
  inspectBackup: (path: string) => command<BackupInspection>("inspect_backup", { path }),
  applyBackup: (inspectionId: string, mode: "Replace" | "Merge") =>
    command<ImportReport>("apply_backup", { inspectionId, mode, resolutions: [] }),
  // Backup options remain explicit because Core owns archive contents,
  // checksums, media limits, and atomic file handling.
  exportBackup: (archivePath: string, locale = navigator.language) => command<unknown>("export_backup", {
    options: {
      archive_path: archivePath,
      includes_media: true,
      includes_derived_health_data: true,
      app_version: "0.4.0",
      app_build: "local",
      locale,
    },
  }),
  // External auth/report links use the opener plugin rather than a browser
  // navigation that would bypass the desktop host boundary.
  openExternal: (url: string) => openUrl(url),
};
