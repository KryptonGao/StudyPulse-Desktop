// These unions mirror Rust enum spellings at the Tauri boundary. Keep the
// PascalCase values stable even though ordinary record fields use snake_case.
export type TaskType = "Homework" | "Reading";
export type AgentMode =
  | "Chat"
  | "DeepSolve"
  | "Mastery"
  | "DeepResearch"
  | "QuestionLab"
  | "Visualize"
  | "Coach"
  | "ExamSimulation"
  | "ReversePlanner";
export type AgentEventKind =
  | "Started"
  | "StatusChanged"
  | "TextDelta"
  | "ToolRequested"
  | "ToolCompleted"
  | "ConfirmationRequired"
  | "StageStarted"
  | "StageProgress"
  | "StageCompleted"
  | "InputRequired"
  | "ArtifactCreated"
  | "Observation"
  | "Sources"
  | "Result"
  | "Usage"
  | "TurnRecovered"
  | "Failed"
  | "Cancelled"
  | "Completed";
export type RunStatus =
  | "Started"
  | "Running"
  | "WaitingForConfirmation"
  | "Cancelling"
  | "Failed"
  | "Cancelled"
  | "Completed";

// Workspace and provider status are the initial app snapshot. ProviderStatus
// is intentionally a redacted capability view and never contains an API key.
export interface Workspace {
  id: string;
  root_path: string;
  schema_version: number;
}

export interface CloudAccount {
  // Account metadata is safe to display in Settings; model availability is a
  // capability list, not a credential or token payload.
  email: string;
  role: string;
  membership_type: string;
  membership_expires_at: string | null;
  plan_name: string;
  available_models: string[];
}

export interface ByokConfig {
  // BYOK status exposes endpoint/model metadata only. The secret itself stays
  // in the host keyring and is never represented by this interface.
  base_url: string;
  model: string;
}

export interface ProviderStatus {
  // `active_provider` describes selection, while the nullable configs describe
  // what is configured; neither field is an authorization decision by itself.
  cloud_account: CloudAccount | null;
  byok_config: ByokConfig | null;
  has_saved_byok: boolean;
  active_provider: "cloud" | "byok" | null;
}

export interface AppSnapshot {
  workspace: Workspace | null;
  provider: ProviderStatus;
}

export type AiFeatureCaller = "Coach" | "ReversePlanner" | "ExamSimulation" | "MistakeAnalysis" | "Chat" | "HomeAsk" | "StudySuggestions" | "DailyPlan" | "ScorePrediction" | "PredictionDiscussion" | "ExamAutopsy";

export type Phase3RecordKind = "homeAsk" | "suggestions" | "dailyPlans" | "predictions" | "autopsies";

export interface Phase3AppliedAction {
  targetId: string;
  appliedAt: string;
  kind: string;
}

// Phase 3 drafts deliberately remain flexible JSON. Core validates their
// record envelope and only materializes tasks/mistakes after explicit action.
export interface Phase3Record {
  id: string;
  createdAt: string;
  updatedAt: string;
  status: "draft" | "saved" | string;
  payload: Record<string, unknown>;
  appliedActions: Record<string, Phase3AppliedAction>;
}

export interface AiAttachment {
  kind: "image";
  sourcePath?: string;
  dataBase64: string;
  mimeType?: string;
}

export interface AiFeatureDiagnostics {
  request_id: string;
  caller: AiFeatureCaller;
  duration_ms: number;
  cache_hit: boolean;
  stale_result: boolean;
  outcome: "success" | "cache" | "stale" | "failed" | string;
  error_code: string | null;
}

export interface AiFeatureResult<T> {
  schema_version: number;
  request_id: string;
  caller: AiFeatureCaller;
  output: T;
  diagnostics: AiFeatureDiagnostics;
}

export interface MistakeAnalysisOutput {
  question?: string;
  errorReason: string;
  wrongSolution: string;
  correctSolution: string;
  tags: string[];
  confidence: number;
  evidence: string[];
}

export type MistakeQuestionKind = "multipleChoice" | "fillBlank";

export interface MistakePracticeQuestion {
  id: string;
  kind: MistakeQuestionKind;
  prompt: string;
  options: string[];
  answer: string;
  explanation: string;
  concept: string;
  difficulty: number;
}

export interface MistakeQuestionSetOutput {
  questions: MistakePracticeQuestion[];
}

