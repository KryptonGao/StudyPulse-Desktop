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
  Exam,
  FileEntry,
  Grade,
  ImportReport,
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

export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop) {
    throw new Error("StudyPulse must be opened as the Tauri desktop application.");
  }
  return invoke<T>(name, args);
}

export async function chooseDirectory(title = "Open StudyPulse Workspace"): Promise<string | null> {
  if (!isDesktop) return null;
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}

export async function chooseWorkspaceToCreate(title = "Create StudyPulse Workspace"): Promise<string | null> {
  if (!isDesktop) return null;
  const result = await save({ defaultPath: "StudyPulseWorkspace", title });
  return result ?? null;
}

export async function chooseSourceFiles(title = "Add Notebook Sources"): Promise<string[]> {
  if (!isDesktop) return [];
  const result = await open({ multiple: true, directory: false, title });
  if (!result) return [];
  return Array.isArray(result) ? result : [result];
}

export async function chooseBackupToInspect(title = "Inspect StudyPulse Backup"): Promise<string | null> {
  if (!isDesktop) return null;
  const result = await open({ multiple: false, directory: false, title });
  return typeof result === "string" ? result : null;
}

export async function chooseBackupExportPath(title = "Export StudyPulse Backup"): Promise<string | null> {
  if (!isDesktop) return null;
  return (await save({ defaultPath: "StudyPulse-Backup.studypulsebackup", title })) ?? null;
}

export const core = {
  snapshot: () => command<AppSnapshot>("app_snapshot"),
  createWorkspace: (path: string) => command<Workspace>("create_workspace", { path }),
  openWorkspace: (path: string) => command<Workspace>("open_workspace", { path }),
  closeWorkspace: () => command<void>("close_workspace"),
  loginUrl: () => command<string>("cloud_ai_login_url"),
  completeCloudAuth: (callbackUrl: string) => command<ProviderStatus>("complete_cloud_ai_auth", { callbackUrl }),
  restoreAi: () => command<ProviderStatus>("restore_ai_configuration"),
  saveByok: (input: { apiKey: string; baseUrl: string; model: string }) =>
    command<ProviderStatus>("save_byok_configuration", { input: { api_key: input.apiKey, base_url: input.baseUrl, model: input.model } }),
  disconnectAi: () => command<ProviderStatus>("disconnect_ai"),
  capabilities: () => command<CapabilityManifest[]>("list_agent_capabilities"),
  startAgent: (input: { mode: AgentMode; goal: string; sourcePaths: string[]; history: AgentMessage[] }) =>
    command<string>("start_agent", { input: { mode: input.mode, goal: input.goal, source_paths: input.sourcePaths, history: input.history } }),
  waitAgentEvents: (runId: string, afterSequence: number, timeoutMs = 1000) =>
    command<AgentEvent[]>("wait_agent_events", { runId, afterSequence, timeoutMs }),
  cancelAgent: (runId: string) => command<void>("cancel_agent", { runId }),
  submitConfirmation: (runId: string, confirmationId: string, decision: "Allow" | "Deny") =>
    command<void>("submit_confirmation", { runId, confirmationId, decision }),
  submitAgentInput: (runId: string, inputId: string, answerJson: string) =>
    command<void>("submit_agent_input", { runId, inputId, answerJson }),
  runState: (runId: string) => command<RunStatus>("get_run_state", { runId }),
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
  learningTrends: (rangeDays: number) => command<TrendsSnapshot>("get_learning_trends", { range_days: rangeDays }),
  exams: () => command<Exam[]>("get_exams"),
  upsertExam: (value: Exam) => command<void>("upsert_exam", { value }),
  deleteExam: (id: string) => command<void>("delete_exam", { id }),
  studySessions: () => command<StudySession[]>("get_study_sessions"),
  investmentSubjects: () => command<TimeInvestmentSubject[]>("get_time_investment_subjects"),
  upsertInvestmentSubject: (value: TimeInvestmentSubject) => command<void>("upsert_time_investment_subject", { value }),
  deleteInvestmentSubject: (id: string) => command<void>("delete_time_investment_subject", { id }),
  today: () => command<TodaySnapshot>("get_today_snapshot"),
  timer: () => command<TimerSnapshot>("active_timer"),
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
  exportBackup: (archivePath: string, locale = navigator.language) => command<unknown>("export_backup", {
    options: {
      archive_path: archivePath,
      includes_media: true,
      includes_derived_health_data: true,
      app_version: "0.1.0",
      app_build: "local",
      locale,
    },
  }),
  openExternal: (url: string) => openUrl(url),
};
