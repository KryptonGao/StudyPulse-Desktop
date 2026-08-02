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

export interface Workspace {
  id: string;
  root_path: string;
  schema_version: number;
}

export interface CloudAccount {
  email: string;
  role: string;
  membership_type: string;
  membership_expires_at: string | null;
  plan_name: string;
  available_models: string[];
}

export interface ByokConfig {
  base_url: string;
  model: string;
}

export interface ProviderStatus {
  cloud_account: CloudAccount | null;
  byok_config: ByokConfig | null;
  has_saved_byok: boolean;
  active_provider: "cloud" | "byok" | null;
}

export interface AppSnapshot {
  workspace: Workspace | null;
  provider: ProviderStatus;
}

export interface Task {
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
  id: string;
  name: string;
  enabled: boolean;
  full_score: number;
  display_name: string;
  extra_json: string;
}

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
  repetitions: number;
  ease_factor: number;
  interval_days: number;
  next_review_date: string;
  last_review_date: string | null;
  lapses: number;
  extra_json: string;
}

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

export interface ExamGoal { id: string; examName: string; subject: string; examDate: string; currentScore: number; targetScore: number; fullScore: number; phaseId: string | null; createdAt: string; }
export interface ExamWeakPoint { id: string; topic: string; mastery: number; possibleScoreGain: number; priority: number; }
export interface ExamPlanPhase { id: string; name: string; dayRange: string; goal: string; }
export interface DailyExamTask { id: string; dayOffset: number; date: string; subject: string; durationMinutes: number; taskTitle: string; reason: string; }
export interface ExamPlan { id: string; examGoalId: string; improvementTarget: number; summary: string; weakPoints: ExamWeakPoint[]; phases: ExamPlanPhase[]; dailyTasks: DailyExamTask[]; modelInfo: string; createdAt: string; }
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
  rangeDays: number; fromDate: string; toDate: string; totalStudyMinutes: number; sessionCount: number; averageSessionMinutes: number;
  subjectDistribution: Record<string, number>; intensityDistribution: Record<string, number>; gradeCount: number; averageScoreRate: number | null;
  mistakeCount: number; examCount: number; topSubject: string | null; weakestSubject: string | null; dailyStudyMinutes: DailyReportPoint[];
  diaryCount: number; averageMoodScore: number | null; averageEnergyScore: number | null;
}

export interface FileEntry {
  relative_path: string;
  is_directory: boolean;
  size_bytes: number;
  modified_at: string | null;
}

export interface SearchMatch {
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

export interface SrsOverview {
  due_count: number;
  upcoming_count: number;
  total_enrolled: number;
}

export interface DailyTrendPoint {
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

export type TimerStatus = "Idle" | "Running" | "Paused";
export type SessionIntensity = "Peak" | "DeepFocus" | "Steady" | "Light" | "Recovery";

export interface TimerSnapshot {
  status: TimerStatus;
  session_id: string | null;
  started_at: string | null;
  elapsed_seconds: number;
  target_duration_seconds: number;
  intensity: SessionIntensity | null;
  investment_target: { kind: string; id: string } | null;
}

export interface StudySession {
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

export interface AgentMessage {
  id: string;
  role: "User" | "Assistant";
  content: string;
  created_at: string;
}

export interface AgentNotebook {
  id: string;
  title: string;
  source_paths: string[];
  messages: AgentMessage[];
  last_goal: string;
  last_answer: string;
  updated_at: string;
}

export interface CapabilityManifest {
  mode: AgentMode;
  title: string;
  description: string;
  stages: string[];
  max_loops: number;
}

export interface AgentEvent {
  run_id: string;
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

export interface BackupInspection {
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
  kind?: string;
  message?: string;
}