export interface MistakeQuizGradeOutput {
  score: number;
  correctCount: number;
  totalCount: number;
  summary: string;
  results: Array<{ questionId: string; isCorrect: boolean; correctAnswer: string; feedback: string }>;
}

export interface MistakeMindMapOutput {
  title: string;
  nodes: Array<{ id: string; label: string; kind: string; description: string; parentId: string | null }>;
}

export interface MistakeDebateOutput {
  reply: string;
  challenge: string;
  nextQuestion: string;
}

export interface MistakeFaultLineOutput {
  summary: string;
  concepts: Array<{ id: string; name: string; category: string; mastery: number; evidence: string[]; priority: number }>;
  repairTasks: Array<{ id: string; title: string; concept: string; reason: string; durationMinutes: number; importance: number }>;
}

export interface MistakeOcrOutput {
  text: string;
  confidence: number;
}

export interface MistakeAiSession {
  id: string;
  kind: string;
  payload: unknown;
  createdAt: string;
}

// Local records preserve the Rust DTO field names here. `extra_json` carries
// forward-compatible fields that the current frontend does not interpret.
export interface Task {
  // Due/reminder timestamps and completion state are consumed by Today/Tasks;
  // the coach fields link optional AI proposals back to ordinary local tasks.
  id: string;
  title: string;
  task_type: TaskType;
  due_date: string;
  reminder_date: string;
  subject: string;
  importance: number;
  notes: string;
  is_completed: boolean;
  reminder_event_id: string | null;
  reminder_calendar_id: string | null;
  created_at: string;
  phase_id: string | null;
  coach_execution_data: string | null;
  coach_goal_id: string | null;
  coach_proposal_id: string | null;
  extra_json: string;
}

export interface Subject {
  // `display_name` is presentation text while `name` remains the stable Core
  // identity used by grade and trend records.
  id: string;
  name: string;
  enabled: boolean;
  full_score: number;
  display_name: string;
  extra_json: string;
}

// Phase, grade, diary, and SRS models are persisted records. Timestamp fields
// remain ISO strings so the bridge does not introduce JavaScript Date objects.
export interface StudyPhase {
  id: string;
  name: string;
  start_date: string;
  end_date: string;
  is_archived: boolean;
  archived_at: string | null;
  goals: unknown[];
  created_at: string;
  extra_json: string;
}

export interface Grade {
  // Grade ratios are derived from score/full_score by Core; nullable image and
  // ranking fields preserve records that do not have those inputs.
  id: string;
  subject: string;
  score: number;
  raw_score: number | null;
  ranking: number | null;
  importance: number;
  image_base64: string | null;
  image_file_name: string | null;
  date: string;
  exam_name: string;
  exam_id: string | null;
  full_score: number | null;
  phase_id: string | null;
  extra_json: string;
}

export interface DiaryEntry {
  // Diary content is local text. `updated_at` is used as the tie-breaker when
  // multiple entries share the same calendar date.
  id: string;
  date: string;
  mood_score: number;
  energy_score: number;
  energy_tag: string;
  content: string;
  phase_id: string | null;
  created_at: string;
  updated_at: string;
  extra_json: string;
}

export interface ReviewState {
  // These values are the persisted SM-2-compatible state. The next date is a
  // wire timestamp, while the UI only chooses a review quality value.
  repetitions: number;
  ease_factor: number;
  interval_days: number;
  next_review_date: string;
  last_review_date: string | null;
  lapses: number;
  extra_json: string;
}

// A null review_state means a mistake is not enrolled in SRS yet; the UI uses
// that distinction to show the explicit enrollment action.
export interface MistakeNote {
  id: string;
  title: string;
  subject: string;
  original_question: string;
  source: string;
  date: string;
  error_reason: string;
  wrong_solution: string;
  correct_solution: string;
  question_images: string[];
  reason_images: string[];
  wrong_solution_images: string[];
  correct_solution_images: string[];
  review_state: ReviewState | null;
  phase_id: string | null;
  exposure_count: number;
  mastery_score: number;
  mastery_history: unknown[];
  handwriting_history: unknown[];
  difficulty: number;
  tags: string[];
  audio_file_name: string | null;
  extra_json: string;
}

export interface Exam {
  // Exam records retain both display names and scheduling/location fields so
  // older imported workspaces can round-trip fields the page does not edit.
  id: string;
  name: string;
  exam_date: string;
  exam_end_date: string | null;
  importance: number;
  subject: string;
  exam_name: string;
  mastery_degree: number;
  time_slot: unknown;
  phase_id: string | null;
  checklist: unknown[];
  location_school: string;
  location_classroom: string;
  location_seat: string;
  countdown_notify_days: number[] | null;
  exam_review: unknown;
  extra_json: string;
}

export interface ComprehensiveExam {
  // A comprehensive exam references multiple subject names and is refreshed
  // through its own query key in the Exams page.
  id: string;
  name: string;
  exam_date: string;
  exam_end_date: string | null;
  importance: number;
  subject: string[];
  exam_name: string;
  mastery_degree: number;
  subject_time_slots: Record<string, unknown> | null;
  phase_id: string | null;
  extra_json: string;
}

// P2 feature records use the existing camelCase JSON shape because those Core
// commands currently transport JSON strings; do not generalize this group to
// the snake_case DTO records above.
export type CoachGoalStatus = "active" | "paused" | "achieved" | "abandoned";
export type CoachProposalStatus = "pending" | "approved" | "rejected" | "expired" | "superseded";

export interface CoachGoalSubject {
  id: string;
  subject: string;
  baselineScore: number;
  targetScore: number;
  fullScore: number;
  weight: number;
}

export interface CoachGoal {
  // `version` participates in proposal resolution: Core can reject a decision
  // made against an older goal snapshot.
  id: string;
  title: string;
  subjects: CoachGoalSubject[];
  examId: string | null;
  comprehensiveExamId: string | null;
  startDate: string;
  targetDate: string;
  dailyAvailableMinutes: number;
  purpose: string;
  constraints: string;
  status: CoachGoalStatus;
  version: number;
  createdAt: string;
  updatedAt: string;
}

export interface CoachPrediction { subject: string; predicted: number; lowerBound: number; upperBound: number; targetScore: number; confidence: number; sampleSize: number; }
export interface CoachRisk { id: string; title: string; severity: string; detail: string; }
export interface CoachEvidence { source: string; detail: string; }
export interface CoachAnalysis {
  // Analysis contains model output plus a goal version; it is evidence for a
  // proposal, not an instruction that the frontend executes directly.
  id: string; goalId: string; goalVersion: number; calculatedAt: string; decision: string;
  weightedPredicted: number; weightedLowerBound: number; weightedUpperBound: number; successProbability: number;
  predictions: CoachPrediction[]; risks: CoachRisk[]; evidence: CoachEvidence[]; dataFingerprint: string;
}
export interface CoachProposalItem { id: string; title: string; subject: string; startDate: string; objective: string; stopCondition: string; importance: number; }
export interface CoachProposal {
  id: string; goalId: string; goalVersion: number; analysisId: string; conclusion: string; rationale: string;
  items: CoachProposalItem[]; status: CoachProposalStatus; createdAt: string; expiresAt: string;
  resolvedAt: string | null; failureReason: string | null; alternative: string | null;
}
export interface CoachChat { id: string; goalId: string | null; title: string; createdAt: string; updatedAt: string; }
export interface CoachConversationMessage { id: string; chatId: string; role: string; content: string; createdAt: string; todoSuggestions: CoachProposalItem[]; }
export interface CoachData { goals: CoachGoal[]; analyses: CoachAnalysis[]; proposals: CoachProposal[]; chats: CoachChat[]; messages: CoachConversationMessage[]; }

// Reverse-planner and simulation values are feature snapshots. Their status
// fields describe the persisted workflow rather than a React loading state.
export interface ExamGoal { id: string; examName: string; subject: string; examDate: string; currentScore: number; targetScore: number; fullScore: number; phaseId: string | null; createdAt: string; }
export interface ExamWeakPoint { id: string; topic: string; mastery: number; possibleScoreGain: number; priority: number; }
export interface ExamPlanPhase { id: string; name: string; dayRange: string; goal: string; }
export interface DailyExamTask { id: string; dayOffset: number; date: string; subject: string; durationMinutes: number; taskTitle: string; reason: string; }
export interface ExamPlan { id: string; examGoalId: string; improvementTarget: number; summary: string; weakPoints: ExamWeakPoint[]; phases: ExamPlanPhase[]; dailyTasks: DailyExamTask[]; modelInfo: string; createdAt: string; }
// Simulation questions and records are separate so answer behavior can be
// tracked without mutating the generated question text or correct answer.
export type ExamSimulationStatus = "preparing" | "running" | "grading" | "analyzing" | "completed" | "abandoned" | "analysisFailed";
export type ExamQuestionKind = "multipleChoice" | "freeResponse";
export interface ExamQuestion { id: string; kind: ExamQuestionKind; prompt: string; options: string[]; correctAnswer: string | null; explanation: string; points: number; }
export interface ExamQuestionRecord { questionId: string; firstViewedAt: string | null; lastViewedAt: string | null; totalViewSeconds: number; visitCount: number; skipCount: number; answerChangeCount: number; firstAnswer: string | null; finalAnswer: string | null; submittedAt: string | null; isCorrect: boolean | null; score: number | null; }
export type ExamSimulationEventKind = "started" | "questionEntered" | "questionLeft" | "answerChanged" | "skipped" | "submitted" | "timedOut" | "abandoned";
export interface ExamSimulationEvent { id: string; kind: ExamSimulationEventKind; timestamp: string; questionId: string | null; questionIndex: number | null; previousAnswer: string | null; answer: string | null; remainingSeconds: number; }
export interface ExamRoleAnalysis { role: string; confidence: number; evidence: string[]; risk: string; strategies: string[]; isStable: boolean; generatedAt: string; }
export interface ExamSimulation { id: string; subject: string; createdAt: string; startedAt: string | null; endedAt: string | null; durationSeconds: number; status: ExamSimulationStatus; questions: ExamQuestion[]; questionRecords: ExamQuestionRecord[]; events: ExamSimulationEvent[]; totalScore: number | null; analysis: ExamRoleAnalysis | null; lastError: string | null; }

export interface DailyReportPoint { date: string; studyMinutes: number; sessionCount: number; moodScore: number | null; energyScore: number | null; }
export interface LearningReport {
  // Report aggregates are selected by rangeDays and include daily points for
  // export; the page does not derive a second set of statistics from them.
  rangeDays: number; fromDate: string; toDate: string; totalStudyMinutes: number; sessionCount: number; averageSessionMinutes: number;
  subjectDistribution: Record<string, number>; intensityDistribution: Record<string, number>; gradeCount: number; averageScoreRate: number | null;
  mistakeCount: number; examCount: number; topSubject: string | null; weakestSubject: string | null; dailyStudyMinutes: DailyReportPoint[];
  diaryCount: number; averageMoodScore: number | null; averageEnergyScore: number | null;
}

// Library entries and matches contain paths returned by Core; the frontend
// renders them but does not resolve or concatenate filesystem paths itself.
export interface FileEntry {
  relative_path: string;
  is_directory: boolean;
  size_bytes: number;
  modified_at: string | null;
}

export interface SearchMatch {
  // Search results are snippets with source-relative coordinates. They are
  // display data, not permission to resolve arbitrary filesystem paths.
  relative_path: string;
  line_number: number | null;
  snippet: string;
}

export interface TodaySnapshot {
  open_task_count: number;
  completed_task_count: number;
  study_minutes: number;
  due_mistake_count: number;
  due_mistake_ids: string[];
  upcoming_exam_ids: string[];
  streak_days: number;
  assigned_investment_seconds: number;
  suggestions: string[];
}

// These are Core-derived analytics snapshots. Pages display them and choose a
// mode, but do not reimplement the trend, streak, or SRS calculations here.
export interface SrsOverview {
  due_count: number;
  upcoming_count: number;
  total_enrolled: number;
}

export interface DailyTrendPoint {
  // One point represents one calendar day; nullable mood/energy means no diary
  // value was recorded rather than a measured zero.
  date: string;
  study_minutes: number;
  activity_points: number;
  completed_session_count: number;
  review_count: number;
  grade_count: number;
  mood_score: number | null;
  energy_score: number | null;
}

export interface SubjectTrend {
  // The string fallback on trend permits forward-compatible Core labels while
  // preserving the known rising/falling/steady presentation branches.
  subject: string;
  display_name: string;
  average_score_rate: number;
  latest_score_rate: number;
  average_ranking: number | null;
  latest_ranking: number | null;
  grade_count: number;
  mistake_count: number;
  due_mistake_count: number;
  trend: "rising" | "falling" | "steady" | string;
  needs_attention: boolean;
}

export interface TrendsSnapshot {
  start_date: string;
  end_date: string;
  active_days: number;
  current_streak: number;
  total_study_minutes: number;
  average_mood: number | null;
  average_energy: number | null;
  daily_points: DailyTrendPoint[];
  subjects: SubjectTrend[];
  srs: SrsOverview;
}

export interface SrsReviewResult {
  state: ReviewState;
  next_review_date: string;
}

// Timer snapshots represent process-backed state. A study session is a saved
// result and may outlive the in-memory active timer that produced it.
export type TimerStatus = "Idle" | "Running" | "Paused";
export type SessionIntensity = "Peak" | "DeepFocus" | "Steady" | "Light" | "Recovery";

export interface TimerSnapshot {
  // `elapsed_seconds` is authoritative for the active process snapshot; the
  // browser only displays it and sends lifecycle commands back to Core.
  status: TimerStatus;
  session_id: string | null;
  started_at: string | null;
  elapsed_seconds: number;
  target_duration_seconds: number;
  intensity: SessionIntensity | null;
  investment_target: { kind: string; id: string } | null;
}

export interface StudySession {
  // Completed sessions are durable history and may be manual or timer-origin;
  // time_zone_identifier preserves the original local context when available.
  id: string;
  start_date: string;
  duration_seconds: number;
  intensity: SessionIntensity;
  completed: boolean;
  source: "Timer" | "Manual";
  time_zone_identifier: string | null;
}

export interface TimeInvestmentSubject {
  id: string;
  name: string;
  symbol_name: string;
  theme: string;
  start_date: string;
  sort_order: number;
  created_at: string;
  is_archived: boolean;
  extra_json: string;
}

// Notebook messages are local history records; AgentEvent is the live/persisted
// timeline used to stream a run and resolve confirmation/input interactions.
export interface AgentMessage {
  id: string;
  role: "User" | "Assistant";
  content: string;
  created_at: string;
  turn_id?: string | null;
  source_refs_json?: string | null;
  artifact_refs_json?: string | null;
}

export interface AgentNotebook {
  // Source paths scope Agent read tools for this notebook. Messages remain
  // local history and are distinct from the live event timeline.
  id: string;
  title: string;
  source_paths: string[];
  messages: AgentMessage[];
  last_goal: string;
  last_answer: string;
  updated_at: string;
}

export interface CapabilityManifest {
  // Capabilities describe the selectable mode/stage contract; max_loops is a
  // Core safety limit, not a UI pagination setting.
  mode: AgentMode;
  title: string;
  description: string;
  stages: string[];
  max_loops: number;
  tools_used: string[];
  result_kind: string;
  request_schema_json: string;
  config_defaults_json: string;
}

export interface AgentTurn {
  id: string;
  mode: string;
  goal: string;
  status: string;
  stage: string | null;
  loop_index: number;
  last_sequence: number;
  resume_safe: boolean;
  checkpoint: string;
  error: string | null;
  created_at: string;
  updated_at: string;
}

export interface SourceRef {
  source_type: string;
  locator: string;
  title: string | null;
  excerpt: string | null;
  tool_call_id: string | null;
}

export interface ArtifactRef {
  artifact_id: string;
  path: string;
  extension: string;
  render_type: string | null;
}

export interface UsageSummary {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  model_calls: number;
  estimated: boolean;
}

export interface TurnResult {
  schema_version: number;
  mode: AgentMode;
  result_kind: string;
  text: string;
  output_json: string | null;
  sources: SourceRef[];
  artifacts: ArtifactRef[];
  usage: UsageSummary;
}

export interface AgentEvent {
  run_id: string;
  // Sequence is monotonic per event stream and is the polling cursor. It is
  // not an array index and must not be replaced with a batch length.
  sequence: number;
  timestamp: string;
  kind: AgentEventKind;
  status: RunStatus | null;
  text: string | null;
  tool_call_id: string | null;
  tool_name: string | null;
  permission: "Read" | "Write" | "Destructive" | "Execute" | null;
  preview: string | null;
  confirmation_id: string | null;
  payload_json: string | null;
  mode: AgentMode | null;
  stage: string | null;
  progress: number | null;
}

// Backup inspection and import results come from Core’s staged workflow. The
// recovery path and warnings are informational outputs, not frontend writes.
export interface BackupInspection {
  // Inspection counts and conflict descriptors are shown before Replace is
  // applied; the frontend does not merge records itself.
  id: string;
  schema_version: number;
  created_at: string;
  added_records: number;
  identical_records: number;
  conflicts: { key: string; domain: string; record_id: string | null; display_name: string }[];
  warnings: string[];
}

export interface ImportReport {
  imported_records: number;
  kept_local_records: number;
  recovery_path: string;
  warnings: string[];
}

export interface ApiError {
  // Error fields are optional because this is a transport/display shape for
  // serialized failures, not the richer Rust error enum.
  kind?: string;
  message?: string;
}
