#![cfg_attr(windows, allow(linker_messages))]

mod ai;

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use parking_lot::Mutex;
use studypulse_agent::{
    AgentEvent, AgentEventKind, AgentMode, AgentRuntime, ArtifactRef, CapabilityManifest,
    ConfirmationDecision, ConversationMessage, ConversationRole, RunStatus, SourceRef, TurnRequest,
    TurnResult, UsageSummary, capability_manifests,
};
use studypulse_model_client::{
    ByokConfig, CloudAuthTokens, CloudModelClient, CloudProfile, DEFAULT_CLOUD_API_BASE_URL,
    ModelClient, ModelImageAttachment, OpenAICompatibleModelClient,
};
use studypulse_tools::PermissionLevel;
use studypulse_workspace::{
    AgentMessage, AgentMessageRole, AgentNotebook, AgentTurn as WorkspaceAgentTurn,
    AiActionApplication, AiFeatureRecord, BackupExportOptions, BackupInspection, BackupResolution,
    CoachAnalysis, CoachChat, CoachConversationMessage, CoachGoal, CoachProposal,
    ComprehensiveExamFull, DiaryEntry, DifficultyAnnotation, ExamChecklistItem, ExamFull, ExamGoal,
    ExamPlan, ExamReview, ExamSimulation, ExamTimeSlot, FileEntry, GoalReward, Grade,
    HandwritingAnswerEntry, HeartRateSample, ImportReport, InvestmentTarget, MasteryHistoryEntry,
    MistakeNoteFull, PhaseGoal, RestoreMode, ReviewState, Routine, RoutineInstance, RoutineType,
    SafeRelativePath, SearchMatch, SessionIntensity, StudyPhase, StudySession, StudySessionSource,
    SubTask, Subject, TaskItem, TaskType, TimeInvestmentSubject, TimeInvestmentSummary,
    TimeInvestmentTheme, TodaySnapshot, TrendsSnapshot, Workspace, WorkspaceInfo,
    default_simulation, expired, parse_structured_json, proposal_task,
};
use thiserror::Error;

pub use ai::{AiAttachmentDto, AiFeatureCallerDto, AiFeatureDiagnosticsDto, AiFeatureRequestDto};

// This crate is the single UniFFI facade over the Rust core.  DTOs are the
// stable Swift/desktop boundary; internal Workspace, Agent, and provider types
// never cross it directly.  Conversion code below preserves existing wire
// names, optionality, validation, and `extra_json` compatibility fields.
//
// The facade also owns process-local coordination for Agent runs, timers, and
// backup inspections.  Those state machines are guarded by Mutex/Atomics and
// are intentionally not persisted as Workspace records.
//
// This boundary is deliberately boring: validation and conversion are visible,
// while side effects remain in typed core methods.
// That separation is the main invariant of this crate.
// DTOs stay owned, typed, and explicit.

uniffi::setup_scaffolding!();

// The facade keeps Rust ownership on the core side and exposes only owned
// records, strings, vectors, and enums that UniFFI can generate safely.  No
// method returns a Workspace reference, provider client, mutex, or raw secret;
// those implementation details remain behind this boundary for every client.

// UniFFI exposes one transportable error shape.  Detailed Rust errors are
// converted to a message here so Swift does not need to understand six crate
// error enums, while secret-bearing values remain outside this conversion path.
#[derive(Debug, Error, uniffi::Error)]
pub enum CoreError {
    #[error("{message}")]
    Failure { message: String },
}

impl CoreError {
    // Centralizing the fold keeps every facade method consistent and avoids
    // accidental serialization of an internal error object or credential.
    fn message(error: impl std::fmt::Display) -> Self {
        Self::Failure {
            message: error.to_string(),
        }
    }
}

const MAX_MISTAKE_AI_SESSION_BYTES: usize = 64 * 1024;
const MAX_MISTAKE_AI_SESSIONS: usize = 20;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MistakeAiPatchInput {
    #[serde(default)]
    error_reason: Option<String>,
    #[serde(default)]
    wrong_solution: Option<String>,
    #[serde(default)]
    correct_solution: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    question_images: Option<Vec<String>>,
    #[serde(default)]
    result: Option<serde_json::Value>,
}

fn bounded_ai_text(value: Option<String>, field: &str) -> Result<Option<String>, CoreError> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim().to_owned();
    if value.len() > 32 * 1024 {
        return Err(CoreError::message(format!("{field} is larger than 32 KiB")));
    }
    Ok(Some(value))
}

// WorkspaceDto identifies the opened local store.  It contains location and
// schema metadata only; provider credentials and Agent transcript state are not
// embedded in this record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct WorkspaceDto {
    pub id: String,
    pub root_path: String,
    pub schema_version: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct CloudAuthTokensDto {
    pub access_token: String,
    pub refresh_token: String,
}

// Token DTOs are used only by explicit authentication handoff methods.  The
// desktop Tauri host stores them in the system credential vault and does not
// serialize them into Workspace preferences or ordinary UI state.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct CloudAccountDto {
    pub email: String,
    pub role: String,
    pub membership_type: String,
    pub membership_expires_at: Option<String>,
    pub plan_name: String,
    pub available_models: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ByokConfigDto {
    pub base_url: String,
    pub model: String,
}

// BYOK configuration intentionally contains reconnectable endpoint metadata,
// never the API key.  This distinction is repeated in the conversion below so
// a harmless-looking status method cannot become a secret exfiltration path.

// Simple enums are explicit FFI values rather than Rust domain enums.  The
// conversion implementations later keep Swift spelling stable even if the
// internal model gains fields or variants.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum TaskTypeDto {
    Homework,
    Reading,
}

// Task DTOs retain the front-end snake_case convention and the opaque
// `extra_json` extension point.  TryFrom validates/parses these values before
// any Workspace write; From performs the reverse, lossless projection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct TaskDto {
    pub id: String,
    pub title: String,
    pub task_type: TaskTypeDto,
    pub due_date: String,
    pub reminder_date: String,
    pub subject: String,
    pub importance: u8,
    pub notes: String,
    pub is_completed: bool,
    pub reminder_event_id: Option<String>,
    pub reminder_calendar_id: Option<String>,
    pub created_at: String,
    pub phase_id: Option<String>,
    pub coach_execution_data: Option<String>,
    pub coach_goal_id: Option<String>,
    pub coach_proposal_id: Option<String>,
    pub extra_json: String,
}

// Record DTOs use the frontend's established snake_case field names even though
// the underlying Workspace serde models use camelCase.  The FFI layer is the
// explicit translation point, keeping each client from maintaining its own
// fragile field-name map.

// Subject and phase records form the planning layer.  Nested goals remain DTOs
// so the Swift boundary does not depend on Rust collection or UUID types.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct SubjectDto {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub full_score: f64,
    pub display_name: String,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct PhaseGoalDto {
    pub id: String,
    pub subject: String,
    pub target_score: f64,
    pub notes: String,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct StudyPhaseDto {
    pub id: String,
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub is_archived: bool,
    pub archived_at: Option<String>,
    pub goals: Vec<PhaseGoalDto>,
    pub created_at: String,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct GradeDto {
    pub id: String,
    pub subject: String,
    pub score: f64,
    pub raw_score: Option<f64>,
    pub ranking: Option<i64>,
    pub importance: i64,
    pub image_base64: Option<String>,
    pub image_file_name: Option<String>,
    pub date: String,
    pub exam_name: String,
    pub exam_id: Option<String>,
    pub full_score: Option<f64>,
    pub phase_id: Option<String>,
    pub extra_json: String,
}

// Diary, review, and mistake DTOs carry the learning-history side of the model.
// ReviewState is optional by design: `None` means a mistake is not enrolled in
// the SRS queue, not that its history is malformed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct DiaryEntryDto {
    // Diary values are transported as validated scalar snapshots.  The FFI
    // record preserves the stored date/time strings so all clients observe the
    // same UTC/localization boundary defined by Workspace.
    pub id: String,
    pub date: String,
    pub mood_score: i64,
    pub energy_score: i64,
    pub energy_tag: String,
    pub content: String,
    pub phase_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ReviewStateDto {
    pub repetitions: i64,
    pub ease_factor: f64,
    pub interval_days: i64,
    pub next_review_date: String,
    pub last_review_date: Option<String>,
    pub lapses: i64,
    pub extra_json: String,
}

// History entries are kept as separate records so Swift can render and edit
// them without knowing the storage envelope or serde flattening rules.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct MasteryHistoryEntryDto {
    pub id: String,
    pub timestamp: String,
    pub score: f64,
    pub quality: i64,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct HandwritingAnswerEntryDto {
    pub id: String,
    pub timestamp: String,
    pub image_base64: String,
    pub quality: i64,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct MistakeNoteDto {
    pub id: String,
    pub title: String,
    pub subject: String,
    pub original_question: String,
    pub source: String,
    pub date: String,
    pub error_reason: String,
    pub wrong_solution: String,
    pub correct_solution: String,
    pub question_images: Vec<String>,
    pub reason_images: Vec<String>,
    pub wrong_solution_images: Vec<String>,
    pub correct_solution_images: Vec<String>,
    pub review_state: Option<ReviewStateDto>,
    pub phase_id: Option<String>,
    pub exposure_count: i64,
    pub mastery_score: f64,
    pub mastery_history: Vec<MasteryHistoryEntryDto>,
    pub handwriting_history: Vec<HandwritingAnswerEntryDto>,
    pub difficulty: i64,
    pub tags: Vec<String>,
    pub audio_file_name: Option<String>,
    pub extra_json: String,
}

// Exam DTOs preserve the nested checklist/time-slot/review shape used by the
// Workspace.  Optional fields are compatibility boundaries for older records,
// so conversion must not replace missing values with fabricated data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ExamTimeSlotDto {
    pub start_time: String,
    pub end_time: String,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ExamChecklistItemDto {
    pub id: String,
    pub title: String,
    pub is_checked: bool,
    pub sort_order: i64,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ExamReviewDto {
    pub id: String,
    pub reviewed_at: String,
    pub what_was_tested: String,
    pub what_went_wrong: String,
    pub what_learned: String,
    pub next_strategy: String,
    pub linked_mistake_ids: Vec<String>,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ExamDto {
    // Exam projections keep planning and review data nested but explicit.  This
    // avoids re-parsing an opaque JSON document in Swift and keeps optional
    // checklist/review sections distinguishable from empty values.
    pub id: String,
    pub name: String,
    pub exam_date: String,
    pub exam_end_date: Option<String>,
    pub importance: i64,
    pub subject: String,
    pub exam_name: String,
    pub mastery_degree: i64,
    pub time_slot: Option<ExamTimeSlotDto>,
    pub phase_id: Option<String>,
    pub checklist: Vec<ExamChecklistItemDto>,
    pub location_school: String,
    pub location_classroom: String,
    pub location_seat: String,
    pub countdown_notify_days: Option<Vec<i64>>,
    pub exam_review: Option<ExamReviewDto>,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ComprehensiveExamDto {
    pub id: String,
    pub name: String,
    pub exam_date: String,
    pub exam_end_date: Option<String>,
    pub importance: i64,
    pub subjects: Vec<String>,
    pub exam_name: String,
    pub mastery_degree: i64,
    pub subject_time_slots_json: Option<String>,
    pub phase_id: Option<String>,
    pub extra_json: String,
}

// Routines and instances share an explicit enum plus date keys.  The facade
// passes their validation to Workspace rather than reimplementing persistence
// semantics in Swift.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum RoutineTypeDto {
    MistakeReview,
    Flashcard,
    General,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct RoutineDto {
    pub id: String,
    pub title: String,
    pub routine_type: RoutineTypeDto,
    pub subject: Option<String>,
    pub weekdays: Vec<i64>,
    pub start_time: String,
    pub end_time: String,
    pub enabled: bool,
    pub created_at: String,
    pub phase_id: Option<String>,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct RoutineInstanceDto {
    pub id: String,
    pub routine_id: String,
    pub title: String,
    pub routine_type: RoutineTypeDto,
    pub subject: Option<String>,
    pub start_time: String,
    pub end_time: String,
    pub date: String,
    pub date_key: String,
    pub is_completed: bool,
    pub completed_at: Option<String>,
    pub spawned_mistake_count: i64,
    pub extra_json: String,
}

// Study-session source/intensity values describe provenance and effort, while
// the records below keep optional physiological annotations extensible.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum SessionIntensityDto {
    Peak,
    DeepFocus,
    Steady,
    Light,
    Recovery,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum StudySessionSourceDto {
    Timer,
    Manual,
}

// Time-investment DTOs are intentionally plain records with string ids and
// JSON extras.  Their conversion layer supplies the domain UUID validation and
// prevents a malformed target from reaching aggregate calculations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct InvestmentTargetDto {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct HeartRateSampleDto {
    pub id: String,
    pub timestamp: String,
    pub bpm: f64,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct DifficultyAnnotationDto {
    pub id: String,
    pub timestamp: String,
    pub heart_rate: Option<f64>,
    pub note: String,
    pub subject_id: Option<String>,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct StudySessionDto {
    // Session DTOs retain optional annotations and investment targets because
    // analytics depends on their provenance.  The facade only projects them;
    // it does not infer missing heart-rate or difficulty information.
    pub id: String,
    pub start_date: String,
    pub duration_seconds: i64,
    pub intensity: SessionIntensityDto,
    pub completed: bool,
    pub heart_rate_samples: Option<Vec<HeartRateSampleDto>>,
    pub difficulty_annotations: Option<Vec<DifficultyAnnotationDto>>,
    pub investment_target: Option<InvestmentTargetDto>,
    pub source: StudySessionSourceDto,
    pub time_zone_identifier: Option<String>,
    pub extra_json: String,
}

// Theme and hierarchy enums/records are FFI-safe projections of the investment
// graph.  Parent ids remain optional to represent root subtasks without a
// sentinel UUID.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum TimeInvestmentThemeDto {
    Ocean,
    Coral,
    Violet,
    Sunshine,
    Mint,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct TimeInvestmentSubjectDto {
    pub id: String,
    pub name: String,
    pub symbol_name: String,
    pub theme: TimeInvestmentThemeDto,
    pub start_date: String,
    pub sort_order: i64,
    pub created_at: String,
    pub is_archived: bool,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct SubTaskDto {
    pub id: String,
    pub subject_id: String,
    pub parent_sub_task_id: Option<String>,
    pub name: String,
    pub sort_order: i64,
    pub created_at: String,
    pub is_archived: bool,
    pub extra_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct GoalRewardDto {
    pub id: String,
    pub title: String,
    pub symbol_name: String,
    pub target: InvestmentTargetDto,
    pub threshold_seconds: i64,
    pub created_at: String,
    pub unlocked_at: Option<String>,
    pub extra_json: String,
}

// Aggregate DTOs are read-only analysis snapshots.  They are generated by pure
// Workspace analytics and do not become additional persisted records.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct TimeInvestmentSummaryDto {
    pub target_id: String,
    pub direct_seconds: i64,
    pub total_seconds: i64,
    pub session_count: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct TodaySnapshotDto {
    pub open_task_count: u64,
    pub completed_task_count: u64,
    pub study_minutes: i64,
    pub due_mistake_count: u64,
    pub due_mistake_ids: Vec<String>,
    pub upcoming_exam_ids: Vec<String>,
    pub streak_days: i64,
    pub assigned_investment_seconds: i64,
    pub suggestions: Vec<String>,
}

// SRS/trend results keep dates and counters in transport-friendly scalar types;
// the algorithm and threshold semantics remain in studypulse-workspace.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct SrsReviewResultDto {
    pub state: ReviewStateDto,
    pub next_review_date: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct SrsOverviewDto {
    pub due_count: u64,
    pub upcoming_count: u64,
    pub total_enrolled: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct DailyTrendPointDto {
    pub date: String,
    pub study_minutes: i64,
    pub activity_points: i64,
    pub completed_session_count: u64,
    pub review_count: u64,
    pub grade_count: u64,
    pub mood_score: Option<f64>,
    pub energy_score: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct SubjectTrendDto {
    pub subject: String,
    pub display_name: String,
    pub average_score_rate: f64,
    pub latest_score_rate: f64,
    pub average_ranking: Option<f64>,
    pub latest_ranking: Option<i64>,
    pub grade_count: u64,
    pub mistake_count: u64,
    pub due_mistake_count: u64,
    pub trend: String,
    pub needs_attention: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct TrendsSnapshotDto {
    pub start_date: String,
    pub end_date: String,
    pub active_days: u64,
    pub current_streak: i64,
    pub total_study_minutes: i64,
    pub average_mood: Option<f64>,
    pub average_energy: Option<f64>,
    pub daily_points: Vec<DailyTrendPointDto>,
    pub subjects: Vec<SubjectTrendDto>,
    pub srs: SrsOverviewDto,
}

// Timer status is a process-local state machine.  Its DTO is a snapshot, not a
// command to persist elapsed time; only finish_timer creates a StudySession.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum TimerStatusKindDto {
    Idle,
    Running,
    Paused,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct TimerSnapshotDto {
    pub status: TimerStatusKindDto,
    pub session_id: Option<String>,
    pub started_at: Option<String>,
    pub elapsed_seconds: i64,
    pub target_duration_seconds: i64,
    pub intensity: Option<SessionIntensityDto>,
    pub investment_target: Option<InvestmentTargetDto>,
}

// Backup options/results cross the FFI boundary as metadata and paths.  The
// Workspace backup module remains responsible for archive validation, checksums,
// recovery points, and conflict semantics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct BackupExportOptionsDto {
    // Export options are user intent, not an archive manifest.  Workspace
    // validates the requested path and emits the authoritative schema/checksum
    // metadata after the export actually succeeds.
    pub archive_path: String,
    pub includes_media: bool,
    pub includes_derived_health_data: bool,
    pub app_version: String,
    pub app_build: String,
    pub locale: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct BackupExportResultDto {
    pub archive_path: String,
    pub schema_version: u32,
    pub record_counts_json: String,
    pub warnings: Vec<String>,
}

// `ActiveTimer` is deliberately not a DTO.  `Instant` is monotonic process
// state and cannot be serialized as a meaningful user timestamp; the facade
// converts it to elapsed seconds only when a snapshot is requested.
struct ActiveTimer {
    workspace_id: String,
    session_id: uuid::Uuid,
    started_at: String,
    running_since: Option<std::time::Instant>,
    elapsed_before_pause: i64,
    target_duration_seconds: i64,
    intensity: SessionIntensityDto,
    investment_target: Option<InvestmentTargetDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct FileEntryDto {
    pub relative_path: String,
    pub is_directory: bool,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

// Notebook DTOs are persisted by Workspace but are still exposed as a complete
// FFI graph so Swift can edit source selection and conversation history locally.
// Workspace identity is checked again when notebooks are saved.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum AgentMessageRoleDto {
    User,
    Assistant,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct AgentMessageDto {
    // A message DTO is inert history.  It cannot cause model completion or tool
    // execution merely by being deserialized; those actions require explicit
    // Agent commands and the runtime's permission flow.
    pub id: String,
    pub role: AgentMessageRoleDto,
    pub content: String,
    pub created_at: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub source_refs_json: Option<String>,
    #[serde(default)]
    pub artifact_refs_json: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct AgentNotebookDto {
    pub id: String,
    pub title: String,
    pub source_paths: Vec<String>,
    pub messages: Vec<AgentMessageDto>,
    pub last_goal: String,
    pub last_answer: String,
    pub updated_at: String,
}

// AgentModeDto and CapabilityManifestDto mirror the runtime's mode/stage
// protocol.  They are presentation metadata; execution and loop limits remain
// enforced by AgentRuntime rather than by a Swift caller.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum AgentModeDto {
    Chat,
    DeepSolve,
    Mastery,
    DeepResearch,
    QuestionLab,
    Visualize,
    Coach,
    ExamSimulation,
    ReversePlanner,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct CapabilityManifestDto {
    pub mode: AgentModeDto,
    pub title: String,
    pub description: String,
    pub stages: Vec<String>,
    pub max_loops: u32,
    pub tools_used: Vec<String>,
    pub result_kind: String,
    pub request_schema_json: String,
    pub config_defaults_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct SourceRefDto {
    pub source_type: String,
    pub locator: String,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ArtifactRefDto {
    pub artifact_id: String,
    pub path: String,
    pub extension: String,
    pub render_type: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct UsageSummaryDto {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub model_calls: u32,
    pub estimated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct TurnResultDto {
    pub schema_version: u32,
    pub mode: AgentModeDto,
    pub result_kind: String,
    pub text: String,
    pub output_json: Option<String>,
    pub sources: Vec<SourceRefDto>,
    pub artifacts: Vec<ArtifactRefDto>,
    pub usage: UsageSummaryDto,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct AgentTurnDto {
    pub id: String,
    pub mode: String,
    pub goal: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    pub status: String,
    pub stage: Option<String>,
    pub loop_index: u32,
    pub last_sequence: u64,
    pub resume_safe: bool,
    pub checkpoint: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct TurnRequestDto {
    pub mode: AgentModeDto,
    pub goal: String,
    pub source_paths: Vec<String>,
    pub history: Vec<AgentMessageDto>,
    pub notebook_id: Option<String>,
    pub config_json: Option<String>,
}

// Search results expose only bounded relative paths/snippets.  PermissionDto is
// a host-side risk label, not a capability token that a foreign caller can use
// to bypass prepare or confirmation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct SearchMatchDto {
    pub relative_path: String,
    pub line_number: Option<u32>,
    pub snippet: String,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum PermissionDto {
    Read,
    Write,
    Destructive,
    Execute,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum RunStatusDto {
    Started,
    Running,
    WaitingForConfirmation,
    Cancelling,
    Failed,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum AgentEventKindDto {
    Started,
    StatusChanged,
    TextDelta,
    ToolRequested,
    ToolCompleted,
    ConfirmationRequired,
    StageStarted,
    StageProgress,
    StageCompleted,
    InputRequired,
    ArtifactCreated,
    Observation,
    Sources,
    Result,
    Usage,
    TurnRecovered,
    Failed,
    Cancelled,
    Completed,
}

// Sequence is an exclusive event cursor, not an array index.  Optional fields
// are populated according to kind: text events use text, tool events use
// preview/payload, and terminal events use status/error text.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct AgentEventDto {
    pub run_id: String,
    pub sequence: u64,
    pub timestamp: String,
    pub kind: AgentEventKindDto,
    pub status: Option<RunStatusDto>,
    pub text: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub permission: Option<PermissionDto>,
    pub preview: Option<String>,
    pub confirmation_id: Option<String>,
    pub payload_json: Option<String>,
    pub mode: Option<AgentModeDto>,
    pub stage: Option<String>,
    pub progress: Option<f64>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum ConfirmationDecisionDto {
    Allow,
    Deny,
}

// Backup mode and conflict records are kept explicit across FFI so the host can
// render an inspection before applying it.  `ImportReportDto` carries the
// recovery path returned by Workspace after a restore attempt.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum RestoreModeDto {
    Replace,
    Merge,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct BackupConflictDto {
    pub key: String,
    pub domain: String,
    pub record_id: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct BackupInspectionDto {
    pub id: String,
    pub schema_version: u32,
    pub created_at: String,
    pub added_records: u64,
    pub identical_records: u64,
    pub conflicts: Vec<BackupConflictDto>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct BackupResolutionDto {
    pub conflict_key: String,
    pub use_incoming: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ImportReportDto {
    pub imported_records: u64,
    pub kept_local_records: u64,
    pub recovery_path: String,
    pub warnings: Vec<String>,
}

// Operation events use the same exclusive sequence convention as Agent events,
// but their state is a separate in-process map because backup sessions are not
// Workspace records.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct OperationEventDto {
    pub operation_id: String,
    pub sequence: u64,
    pub kind: String,
    pub progress: f64,
    pub message: String,
}

// Operation events are transport snapshots, not commands.  Their sequence is
// assigned by the process-local operation state machine, and their payload is
// intentionally small enough for polling clients to render without decoding
// Workspace internals.

// StudyPulseCore owns the process boundary visible to Swift.  Workspace and
// provider selection are mutually coordinated here, while Agent/timer/backup
// controls remain in memory.  Timers are bound to the active Workspace, and
// lifecycle-changing operations reject an active timer instead of migrating
// or silently discarding it.
#[derive(uniffi::Object)]
pub struct StudyPulseCore {
    workspace: Mutex<Option<Workspace>>,
    runtime: Mutex<Option<Arc<AgentRuntime>>>,
    model: Mutex<Option<Arc<dyn ModelClient>>>,
    cloud_client: Mutex<Option<CloudModelClient>>,
    cloud_account: Mutex<Option<CloudAccountDto>>,
    byok_client: Mutex<Option<OpenAICompatibleModelClient>>,
    byok_config: Mutex<Option<ByokConfigDto>>,
    cloud_api_base_url: Mutex<String>,
    last_run_id: Mutex<Option<String>>,
    inspections: Mutex<HashMap<String, BackupInspection>>,
    operation_events: Mutex<HashMap<String, Vec<OperationEventDto>>>,
    ai_state: Mutex<ai::AiFeatureState>,
    active_timer: Mutex<Option<ActiveTimer>>,
    restore_active: AtomicBool,
}

// `StudyPulseCore` is a long-lived facade object, but its child state is scoped
// to the current Workspace and provider.  Reinstalling either clears runtime
// identities so a later command cannot accidentally address an old root.

#[uniffi::export]
impl StudyPulseCore {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        // Start disconnected and without a Workspace.  This makes provider and
        // filesystem readiness explicit instead of manufacturing hidden global
        // state during object construction.
        Arc::new(Self {
            workspace: Mutex::new(None),
            runtime: Mutex::new(None),
            model: Mutex::new(None),
            cloud_client: Mutex::new(None),
            cloud_account: Mutex::new(None),
            byok_client: Mutex::new(None),
            byok_config: Mutex::new(None),
            cloud_api_base_url: Mutex::new(DEFAULT_CLOUD_API_BASE_URL.into()),
            last_run_id: Mutex::new(None),
            inspections: Mutex::new(HashMap::new()),
            operation_events: Mutex::new(HashMap::new()),
            ai_state: Mutex::new(ai::AiFeatureState::default()),
            active_timer: Mutex::new(None),
            restore_active: AtomicBool::new(false),
        })
    }

    // Public methods below follow a common lifecycle: validate input at the
    // facade edge, obtain a cloned domain handle, call the typed core method,
    // then project the result into a UniFFI record.  This keeps sync FFI calls
    // predictable while the domain layer retains its own validation rules.

    pub fn create_workspace(&self, path: String) -> Result<WorkspaceDto, CoreError> {
        // Opening/creating a Workspace is blocked while Agent or restore state
        // could still be using the previous store.  Installing the new handle
        // also rebuilds the runtime against the current model, if any.
        self.ensure_no_active_run()?;
        self.ensure_no_active_timer()?;
        let workspace = Workspace::create(path).map_err(CoreError::message)?;
        let dto = workspace.info().into();
        self.install_workspace(workspace)?;
        Ok(dto)
    }

    pub fn open_workspace(&self, path: String) -> Result<WorkspaceDto, CoreError> {
        // `open` validates existing metadata without recreating the directory;
        // the facade only publishes its info after the Workspace accepts it.
        self.ensure_no_active_run()?;
        self.ensure_no_active_timer()?;
        let workspace = Workspace::open(path).map_err(CoreError::message)?;
        let dto = workspace.info().into();
        self.install_workspace(workspace)?;
        Ok(dto)
    }

    pub fn close_workspace(&self) -> Result<(), CoreError> {
        // Closing is a lifecycle transition, so it rejects active restore/Agent
        // work and clears runtime, last-run, and staged-inspection state together.
        if self.restore_active.load(Ordering::Acquire) {
            return Err(CoreError::message(
                "cannot close Workspace while a backup restore is active",
            ));
        }
        if let (Some(runtime), Some(run_id)) = (
            self.runtime.lock().as_ref().cloned(),
            self.last_run_id.lock().clone(),
        ) && runtime.run_status(&run_id).is_ok_and(|status| {
            !matches!(
                status,
                RunStatus::Failed | RunStatus::Cancelled | RunStatus::Completed
            )
        }) {
            return Err(CoreError::message(
                "cannot close Workspace while Agent is running",
            ));
        }
        let mut active_timer = self.active_timer.lock();
        if active_timer.is_some() {
            return Err(CoreError::message(
                "cannot close Workspace while a timer is active",
            ));
        }
        *self.runtime.lock() = None;
        *self.workspace.lock() = None;
        *self.last_run_id.lock() = None;
        self.inspections.lock().clear();
        self.clear_ai_state();
        // Keep the lifecycle reset explicit even though the check above means
        // this is currently always None.  The lock also prevents a timer from
        // starting between the check and Workspace removal.
        *active_timer = None;
        Ok(())
    }

    pub fn current_workspace(&self) -> Option<WorkspaceDto> {
        // This is a metadata snapshot; callers cannot obtain a raw Workspace
        // reference or infer credentials from an absent/present value.
        self.workspace
            .lock()
            .as_ref()
            .map(|value| value.info().into())
    }

    pub fn cloud_ai_login_url(&self) -> Result<String, CoreError> {
        // The callback is fixed to the registered deep-link route so login URLs
        // cannot redirect tokens to an arbitrary scheme or host.
        CloudModelClient::login_url("studypulse://auth/callback").map_err(CoreError::message)
    }

    pub fn parse_cloud_ai_auth_callback(
        &self,
        callback_url: String,
    ) -> Result<CloudAuthTokensDto, CoreError> {
        // Parsing validates scheme, host, path, error parameters, and token
        // prefixes in the model client before tokens become a DTO.
        CloudModelClient::parse_auth_callback(&callback_url)
            .map(Into::into)
            .map_err(CoreError::message)
    }

    pub fn connect_cloud_ai(
        &self,
        access_token: String,
        refresh_token: String,
    ) -> Result<CloudAccountDto, CoreError> {
        // Cloud connection verifies the refresh-token shape, fetches a profile,
        // selects the first available model, and replaces any BYOK client.  The
        // API token remains inside CloudModelClient/secure host storage.
        self.ensure_no_active_run()?;
        self.ensure_no_active_timer()?;
        if !refresh_token.starts_with("sp_refresh_") {
            return Err(CoreError::message("Cloud AI refresh token is invalid"));
        }
        let api_base_url = self.cloud_api_base_url.lock().clone();
        let initial_client = CloudModelClient::new(&api_base_url, access_token.clone(), None)
            .map_err(CoreError::message)?;
        let profile = run_async(initial_client.profile()).map_err(CoreError::message)?;
        let model = profile.available_models.first().cloned();
        let client = CloudModelClient::new(&api_base_url, access_token, model)
            .map_err(CoreError::message)?;
        let account: CloudAccountDto = profile.into();
        let model: Arc<dyn ModelClient> = Arc::new(client.clone());
        self.rebuild_runtime(Arc::clone(&model))?;
        *self.model.lock() = Some(Arc::clone(&model));
        *self.cloud_client.lock() = Some(client);
        *self.cloud_account.lock() = Some(account.clone());
        *self.byok_client.lock() = None;
        *self.byok_config.lock() = None;
        Ok(account)
    }

    pub fn connect_byok(
        &self,
        api_key: String,
        base_url: String,
        model: String,
    ) -> Result<ByokConfigDto, CoreError> {
        // BYOK follows the same runtime rebuild but publishes only base URL and
        // model.  The API key is consumed by the provider client and never enters
        // the returned configuration DTO.
        self.ensure_no_active_run()?;
        self.ensure_no_active_timer()?;
        let client = OpenAICompatibleModelClient::new(&base_url, api_key, model)
            .map_err(CoreError::message)?;
        let config = ByokConfigDto::from(client.config());
        let model: Arc<dyn ModelClient> = Arc::new(client.clone());
        self.rebuild_runtime(Arc::clone(&model))?;
        *self.model.lock() = Some(Arc::clone(&model));
        *self.cloud_client.lock() = None;
        *self.cloud_account.lock() = None;
        *self.byok_client.lock() = Some(client);
        *self.byok_config.lock() = Some(config.clone());
        Ok(config)
    }

    pub fn refresh_cloud_ai(&self, refresh_token: String) -> Result<CloudAuthTokensDto, CoreError> {
        // Refresh is a provider operation, not a Workspace mutation.  The model
        // client accepts compatible response envelopes and validates prefixes.
        self.ensure_no_active_run()?;
        let api_base_url = self.cloud_api_base_url.lock().clone();
        run_async(CloudModelClient::refresh_session(
            &api_base_url,
            &refresh_token,
        ))
        .map(Into::into)
        .map_err(CoreError::message)
    }

    pub fn disconnect_cloud_ai(&self) -> Result<(), CoreError> {
        // Disconnect clears model/runtime state before performing best-effort
        // provider logout, preventing a stale runtime from using old credentials.
        self.ensure_no_active_run()?;
        let client = self.cloud_client.lock().take();
        *self.cloud_account.lock() = None;
        if client.is_some() {
            *self.model.lock() = None;
            *self.runtime.lock() = None;
            *self.last_run_id.lock() = None;
            self.clear_ai_state();
        }
        if let Some(client) = client {
            run_async(client.logout()).map_err(CoreError::message)?;
        }
        Ok(())
    }

    pub fn disconnect_byok(&self) -> Result<(), CoreError> {
        // BYOK disconnect removes the in-memory key-bearing client and its public
        // status view; no key is persisted in this facade state.
        self.ensure_no_active_run()?;
        let client = self.byok_client.lock().take();
        *self.byok_config.lock() = None;
        if client.is_some() {
            *self.model.lock() = None;
            *self.runtime.lock() = None;
            *self.last_run_id.lock() = None;
            self.clear_ai_state();
        }
        Ok(())
    }

    pub fn cloud_ai_account(&self) -> Option<CloudAccountDto> {
        // Account status is a deliberately redacted view of the Cloud profile.
        self.cloud_account.lock().clone()
    }

    pub fn start_agent(
        &self,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<AgentMessageDto>,
    ) -> Result<String, CoreError> {
        // Convert only the history DTOs needed by AgentRuntime, then remember
        // the run id for status/cancellation commands.  Runtime owns validation
        // of the goal, selected sources, and single-active-run rule.
        if self.restore_active.load(Ordering::Acquire) {
            return Err(CoreError::message(
                "cannot start Agent while a backup restore is active",
            ));
        }
        let runtime = self.runtime()?;
        let history = history
            .into_iter()
            .map(|message| ConversationMessage {
                role: match message.role {
                    AgentMessageRoleDto::User => ConversationRole::User,
                    AgentMessageRoleDto::Assistant => ConversationRole::Assistant,
                },
                content: message.content,
            })
            .collect();
        let run_id = runtime
            .start_agent(goal, source_paths, history)
            .map_err(CoreError::message)?;
        *self.last_run_id.lock() = Some(run_id.clone());
        Ok(run_id)
    }

    pub fn start_agent_with_mode(
        &self,
        mode: AgentModeDto,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<AgentMessageDto>,
    ) -> Result<String, CoreError> {
        // Mode conversion is kept at the FFI edge; all event sequencing and
        // confirmation behavior remains shared with the default Agent entry.
        if self.restore_active.load(Ordering::Acquire) {
            return Err(CoreError::message(
                "cannot start Agent while a backup restore is active",
            ));
        }
        let runtime = self.runtime()?;
        let history = history
            .into_iter()
            .map(|message| ConversationMessage {
                role: match message.role {
                    AgentMessageRoleDto::User => ConversationRole::User,
                    AgentMessageRoleDto::Assistant => ConversationRole::Assistant,
                },
                content: message.content,
            })
            .collect();
        let run_id = runtime
            .start_agent_with_mode(mode.into(), goal, source_paths, history)
            .map_err(CoreError::message)?;
        *self.last_run_id.lock() = Some(run_id.clone());
        Ok(run_id)
    }

    pub fn start_turn(&self, request: TurnRequestDto) -> Result<String, CoreError> {
        if self.restore_active.load(Ordering::Acquire) {
            return Err(CoreError::message(
                "cannot start Agent while a backup restore is active",
            ));
        }
        let runtime = self.runtime()?;
        let history = request
            .history
            .into_iter()
            .map(|message| ConversationMessage {
                role: match message.role {
                    AgentMessageRoleDto::User => ConversationRole::User,
                    AgentMessageRoleDto::Assistant => ConversationRole::Assistant,
                },
                content: message.content,
            })
            .collect();
        let run_id = runtime
            .start_turn(TurnRequest {
                mode: request.mode.into(),
                goal: request.goal,
                source_paths: request.source_paths,
                history,
                notebook_id: request.notebook_id,
                config_json: request.config_json,
            })
            .map_err(CoreError::message)?;
        *self.last_run_id.lock() = Some(run_id.clone());
        Ok(run_id)
    }

    pub fn list_agent_turns(&self) -> Result<Vec<AgentTurnDto>, CoreError> {
        Ok(self
            .runtime()?
            .list_turns()
            .map_err(CoreError::message)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn resume_agent_turn(&self, turn_id: String) -> Result<String, CoreError> {
        if self.restore_active.load(Ordering::Acquire) {
            return Err(CoreError::message(
                "cannot resume Agent while a backup restore is active",
            ));
        }
        let run_id = self
            .runtime()?
            .resume_turn(&turn_id)
            .map_err(CoreError::message)?;
        *self.last_run_id.lock() = Some(run_id.clone());
        Ok(run_id)
    }

    pub fn get_agent_turn_result(&self, run_id: String) -> Result<TurnResultDto, CoreError> {
        self.runtime()?
            .turn_result_for_run(&run_id)
            .map(Into::into)
            .map_err(CoreError::message)
    }

    pub fn run_ai_feature_json(&self, request: AiFeatureRequestDto) -> Result<String, CoreError> {
        // A feature caller is a bounded, synchronous facade over the existing
        // event-driven Agent.  The frontend receives one validated result, so
        // prompts and model JSON never need to be interpreted in React.
        let request = self.hydrate_ai_feature_request(request)?;
        let prepared = ai::prepare(request).map_err(CoreError::message)?;
        let started = Instant::now();
        if let Some(output_json) = self.ai_state.lock().fresh(&prepared.cache_key) {
            let diagnostic = AiFeatureDiagnosticsDto {
                request_id: prepared.request_id.clone(),
                caller: prepared.caller,
                duration_ms: started.elapsed().as_millis() as u64,
                cache_hit: true,
                stale_result: false,
                outcome: "cache".into(),
                error_code: None,
            };
            self.ai_state.lock().record(diagnostic.clone());
            return ai::envelope(&prepared, &output_json, diagnostic)
                .map_err(|error| CoreError::message(error.message));
        }

        if self.restore_active.load(Ordering::Acquire) {
            return Err(CoreError::message(
                "cannot start AI feature while a backup restore is active",
            ));
        }
        let runtime = self.runtime()?;
        let history = prepared
            .history
            .clone()
            .into_iter()
            .map(|message| ConversationMessage {
                role: match message.role {
                    AgentMessageRoleDto::User => ConversationRole::User,
                    AgentMessageRoleDto::Assistant => ConversationRole::Assistant,
                },
                content: message.content,
            })
            .collect();
        let attachments = prepared
            .attachments
            .iter()
            .map(|attachment| ModelImageAttachment {
                media_type: attachment.mime_type.clone(),
                data_url: format!(
                    "data:{};base64,{}",
                    attachment.mime_type, attachment.data_base64
                ),
                source_path: attachment.source_path.clone(),
            })
            .collect();
        let run_id = runtime
            .start_feature_with_mode(
                prepared.mode.into(),
                prepared.prompt.clone(),
                prepared.source_paths.clone(),
                history,
                attachments,
            )
            .map_err(|error| CoreError::message(format!("AI feature could not start: {error}")))?;
        *self.last_run_id.lock() = Some(run_id.clone());

        let mut cursor = 0;
        let mut raw = String::new();
        let terminal = loop {
            let events = runtime
                .wait_for_events(&run_id, cursor, 1_000)
                .map_err(|error| {
                    CoreError::message(format!("AI feature event wait failed: {error}"))
                })?;
            let mut terminal = None;
            for event in events {
                cursor = cursor.max(event.sequence);
                match event.kind {
                    AgentEventKind::TextDelta => {
                        if let Some(text) = event.text {
                            raw.push_str(&text);
                        }
                    }
                    // A feature caller never authorizes writes or interactive
                    // questions. Cancel immediately if a provider attempts to
                    // cross that boundary rather than leaving a synchronous IPC
                    // request blocked on a UI response it cannot receive.
                    AgentEventKind::ConfirmationRequired | AgentEventKind::InputRequired => {
                        let _ = runtime.cancel_agent(&run_id);
                    }
                    AgentEventKind::Failed => {
                        terminal = Some(Err(ai::AiFailure {
                            code: "model_error",
                            message: event.text.unwrap_or_else(|| "model request failed".into()),
                        }));
                    }
                    AgentEventKind::Cancelled => {
                        terminal = Some(Err(ai::AiFailure {
                            code: "cancelled",
                            message: "AI feature was cancelled".into(),
                        }));
                    }
                    AgentEventKind::Completed => terminal = Some(Ok(())),
                    _ => {}
                }
                if terminal.is_some() {
                    break;
                }
            }
            if let Some(terminal) = terminal {
                break terminal;
            }
        };

        let output_json = match terminal {
            Ok(()) => match ai::parse_output(&prepared, &raw) {
                Ok(output) => output,
                Err(error) => return self.finish_ai_failure(&prepared, started, error),
            },
            Err(error) => return self.finish_ai_failure(&prepared, started, error),
        };
        self.ai_state
            .lock()
            .store(prepared.cache_key.clone(), output_json.clone());
        let diagnostic = AiFeatureDiagnosticsDto {
            request_id: prepared.request_id.clone(),
            caller: prepared.caller,
            duration_ms: started.elapsed().as_millis() as u64,
            cache_hit: false,
            stale_result: false,
            outcome: "success".into(),
            error_code: None,
        };
        self.ai_state.lock().record(diagnostic.clone());
        ai::envelope(&prepared, &output_json, diagnostic)
            .map_err(|error| CoreError::message(error.message))
    }

    pub fn get_ai_diagnostics_json(&self) -> String {
        // Diagnostics deliberately contain only caller/timing/cache metadata;
        // prompt, model output, credentials, and Workspace contents stay out.
        self.ai_state.lock().diagnostics_json()
    }

    /// Read one of the optional Phase 3 collections.  These remain JSON at the
    /// FFI edge while Workspace validates the shared record envelope.
    pub fn get_phase3_records_json(&self, kind: String) -> Result<Vec<String>, CoreError> {
        let workspace = self.workspace()?;
        let values = match kind.as_str() {
            "homeAsk" => workspace.read_home_ask_sessions(),
            "suggestions" => workspace.read_study_suggestions(),
            "dailyPlans" => workspace.read_daily_ai_plans(),
            "predictions" => workspace.read_score_predictions(),
            "autopsies" => workspace.read_exam_autopsies(),
            _ => return Err(CoreError::message("unknown Phase 3 record collection")),
        }
        .map_err(CoreError::message)?;
        values
            .into_iter()
            .map(|value| serde_json::to_string(&value).map_err(CoreError::message))
            .collect()
    }

    pub fn upsert_phase3_record_json(
        &self,
        kind: String,
        value_json: String,
    ) -> Result<(), CoreError> {
        let value: AiFeatureRecord =
            parse_structured_json(&value_json).map_err(CoreError::message)?;
        let workspace = self.workspace()?;
        match kind.as_str() {
            "homeAsk" => workspace.upsert_home_ask_session(value),
            "suggestions" => workspace.upsert_study_suggestion(value),
            "dailyPlans" => workspace.upsert_daily_ai_plan(value),
            "predictions" => workspace.upsert_score_prediction(value),
            "autopsies" => workspace.upsert_exam_autopsy(value),
            _ => return Err(CoreError::message("unknown Phase 3 record collection")),
        }
        .map_err(CoreError::message)
    }

    pub fn delete_phase3_record(&self, kind: String, id: String) -> Result<(), CoreError> {
        let id = parse_uuid(&id)?;
        let workspace = self.workspace()?;
        match kind.as_str() {
            "homeAsk" => workspace.delete_home_ask_session(id),
            "suggestions" => workspace.delete_study_suggestion(id),
            "dailyPlans" => workspace.delete_daily_ai_plan(id),
            "predictions" => workspace.delete_score_prediction(id),
            "autopsies" => workspace.delete_exam_autopsy(id),
            _ => return Err(CoreError::message("unknown Phase 3 record collection")),
        }
        .map_err(CoreError::message)
    }

    /// Apply selected task drafts exactly once.  The action key is durable and
    /// the generated UUID is derived from record/action identity, so a retry
    /// after an interrupted batch cannot create a duplicate task.
    pub fn apply_phase3_task_actions(
        &self,
        kind: String,
        record_id: String,
        action_ids: Vec<String>,
    ) -> Result<String, CoreError> {
        let id = parse_uuid(&record_id)?;
        let workspace = self.workspace()?;
        let mut values = match kind.as_str() {
            "suggestions" => workspace.read_study_suggestions(),
            "dailyPlans" => workspace.read_daily_ai_plans(),
            "predictions" => workspace.read_score_predictions(),
            _ => {
                return Err(CoreError::message(
                    "this Phase 3 collection has no task actions",
                ));
            }
        }
        .map_err(CoreError::message)?;
        let record = values
            .iter_mut()
            .find(|value| value.id == id)
            .ok_or_else(|| CoreError::message("Phase 3 record not found"))?;
        // Clone the selected drafts before recording applications below.  This
        // keeps the immutable payload lookup separate from mutation of the
        // durable applied-actions map.
        let items = record
            .payload
            .get("items")
            .or_else(|| record.payload.get("recommendations"))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .ok_or_else(|| CoreError::message("Phase 3 record has no task drafts"))?;
        let mut results = Vec::new();
        for action_id in action_ids {
            if let Some(existing) = record.applied_actions.get(&action_id) {
                results.push(serde_json::json!({"actionId": action_id, "targetId": existing.target_id, "alreadyApplied": true}));
                continue;
            }
            let item = items
                .iter()
                .find(|item| {
                    item.get("id").and_then(serde_json::Value::as_str) == Some(action_id.as_str())
                })
                .ok_or_else(|| CoreError::message("selected action is not in this record"))?;
            let task = item
                .get("task")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| CoreError::message("selected action has no task draft"))?;
            let task_id = phase3_action_uuid(id, &action_id);
            let now = chrono::Utc::now().to_rfc3339();
            let value = TaskItem {
                id: task_id,
                title: task
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Study task")
                    .to_owned(),
                task_type: TaskType::Homework,
                due_date: (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339(),
                reminder_date: now.clone(),
                subject: task
                    .get("subject")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                importance: task
                    .get("importance")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(3)
                    .clamp(1, 5) as u8,
                notes: task
                    .get("notes")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                is_completed: false,
                reminder_event_id: None,
                reminder_calendar_id: None,
                created_at: now.clone(),
                phase_id: None,
                coach_execution_data: None,
                coach_goal_id: None,
                coach_proposal_id: None,
                extra: BTreeMap::new(),
            };
            value.validate().map_err(CoreError::message)?;
            workspace.upsert_task(value).map_err(CoreError::message)?;
            record.applied_actions.insert(
                action_id.clone(),
                AiActionApplication {
                    target_id: task_id,
                    applied_at: now,
                    kind: "task".into(),
                    extra: BTreeMap::new(),
                },
            );
            results.push(serde_json::json!({"actionId": action_id, "targetId": task_id, "alreadyApplied": false}));
        }
        record.updated_at = chrono::Utc::now().to_rfc3339();
        match kind.as_str() {
            "suggestions" => workspace.upsert_study_suggestion(record.clone()),
            "dailyPlans" => workspace.upsert_daily_ai_plan(record.clone()),
            "predictions" => workspace.upsert_score_prediction(record.clone()),
            _ => unreachable!(),
        }
        .map_err(CoreError::message)?;
        serde_json::to_string(&results).map_err(CoreError::message)
    }

    /// Materialize selected Exam Autopsy drafts.  Mistake and repair-task
    /// selections have distinct durable action ids, so either can be retried
    /// without duplicating the other after a crash or repeated confirmation.
    pub fn apply_exam_autopsy_actions(
        &self,
        record_id: String,
        mistake_item_ids: Vec<String>,
        task_item_ids: Vec<String>,
    ) -> Result<String, CoreError> {
        let id = parse_uuid(&record_id)?;
        let workspace = self.workspace()?;
        let mut records = workspace
            .read_exam_autopsies()
            .map_err(CoreError::message)?;
        let record = records
            .iter_mut()
            .find(|value| value.id == id)
            .ok_or_else(|| CoreError::message("Exam Autopsy record not found"))?;
        let items = record
            .payload
            .get("items")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .ok_or_else(|| CoreError::message("Exam Autopsy record has no items"))?;
        let images = record
            .payload
            .get("imagePaths")
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut results = Vec::new();
        for (kind, selected) in [("mistake", mistake_item_ids), ("task", task_item_ids)] {
            for item_id in selected {
                let action_id = format!("{kind}:{item_id}");
                if let Some(existing) = record.applied_actions.get(&action_id) {
                    results.push(serde_json::json!({"actionId": action_id, "targetId": existing.target_id, "alreadyApplied": true}));
                    continue;
                }
                let item = items
                    .iter()
                    .find(|value| {
                        value.get("id").and_then(serde_json::Value::as_str)
                            == Some(item_id.as_str())
                    })
                    .ok_or_else(|| {
                        CoreError::message("selected autopsy item is not in this record")
                    })?;
                let now = chrono::Utc::now().to_rfc3339();
                let target_id = phase3_action_uuid(id, &action_id);
                if kind == "mistake" {
                    let value = MistakeNoteFull {
                        id: target_id,
                        title: item
                            .get("questionNumber")
                            .and_then(serde_json::Value::as_str)
                            .filter(|v| !v.is_empty())
                            .map(|v| format!("Exam question {v}"))
                            .unwrap_or_else(|| "Exam Autopsy mistake".into()),
                        subject: record
                            .payload
                            .get("subject")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        original_question: item
                            .get("question")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        source: "Exam Autopsy".into(),
                        date: now.clone(),
                        error_reason: item
                            .get("reason")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned(),
                        wrong_solution: item
                            .get("userAnswer")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        correct_solution: item
                            .get("correctAnswer")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        question_images: images.clone(),
                        reason_images: Vec::new(),
                        wrong_solution_images: Vec::new(),
                        correct_solution_images: Vec::new(),
                        review_state: None,
                        phase_id: None,
                        exposure_count: 0,
                        mastery_score: 0.0,
                        mastery_history: Vec::new(),
                        handwriting_history: Vec::new(),
                        difficulty: 3,
                        tags: item
                            .get("knowledgePoints")
                            .and_then(serde_json::Value::as_array)
                            .map(|values| {
                                values
                                    .iter()
                                    .filter_map(serde_json::Value::as_str)
                                    .map(str::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default(),
                        audio_file_name: None,
                        extra: BTreeMap::new(),
                    };
                    workspace
                        .upsert_mistake(value)
                        .map_err(CoreError::message)?;
                } else {
                    let value = TaskItem {
                        id: target_id,
                        title: item
                            .get("repairSuggestion")
                            .and_then(serde_json::Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or("Review exam mistake")
                            .to_owned(),
                        task_type: TaskType::Homework,
                        due_date: (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339(),
                        reminder_date: now.clone(),
                        subject: record
                            .payload
                            .get("subject")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        importance: 3,
                        notes: item
                            .get("evidence")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        is_completed: false,
                        reminder_event_id: None,
                        reminder_calendar_id: None,
                        created_at: now.clone(),
                        phase_id: None,
                        coach_execution_data: None,
                        coach_goal_id: None,
                        coach_proposal_id: None,
                        extra: BTreeMap::new(),
                    };
                    workspace.upsert_task(value).map_err(CoreError::message)?;
                }
                record.applied_actions.insert(
                    action_id.clone(),
                    AiActionApplication {
                        target_id,
                        applied_at: now,
                        kind: kind.into(),
                        extra: BTreeMap::new(),
                    },
                );
                results.push(serde_json::json!({"actionId": action_id, "targetId": target_id, "alreadyApplied": false}));
            }
        }
        record.updated_at = chrono::Utc::now().to_rfc3339();
        workspace
            .upsert_exam_autopsy(record.clone())
            .map_err(CoreError::message)?;
        serde_json::to_string(&results).map_err(CoreError::message)
    }

    pub fn list_agent_capabilities(&self) -> Vec<CapabilityManifestDto> {
        // Return the runtime manifest as DTOs rather than duplicating stage
        // labels in Swift, keeping loop budgets and labels in one source.
        capability_manifests().into_iter().map(Into::into).collect()
    }

    pub fn cancel_agent(&self, run_id: String) -> Result<(), CoreError> {
        // Cancellation is delegated so the runtime can wake model, confirmation,
        // input, and event waiters as one atomic lifecycle action.
        self.runtime()?
            .cancel_agent(&run_id)
            .map_err(CoreError::message)
    }

    pub fn submit_confirmation(
        &self,
        run_id: String,
        confirmation_id: String,
        decision: ConfirmationDecisionDto,
    ) -> Result<(), CoreError> {
        // The FFI converts the enum but does not make authorization decisions;
        // AgentRuntime matches the one-shot confirmation id and pending state.
        self.runtime()?
            .submit_confirmation(
                &run_id,
                &confirmation_id,
                match decision {
                    ConfirmationDecisionDto::Allow => ConfirmationDecision::Allow,
                    ConfirmationDecisionDto::Deny => ConfirmationDecision::Deny,
                },
            )
            .map_err(CoreError::message)
    }

    pub fn submit_agent_input(
        &self,
        run_id: String,
        input_id: String,
        answer_json: String,
    ) -> Result<(), CoreError> {
        // AgentRuntime enforces the bounded answer size and one-shot input id;
        // the facade only transports the JSON string without interpreting it.
        self.runtime()?
            .submit_input(&run_id, &input_id, answer_json)
            .map_err(CoreError::message)
    }

    pub fn get_run_state(&self, run_id: String) -> Result<RunStatusDto, CoreError> {
        // Status is read from the runtime state machine, not reconstructed from
        // the last event, so transitional Waiting/Cancelling states remain visible.
        self.runtime()?
            .run_status(&run_id)
            .map(Into::into)
            .map_err(CoreError::message)
    }

    pub fn wait_for_agent_events(
        &self,
        run_id: String,
        after_sequence: u64,
        timeout_ms: u32,
    ) -> Result<Vec<AgentEventDto>, CoreError> {
        // `after_sequence` is exclusive and survives batching or reconnects.
        // The timeout is bounded by AgentRuntime, while conversion preserves
        // every event's original sequence and optional payloads.
        self.runtime()?
            .wait_for_events(&run_id, after_sequence, timeout_ms)
            .map(|events| events.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn get_tasks(&self) -> Result<Vec<TaskDto>, CoreError> {
        // CRUD methods are intentionally thin adapters: Workspace owns JSONL
        // envelopes, atomic writes, validation, and UUID identity checks.
        self.workspace()?
            .read_tasks()
            .map(|tasks| tasks.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_task(&self, task: TaskDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_task(task.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_task(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_task(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn set_task_completed(&self, id: String, completed: bool) -> Result<(), CoreError> {
        self.workspace()?
            .set_task_completed(parse_uuid(&id)?, completed)
            .map_err(CoreError::message)
    }

    pub fn get_subjects(&self) -> Result<Vec<SubjectDto>, CoreError> {
        // Subjects and phases follow the same read/try-convert/write pattern;
        // keeping the pattern in the facade avoids a second persistence layer.
        self.workspace()?
            .read_subjects()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_subject(&self, value: SubjectDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_subject(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_subject(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_subject(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_phases(&self) -> Result<Vec<StudyPhaseDto>, CoreError> {
        self.workspace()?
            .read_phases()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_phase(&self, value: StudyPhaseDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_phase(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_phase(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_phase(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_grades(&self) -> Result<Vec<GradeDto>, CoreError> {
        // Grade conversion keeps score/ranking optionals and phase linkage intact
        // while delegating validation and storage format to Workspace.
        self.workspace()?
            .read_grades()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_grade(&self, value: GradeDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_grade(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_grade(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_grade(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_mistakes(&self) -> Result<Vec<MistakeNoteDto>, CoreError> {
        // Mistakes include nested review/history DTOs, so From/TryFrom below is
        // the compatibility boundary rather than ad-hoc JSON in the UI.
        self.workspace()?
            .read_mistakes()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn get_diary_entries(&self) -> Result<Vec<DiaryEntryDto>, CoreError> {
        self.workspace()?
            .read_diary_entries()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_diary_entry(&self, value: DiaryEntryDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_diary_entry(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_diary_entry(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_diary_entry(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_due_mistakes(&self) -> Result<Vec<MistakeNoteDto>, CoreError> {
        // Due selection is computed by the shared analytics function using the
        // current UTC time; the facade does not duplicate SRS date comparisons.
        let values = self
            .workspace()?
            .read_mistakes()
            .map_err(CoreError::message)?;
        let now = chrono::Utc::now();
        Ok(studypulse_workspace::due_mistakes(&values, now)
            .into_iter()
            .cloned()
            .map(Into::into)
            .collect())
    }

    pub fn upsert_mistake(&self, value: MistakeNoteDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_mistake(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn apply_mistake_ai_patch(
        &self,
        id: String,
        patch_json: String,
    ) -> Result<MistakeNoteDto, CoreError> {
        // AI output is applied to the current record, not to the stale editor
        // snapshot. This preserves the latest SRS state and review history.
        if patch_json.len() > MAX_MISTAKE_AI_SESSION_BYTES {
            return Err(CoreError::message("mistake AI patch is too large"));
        }
        let patch: MistakeAiPatchInput = serde_json::from_str(&patch_json)
            .map_err(|error| CoreError::message(format!("mistake AI patch is invalid: {error}")))?;
        let error_reason = bounded_ai_text(patch.error_reason, "error reason")?;
        let wrong_solution = bounded_ai_text(patch.wrong_solution, "wrong solution")?;
        let correct_solution = bounded_ai_text(patch.correct_solution, "correct solution")?;
        let tags = patch.tags.map(|values| {
            values
                .into_iter()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .take(8)
                .collect::<Vec<_>>()
        });
        let question_images = patch.question_images.map(|values| {
            values
                .into_iter()
                .filter(|value| {
                    value.starts_with("images/")
                        && SafeRelativePath::parse(&format!("Media/{value}")).is_ok()
                })
                .take(8)
                .collect::<Vec<_>>()
        });
        if let Some(result) = &patch.result {
            let result_json = serde_json::to_vec(result).map_err(CoreError::message)?;
            if result_json.len() > MAX_MISTAKE_AI_SESSION_BYTES {
                return Err(CoreError::message("mistake AI result is too large"));
            }
        }
        let mistake_id = parse_uuid(&id)?;
        let workspace = self.workspace()?;
        let mut mistakes = workspace.read_mistakes().map_err(CoreError::message)?;
        let mistake = mistakes
            .iter_mut()
            .find(|value| value.id == mistake_id)
            .ok_or_else(|| CoreError::message(format!("mistake UUID not found: {id}")))?;
        if let Some(value) = error_reason {
            mistake.error_reason = value;
        }
        if let Some(value) = wrong_solution {
            mistake.wrong_solution = value;
        }
        if let Some(value) = correct_solution {
            mistake.correct_solution = value;
        }
        if let Some(values) = tags {
            mistake.tags = values;
        }
        if let Some(values) = question_images {
            mistake.question_images = values;
        }
        if let Some(result) = patch.result {
            mistake.extra.insert("studypulseAiAnalysis".into(), result);
        }
        let updated = mistake.clone();
        workspace
            .upsert_mistake(updated.clone())
            .map_err(CoreError::message)?;
        Ok(updated.into())
    }

    pub fn save_mistake_ai_session(
        &self,
        id: String,
        kind: String,
        payload_json: String,
    ) -> Result<(), CoreError> {
        // Generated questions, grading, maps, and debates are persisted as
        // bounded opaque extensions so old clients can still round-trip the
        // parent mistake without a schema migration.
        let kind = kind.trim().to_owned();
        if kind.is_empty()
            || kind.len() > 64
            || !matches!(
                kind.as_str(),
                "analysis"
                    | "similar_questions"
                    | "self_test_generate"
                    | "self_test_grade"
                    | "mind_map"
                    | "debate"
                    | "fault_line"
                    | "image_recognition"
                    | "ocr"
            )
        {
            return Err(CoreError::message("mistake AI session kind is invalid"));
        }
        if payload_json.len() > MAX_MISTAKE_AI_SESSION_BYTES {
            return Err(CoreError::message("mistake AI session is too large"));
        }
        let payload: serde_json::Value = serde_json::from_str(&payload_json).map_err(|error| {
            CoreError::message(format!("mistake AI session is invalid: {error}"))
        })?;
        if !payload.is_object() && !payload.is_array() {
            return Err(CoreError::message(
                "mistake AI session payload must be an object or array",
            ));
        }
        let mistake_id = parse_uuid(&id)?;
        let workspace = self.workspace()?;
        let mut mistakes = workspace.read_mistakes().map_err(CoreError::message)?;
        let mistake = mistakes
            .iter_mut()
            .find(|value| value.id == mistake_id)
            .ok_or_else(|| CoreError::message(format!("mistake UUID not found: {id}")))?;
        let sessions = mistake
            .extra
            .entry("studypulseAiSessions".into())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        let sessions = sessions
            .as_array_mut()
            .ok_or_else(|| CoreError::message("stored mistake AI sessions are malformed"))?;
        sessions.push(serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "kind": kind,
            "payload": payload,
            "createdAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        }));
        if sessions.len() > MAX_MISTAKE_AI_SESSIONS {
            let remove = sessions.len() - MAX_MISTAKE_AI_SESSIONS;
            sessions.drain(..remove);
        }
        workspace
            .upsert_mistake(mistake.clone())
            .map_err(CoreError::message)
    }

    pub fn delete_mistake(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_mistake(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn review_mistake(
        &self,
        id: String,
        quality: i64,
    ) -> Result<SrsReviewResultDto, CoreError> {
        // The accepted quality values mirror iOS and analytics: Again=1,
        // Hard=3, Good=4, Easy=5.  The domain function updates interval/ease,
        // while this facade persists the updated mistake atomically.
        if !matches!(quality, 1 | 3 | 4 | 5) {
            return Err(CoreError::message(
                "review quality must be one of 1, 3, 4, or 5",
            ));
        }
        let workspace = self.workspace()?;
        let mistake_id = parse_uuid(&id)?;
        let mut mistakes = workspace.read_mistakes().map_err(CoreError::message)?;
        let mistake = mistakes
            .iter_mut()
            .find(|value| value.id == mistake_id)
            .ok_or_else(|| CoreError::message(format!("mistake UUID not found: {id}")))?;
        let now = chrono::Utc::now();
        let review = studypulse_workspace::apply_srs(
            mistake.review_state.as_ref(),
            quality,
            mistake.difficulty,
            now,
        );
        let alpha = 1.0 / (mistake.exposure_count.max(0) as f64 + 2.0);
        let target = match quality {
            1 => 0.0,
            3 => 0.45,
            4 => 0.75,
            _ => 1.0,
        };
        mistake.mastery_score =
            (mistake.mastery_score * (1.0 - alpha) + target * alpha).clamp(0.0, 1.0);
        mistake.exposure_count += 1;
        mistake.mastery_history.push(MasteryHistoryEntry {
            id: uuid::Uuid::new_v4(),
            timestamp: now.to_rfc3339(),
            score: mistake.mastery_score,
            quality,
            extra: BTreeMap::new(),
        });
        mistake.review_state = Some(review.state.clone());
        workspace
            .upsert_mistake(mistake.clone())
            .map_err(CoreError::message)?;
        Ok(review.into())
    }

    pub fn enroll_mistake(&self, id: String) -> Result<ReviewStateDto, CoreError> {
        // Enrollment is separate from rating so an unqueued mistake can become
        // due immediately without inventing a review history entry.
        let workspace = self.workspace()?;
        let mistake_id = parse_uuid(&id)?;
        let mut mistakes = workspace.read_mistakes().map_err(CoreError::message)?;
        let mistake = mistakes
            .iter_mut()
            .find(|value| value.id == mistake_id)
            .ok_or_else(|| CoreError::message(format!("mistake UUID not found: {id}")))?;
        if mistake.review_state.is_none() {
            let now = chrono::Utc::now();
            mistake.review_state = Some(ReviewState {
                repetitions: 0,
                ease_factor: 2.5,
                interval_days: 0,
                // Enrolling is an explicit request to make the mistake available
                // in the next flashcard session; the first interval starts after
                // the first rating.
                next_review_date: now.to_rfc3339(),
                last_review_date: None,
                lapses: 0,
                extra: BTreeMap::new(),
            });
            workspace
                .upsert_mistake(mistake.clone())
                .map_err(CoreError::message)?;
        }
        Ok(mistake
            .review_state
            .clone()
            .expect("state was checked")
            .into())
    }

    pub fn get_exams(&self) -> Result<Vec<ExamDto>, CoreError> {
        // Exam and comprehensive-exam DTOs retain nested checklist/review data;
        // storage and future-field compatibility remain Workspace concerns.
        self.workspace()?
            .read_exams()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_exam(&self, value: ExamDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_exam(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_exam(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_exam(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_comprehensive_exams(&self) -> Result<Vec<ComprehensiveExamDto>, CoreError> {
        self.workspace()?
            .read_comprehensive_exams()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_comprehensive_exam(&self, value: ComprehensiveExamDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_comprehensive_exam(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_comprehensive_exam(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_comprehensive_exam(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_coach_data_json(&self) -> Result<String, CoreError> {
        // Coach records remain typed inside Workspace but cross this older FFI
        // surface as structured JSON.  Parsing on every write preserves domain
        // validation without exposing all coach structs as UniFFI records.
        let data = self
            .workspace()?
            .read_coach_data()
            .map_err(CoreError::message)?;
        serde_json::to_string(&serde_json::json!({
            "goals": data.goals,
            "analyses": data.analyses,
            "proposals": data.proposals,
            "chats": data.chats,
            "messages": data.messages,
        }))
        .map_err(CoreError::message)
    }

    pub fn upsert_coach_goal_json(&self, value_json: String) -> Result<(), CoreError> {
        let value: CoachGoal = parse_structured_json(&value_json).map_err(CoreError::message)?;
        self.workspace()?
            .upsert_coach_goal(value)
            .map_err(CoreError::message)
    }

    pub fn upsert_coach_analysis_json(&self, value_json: String) -> Result<(), CoreError> {
        let value: CoachAnalysis =
            parse_structured_json(&value_json).map_err(CoreError::message)?;
        self.workspace()?
            .upsert_coach_analysis(value)
            .map_err(CoreError::message)
    }

    pub fn upsert_coach_proposal_json(&self, value_json: String) -> Result<(), CoreError> {
        let value: CoachProposal =
            parse_structured_json(&value_json).map_err(CoreError::message)?;
        self.workspace()?
            .upsert_coach_proposal(value)
            .map_err(CoreError::message)
    }

    pub fn upsert_coach_chat_json(&self, value_json: String) -> Result<(), CoreError> {
        let value: CoachChat = parse_structured_json(&value_json).map_err(CoreError::message)?;
        self.workspace()?
            .upsert_coach_chat(value)
            .map_err(CoreError::message)
    }

    pub fn upsert_coach_message_json(&self, value_json: String) -> Result<(), CoreError> {
        let value: CoachConversationMessage =
            parse_structured_json(&value_json).map_err(CoreError::message)?;
        self.workspace()?
            .upsert_coach_message(value)
            .map_err(CoreError::message)
    }

    pub fn delete_coach_goal(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_coach_goal(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn resolve_coach_proposal(
        &self,
        proposal_id: String,
        decision: String,
        expected_goal_version: i64,
    ) -> Result<Vec<String>, CoreError> {
        // Proposal resolution is an optimistic-concurrency boundary: goal and
        // proposal versions must match, expiry is persisted, and approval turns
        // validated proposal items into tasks only after all checks pass.
        let workspace = self.workspace()?;
        let mut data = workspace.read_coach_data().map_err(CoreError::message)?;
        let proposal_id = parse_uuid(&proposal_id)?;
        let index = data
            .proposals
            .iter()
            .position(|value| value.id == proposal_id)
            .ok_or_else(|| CoreError::message("coach proposal not found"))?;
        let proposal = data.proposals[index].clone();
        let goal = data
            .goals
            .iter()
            .find(|value| value.id == proposal.goal_id)
            .ok_or_else(|| CoreError::message("coach goal not found"))?;
        if goal.version != expected_goal_version || proposal.goal_version != expected_goal_version {
            return Err(CoreError::message("coach proposal version is stale"));
        }
        if !matches!(
            proposal.status,
            studypulse_workspace::CoachProposalStatus::Pending
        ) {
            return Err(CoreError::message("coach proposal is no longer pending"));
        }
        if expired(&proposal.expires_at) {
            data.proposals[index].status = studypulse_workspace::CoachProposalStatus::Expired;
            data.proposals[index].resolved_at = Some(chrono::Utc::now().to_rfc3339());
            workspace
                .write_coach_data(&data)
                .map_err(CoreError::message)?;
            return Err(CoreError::message("coach proposal has expired"));
        }
        let approved =
            decision.eq_ignore_ascii_case("approve") || decision.eq_ignore_ascii_case("approved");
        let now = chrono::Utc::now().to_rfc3339();
        if approved {
            let tasks: Vec<_> = proposal
                .items
                .iter()
                .map(|item| proposal_task(&proposal, item))
                .collect::<std::result::Result<_, _>>()
                .map_err(CoreError::message)?;
            for task in &tasks {
                task.validate().map_err(CoreError::message)?;
            }
            for task in &tasks {
                workspace
                    .upsert_task(task.clone())
                    .map_err(CoreError::message)?;
            }
            data.proposals[index].status = studypulse_workspace::CoachProposalStatus::Approved;
            data.proposals[index].resolved_at = Some(now);
            workspace
                .write_coach_data(&data)
                .map_err(CoreError::message)?;
            Ok(tasks.into_iter().map(|task| task.id.to_string()).collect())
        } else {
            data.proposals[index].status = studypulse_workspace::CoachProposalStatus::Rejected;
            data.proposals[index].resolved_at = Some(now);
            workspace
                .write_coach_data(&data)
                .map_err(CoreError::message)?;
            Ok(Vec::new())
        }
    }

    pub fn get_exam_goals_json(&self) -> Result<Vec<String>, CoreError> {
        // Coach/exam planning JSON keeps the FFI stable while the domain structs
        // evolve.  Every string still round-trips through serde and Workspace
        // validation before it can be persisted.
        self.workspace()?
            .read_exam_goals()
            .map_err(CoreError::message)?
            .into_iter()
            .map(|value| serde_json::to_string(&value).map_err(CoreError::message))
            .collect()
    }

    pub fn upsert_exam_goal_json(&self, value_json: String) -> Result<(), CoreError> {
        let value: ExamGoal = parse_structured_json(&value_json).map_err(CoreError::message)?;
        self.workspace()?
            .upsert_exam_goal(value)
            .map_err(CoreError::message)
    }

    pub fn delete_exam_goal(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_exam_goal(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_exam_plans_json(&self) -> Result<Vec<String>, CoreError> {
        self.workspace()?
            .read_exam_plans()
            .map_err(CoreError::message)?
            .into_iter()
            .map(|value| serde_json::to_string(&value).map_err(CoreError::message))
            .collect()
    }

    pub fn upsert_exam_plan_json(&self, value_json: String) -> Result<(), CoreError> {
        let value: ExamPlan = parse_structured_json(&value_json).map_err(CoreError::message)?;
        self.workspace()?
            .upsert_exam_plan(value)
            .map_err(CoreError::message)
    }

    pub fn delete_exam_plan(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_exam_plan(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_exam_simulations_json(&self) -> Result<Vec<String>, CoreError> {
        // Simulation helpers use the same typed-inside/JSON-at-edge pattern as
        // goals and plans; default_simulation supplies a deterministic initial
        // value without creating a record until the caller saves it.
        self.workspace()?
            .read_exam_simulations()
            .map_err(CoreError::message)?
            .into_iter()
            .map(|value| serde_json::to_string(&value).map_err(CoreError::message))
            .collect()
    }

    pub fn new_exam_simulation_json(&self, subject: String) -> Result<String, CoreError> {
        serde_json::to_string(&default_simulation(subject, None)).map_err(CoreError::message)
    }

    pub fn upsert_exam_simulation_json(&self, value_json: String) -> Result<(), CoreError> {
        let value: ExamSimulation =
            parse_structured_json(&value_json).map_err(CoreError::message)?;
        self.workspace()?
            .upsert_exam_simulation(value)
            .map_err(CoreError::message)
    }

    pub fn delete_exam_simulation(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_exam_simulation(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_learning_report_json(&self, range_days: i64) -> Result<String, CoreError> {
        // Reports are derived snapshots.  Range clamping and aggregation remain
        // in Workspace so all clients receive identical analytics semantics.
        let report = self
            .workspace()?
            .learning_report(range_days)
            .map_err(CoreError::message)?;
        serde_json::to_string(&report).map_err(CoreError::message)
    }

    pub fn get_routines(&self) -> Result<Vec<RoutineDto>, CoreError> {
        // Routine records and generated instances are separate collections; the
        // facade keeps their CRUD calls distinct so completion cannot mutate the
        // routine definition accidentally.
        self.workspace()?
            .read_routines()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn get_routine_instances(&self) -> Result<Vec<RoutineInstanceDto>, CoreError> {
        self.workspace()?
            .read_routine_instances()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_routine(&self, value: RoutineDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_routine(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn upsert_routine_instance(&self, value: RoutineInstanceDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_routine_instance(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_routine(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_routine(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn delete_routine_instance(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_routine_instance(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_study_sessions(&self) -> Result<Vec<StudySessionDto>, CoreError> {
        self.workspace()?
            .read_study_sessions()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_study_session(&self, value: StudySessionDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_study_session(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_study_session(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_study_session(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_time_investment_subjects(&self) -> Result<Vec<TimeInvestmentSubjectDto>, CoreError> {
        self.workspace()?
            .read_time_investment_subjects()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_time_investment_subject(
        &self,
        value: TimeInvestmentSubjectDto,
    ) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_time_investment_subject(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_time_investment_subject(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_time_investment_subject(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_sub_tasks(&self) -> Result<Vec<SubTaskDto>, CoreError> {
        self.workspace()?
            .read_subtasks()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_sub_task(&self, value: SubTaskDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_subtask(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_sub_task(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_subtask(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_goal_rewards(&self) -> Result<Vec<GoalRewardDto>, CoreError> {
        self.workspace()?
            .read_goal_rewards()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn upsert_goal_reward(&self, value: GoalRewardDto) -> Result<(), CoreError> {
        self.workspace()?
            .upsert_goal_reward(value.try_into()?)
            .map_err(CoreError::message)
    }

    pub fn delete_goal_reward(&self, id: String) -> Result<(), CoreError> {
        self.workspace()?
            .delete_goal_reward(parse_uuid(&id)?)
            .map_err(CoreError::message)
    }

    pub fn get_time_investment_summary(&self) -> Result<Vec<TimeInvestmentSummaryDto>, CoreError> {
        // Investment summaries are computed from subjects, subtasks, and
        // sessions together.  Returning an aggregate DTO avoids duplicating
        // direct/total-second rules in each client.
        let workspace = self.workspace()?;
        let summaries = studypulse_workspace::investment_summary(
            &workspace
                .read_time_investment_subjects()
                .map_err(CoreError::message)?,
            &workspace.read_subtasks().map_err(CoreError::message)?,
            &workspace
                .read_study_sessions()
                .map_err(CoreError::message)?,
        );
        Ok(summaries.into_iter().map(Into::into).collect())
    }

    pub fn get_learning_trends(&self, range_days: i64) -> Result<TrendsSnapshotDto, CoreError> {
        // Clamp/aggregation semantics are owned by learning_trends; this method
        // only gathers the required Workspace collections and projects the
        // result into Swift-safe records.
        let workspace = self.workspace()?;
        let snapshot = studypulse_workspace::learning_trends(
            chrono::Utc::now(),
            range_days.clamp(1, 90) as u32,
            &workspace.read_diary_entries().map_err(CoreError::message)?,
            &workspace.read_grades().map_err(CoreError::message)?,
            &workspace.read_subjects().map_err(CoreError::message)?,
            &workspace.read_mistakes().map_err(CoreError::message)?,
            &workspace
                .read_study_sessions()
                .map_err(CoreError::message)?,
        );
        Ok(snapshot.into())
    }

    pub fn get_today_snapshot(&self) -> Result<TodaySnapshotDto, CoreError> {
        // Today snapshot is another derived view.  It does not write suggestions
        // or counters back to Workspace, so refreshes remain side-effect free.
        let workspace = self.workspace()?;
        let snapshot = studypulse_workspace::today_snapshot(
            chrono::Utc::now(),
            &workspace.read_tasks().map_err(CoreError::message)?,
            &workspace.read_mistakes().map_err(CoreError::message)?,
            &workspace.read_exams().map_err(CoreError::message)?,
            &workspace
                .read_study_sessions()
                .map_err(CoreError::message)?,
            0,
            &workspace.read_phases().map_err(CoreError::message)?,
        );
        Ok(snapshot.into())
    }

    pub fn start_timer(
        &self,
        intensity: SessionIntensityDto,
        target_duration_seconds: i64,
        investment_target: Option<InvestmentTargetDto>,
    ) -> Result<TimerSnapshotDto, CoreError> {
        // Starting a timer reserves process-local state and does not persist a
        // session.  A running Agent is rejected to keep the shared runtime from
        // mixing background work with timer ownership.
        self.ensure_no_active_run()?;
        let mut timer = self.active_timer.lock();
        if timer.is_some() {
            return Err(CoreError::message("a timer is already active"));
        }
        // Take the timer lock before reading Workspace so lifecycle changes
        // cannot remove/swap the Workspace between validation and ownership.
        let workspace = self.workspace()?;
        let workspace_id = workspace.info().id;
        if target_duration_seconds < 0 {
            return Err(CoreError::message("timer duration cannot be negative"));
        }
        let value = ActiveTimer {
            workspace_id,
            session_id: uuid::Uuid::new_v4(),
            started_at: chrono::Utc::now().to_rfc3339(),
            running_since: Some(std::time::Instant::now()),
            elapsed_before_pause: 0,
            target_duration_seconds,
            intensity,
            investment_target,
        };
        *timer = Some(value);
        Ok(timer_snapshot(
            timer.as_ref().expect("timer was just installed"),
        ))
    }

    pub fn pause_timer(&self) -> Result<TimerSnapshotDto, CoreError> {
        // Pause transfers elapsed time from monotonic Instant into the integer
        // accumulator; the timer remains in memory and can be resumed.
        let mut timer = self.active_timer.lock();
        let value = timer
            .as_mut()
            .ok_or_else(|| CoreError::message("no active timer"))?;
        let workspace = self.workspace()?;
        ensure_timer_workspace(value, &workspace)?;
        if let Some(running_since) = value.running_since.take() {
            value.elapsed_before_pause += running_since.elapsed().as_secs() as i64;
        }
        Ok(timer_snapshot(value))
    }

    pub fn resume_timer(&self) -> Result<TimerSnapshotDto, CoreError> {
        // Resume starts a fresh monotonic segment only when currently paused,
        // preventing repeated resume calls from double-counting elapsed time.
        let mut timer = self.active_timer.lock();
        let value = timer
            .as_mut()
            .ok_or_else(|| CoreError::message("no active timer"))?;
        let workspace = self.workspace()?;
        ensure_timer_workspace(value, &workspace)?;
        if value.running_since.is_none() {
            value.running_since = Some(std::time::Instant::now());
        }
        Ok(timer_snapshot(value))
    }

    pub fn finish_timer(&self) -> Result<StudySessionDto, CoreError> {
        // Hold the timer while resolving the Workspace so a lifecycle change
        // cannot redirect this session.  The timer is only removed after the
        // Workspace write succeeds; failed persistence remains safely retryable.
        let mut timer = self.active_timer.lock();
        let value = timer
            .as_ref()
            .ok_or_else(|| CoreError::message("no active timer"))?;
        let workspace = self.workspace()?;
        ensure_timer_workspace(value, &workspace)?;
        let elapsed = elapsed_seconds(value);
        let session = StudySession {
            id: value.session_id,
            start_date: value.started_at.clone(),
            duration_seconds: elapsed,
            intensity: value.intensity.into(),
            completed: true,
            heart_rate_samples: None,
            difficulty_annotations: None,
            investment_target: value
                .investment_target
                .clone()
                .map(TryInto::try_into)
                .transpose()?,
            source: StudySessionSource::Timer,
            time_zone_identifier: Some("UTC".into()),
            extra: BTreeMap::new(),
        };
        workspace
            .upsert_study_session(session.clone())
            .map_err(CoreError::message)?;
        timer.take();
        Ok(session.into())
    }

    pub fn cancel_timer(&self) -> Result<(), CoreError> {
        // Cancel discards only the in-memory timer; unlike finish it must not
        // create a partial StudySession record.
        let mut timer = self.active_timer.lock();
        if timer.take().is_none() {
            return Err(CoreError::message("no active timer"));
        }
        Ok(())
    }

    pub fn active_timer(&self) -> TimerSnapshotDto {
        // Missing state is represented as an explicit Idle snapshot so Swift can
        // render the timer without an optional state machine of its own.
        self.active_timer
            .lock()
            .as_ref()
            .map(timer_snapshot)
            .unwrap_or(TimerSnapshotDto {
                status: TimerStatusKindDto::Idle,
                session_id: None,
                started_at: None,
                elapsed_seconds: 0,
                target_duration_seconds: 0,
                intensity: None,
                investment_target: None,
            })
    }

    pub fn read_media(&self, relative_path: String) -> Result<Vec<u8>, CoreError> {
        // Workspace performs media path and size checks.  Raw bytes are returned
        // here because the Tauri host decides how to encode them for the UI.
        self.workspace()?
            .read_media(&relative_path)
            .map_err(CoreError::message)
    }

    pub fn write_media(
        &self,
        relative_path: String,
        contents: Vec<u8>,
    ) -> Result<String, CoreError> {
        // Media writes remain behind Workspace path/size validation; the FFI
        // does not construct filesystem paths from the wire string itself.
        self.workspace()?
            .write_media(&relative_path, &contents)
            .map_err(CoreError::message)
    }

    pub fn export_backup(
        &self,
        options: BackupExportOptionsDto,
    ) -> Result<BackupExportResultDto, CoreError> {
        // Export delegates archive format, checksums, and media policy to the
        // backup module, then exposes only manifest metadata to the client.
        let result = self
            .workspace()?
            .export_backup(
                &options.archive_path,
                BackupExportOptions {
                    includes_media: options.includes_media,
                    includes_derived_health_data: options.includes_derived_health_data,
                    app_version: options.app_version,
                    app_build: options.app_build,
                    locale: options.locale,
                },
            )
            .map_err(CoreError::message)?;
        Ok(BackupExportResultDto {
            archive_path: result.archive_path,
            schema_version: result.manifest.schema_version,
            record_counts_json: serde_json::to_string(&result.manifest.record_counts)
                .map_err(CoreError::message)?,
            warnings: result.manifest.warnings,
        })
    }

    pub fn list_library_files(&self) -> Result<Vec<FileEntryDto>, CoreError> {
        self.workspace()?
            .list_library_files()
            .map(|files| files.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn import_library_source(
        &self,
        file_name: String,
        contents: Vec<u8>,
    ) -> Result<FileEntryDto, CoreError> {
        self.workspace()?
            .import_library_source(&file_name, contents)
            .map(Into::into)
            .map_err(CoreError::message)
    }

    pub fn get_agent_notebooks(&self) -> Result<Vec<AgentNotebookDto>, CoreError> {
        self.workspace()?
            .read_agent_notebooks()
            .map(|notebooks| notebooks.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn save_agent_notebooks(
        &self,
        workspace_id: String,
        notebooks: Vec<AgentNotebookDto>,
    ) -> Result<(), CoreError> {
        // Workspace identity is compared before writing notebook history so a
        // delayed UI save cannot overwrite data after the user switched stores.
        let workspace = self.workspace()?;
        if workspace.info().id != workspace_id {
            return Err(CoreError::message(
                "Workspace changed before notebooks could be saved",
            ));
        }
        let notebooks = notebooks
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;
        workspace
            .write_agent_notebooks(notebooks)
            .map_err(CoreError::message)
    }

    pub fn search_library(&self, query: String) -> Result<Vec<SearchMatchDto>, CoreError> {
        self.workspace()?
            .search_library(&query)
            .map(|matches| matches.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn inspect_backup(&self, archive_path: String) -> Result<BackupInspectionDto, CoreError> {
        // Inspection stages and caches the validated backup without applying it.
        // The inspection id is then used to correlate operation events and the
        // later apply/cancel command.
        let workspace = self.workspace()?;
        let inspection = workspace
            .inspect_backup(archive_path)
            .map_err(CoreError::message)?;
        let dto = BackupInspectionDto::from(&inspection);
        self.operation_events.lock().insert(
            inspection.id.clone(),
            vec![OperationEventDto {
                operation_id: inspection.id.clone(),
                sequence: 1,
                kind: "inspection_completed".into(),
                progress: 1.0,
                message: "Backup inspection completed".into(),
            }],
        );
        self.inspections
            .lock()
            .insert(inspection.id.clone(), inspection);
        Ok(dto)
    }

    pub fn apply_backup(
        &self,
        inspection_id: String,
        mode: RestoreModeDto,
        resolutions: Vec<BackupResolutionDto>,
    ) -> Result<ImportReportDto, CoreError> {
        // Restore is an exclusive process-local state transition.  The guard is
        // released even on error, while the staged inspection is removed only
        // after Workspace applies it successfully.
        self.ensure_no_active_run()?;
        self.restore_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| CoreError::message("a backup restore is already active"))?;
        let _restore_guard = RestoreFlagGuard(&self.restore_active);
        let inspection = self
            .inspections
            .lock()
            .get(&inspection_id)
            .cloned()
            .ok_or_else(|| CoreError::message("backup inspection was not found"))?;
        let report = self
            .workspace()?
            .apply_backup(
                &inspection,
                match mode {
                    RestoreModeDto::Replace => RestoreMode::Replace,
                    RestoreModeDto::Merge => RestoreMode::Merge,
                },
                &resolutions
                    .into_iter()
                    .map(|resolution| BackupResolution {
                        conflict_key: resolution.conflict_key,
                        use_incoming: resolution.use_incoming,
                    })
                    .collect::<Vec<_>>(),
            )
            .map_err(CoreError::message)?;
        self.operation_events
            .lock()
            .entry(inspection_id.clone())
            .or_default()
            .push(OperationEventDto {
                operation_id: inspection_id.clone(),
                sequence: 2,
                kind: "restore_completed".into(),
                progress: 1.0,
                message: "Backup restore completed".into(),
            });
        self.inspections.lock().remove(&inspection_id);
        Ok(report.into())
    }

    pub fn cancel_backup(&self, inspection_id: String) -> Result<(), CoreError> {
        // Cancel removes the staged inspection and lets Workspace clean its
        // temporary import area without touching current records.
        let inspection = self
            .inspections
            .lock()
            .remove(&inspection_id)
            .ok_or_else(|| CoreError::message("backup inspection was not found"))?;
        self.workspace()?
            .cancel_backup(&inspection)
            .map_err(CoreError::message)
    }

    pub fn wait_for_operation_events(
        &self,
        operation_id: String,
        after_sequence: u64,
        _timeout_ms: u32,
    ) -> Vec<OperationEventDto> {
        // Backup event cursors use the same exclusive `sequence > cursor` rule
        // as Agent events, even though the current operation list is short and
        // in-memory.
        self.operation_events
            .lock()
            .get(&operation_id)
            .into_iter()
            .flatten()
            .filter(|event| event.sequence > after_sequence)
            .cloned()
            .collect()
    }
}

impl StudyPulseCore {
    // Private helpers below are lifecycle guards and projections, not a second
    // service layer.  They centralize invariants that every exported command
    // needs: one Workspace handle, one current model, no active restore, and no
    // stale Agent run id after a provider or Workspace switch.
    fn ensure_no_active_run(&self) -> Result<(), CoreError> {
        // Workspace replacement, provider changes, and restore must not race an
        // Agent.  This guard checks both backup and the runtime's terminal state
        // before any lifecycle-changing command proceeds.
        if self.restore_active.load(Ordering::Acquire) {
            return Err(CoreError::message("a backup restore is currently active"));
        }
        if let (Some(runtime), Some(run_id)) = (
            self.runtime.lock().as_ref().cloned(),
            self.last_run_id.lock().clone(),
        ) && runtime.run_status(&run_id).is_ok_and(|status| {
            !matches!(
                status,
                RunStatus::Failed | RunStatus::Cancelled | RunStatus::Completed
            )
        }) {
            return Err(CoreError::message("an Agent run is currently active"));
        }
        Ok(())
    }

    fn hydrate_ai_feature_request(
        &self,
        mut request: AiFeatureRequestDto,
    ) -> Result<AiFeatureRequestDto, CoreError> {
        use serde_json::{Value, json};
        let supplied: Value = serde_json::from_str(&request.input_json).map_err(|error| {
            CoreError::message(format!("AI feature input is not valid JSON: {error}"))
        })?;
        if !supplied.is_object() {
            return Err(CoreError::message("AI feature input must be an object"));
        }
        if !matches!(
            request.caller,
            AiFeatureCallerDto::HomeAsk
                | AiFeatureCallerDto::StudySuggestions
                | AiFeatureCallerDto::DailyPlan
                | AiFeatureCallerDto::ScorePrediction
                | AiFeatureCallerDto::PredictionDiscussion
                | AiFeatureCallerDto::ExamAutopsy
                | AiFeatureCallerDto::Coach
                | AiFeatureCallerDto::ReversePlanner
                | AiFeatureCallerDto::ExamSimulation
                | AiFeatureCallerDto::Chat
        ) {
            return Ok(request);
        }
        let workspace = self.workspace()?;
        let grades = workspace.read_grades().map_err(CoreError::message)?;
        let mistakes = workspace.read_mistakes().map_err(CoreError::message)?;
        let tasks = workspace.read_tasks().map_err(CoreError::message)?;
        let subjects = workspace.read_subjects().map_err(CoreError::message)?;
        let report = workspace.learning_report(30).map_err(CoreError::message)?;
        let evidence = ai_evidence(&grades, &mistakes, &tasks);
        let allowed: Vec<String> = evidence
            .iter()
            .filter_map(|value| value.get("key").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        let common = json!({
            "allowedEvidenceKeys": allowed,
            "evidence": evidence,
            "grades": grades.iter().rev().take(20).map(|value| json!({"id":value.id,"subject":value.subject,"score":value.score,"fullScore":value.full_score,"date":value.date,"examId":value.exam_id,"examName":value.exam_name})).collect::<Vec<_>>(),
            "mistakes": mistakes.iter().take(20).map(|value| json!({"id":value.id,"subject":value.subject,"title":value.title,"mastery":value.mastery_score,"difficulty":value.difficulty,"tags":value.tags})).collect::<Vec<_>>(),
            "openTasks": tasks.iter().filter(|value| !value.is_completed).take(20).map(|value| json!({"id":value.id,"title":value.title,"subject":value.subject,"importance":value.importance,"dueDate":value.due_date})).collect::<Vec<_>>(),
            "trends": {"studyMinutes":report.total_study_minutes,"sessionCount":report.session_count,"averageScoreRate":report.average_score_rate,"weakestSubject":report.weakest_subject,"mistakeCount":report.mistake_count}
        });
        let input = match request.caller {
            AiFeatureCallerDto::Coach => {
                json!({"goal": supplied.get("goal").cloned().unwrap_or(Value::Null), "context": common})
            }
            AiFeatureCallerDto::ReversePlanner => {
                json!({"goal": supplied.get("goal").cloned().unwrap_or(Value::Null), "context": common})
            }
            AiFeatureCallerDto::ExamSimulation => {
                let mut value = supplied.as_object().cloned().expect("object checked above");
                value.insert("context".into(), common);
                Value::Object(value)
            }
            AiFeatureCallerDto::Chat => {
                let goal = supplied.get("goal").cloned().unwrap_or(Value::Null);
                let message = supplied
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let coach = workspace.read_coach_data().map_err(CoreError::message)?;
                let goal_id = goal.get("id").and_then(Value::as_str);
                let chat_id = goal_id.and_then(|goal_id| {
                    coach
                        .chats
                        .iter()
                        .find(|chat| chat.goal_id.is_some_and(|id| id.to_string() == goal_id))
                        .map(|chat| chat.id)
                });
                let history = chat_id
                    .map(|id| {
                        coach
                            .messages
                            .iter()
                            .filter(|entry| entry.chat_id == id)
                            .rev()
                            .take(20)
                            .map(|entry| json!({"role":entry.role,"content":entry.content}))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                json!({"goal":goal,"message":message,"history":history,"context":common})
            }
            AiFeatureCallerDto::HomeAsk => {
                let session_id = supplied
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CoreError::message("Home Ask requires sessionId"))?;
                let message = supplied
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| CoreError::message("Home Ask requires a message"))?;
                let session_id = parse_uuid(session_id)?;
                let session = workspace
                    .read_home_ask_sessions()
                    .map_err(CoreError::message)?
                    .into_iter()
                    .find(|value| value.id == session_id);
                let history = session
                    .and_then(|value| {
                        value
                            .payload
                            .get("messages")
                            .and_then(Value::as_array)
                            .cloned()
                    })
                    .unwrap_or_default();
                json!({"question":message,"history":history.into_iter().rev().take(20).collect::<Vec<_>>(),"readinessAvailable":false,"context":common})
            }
            AiFeatureCallerDto::StudySuggestions | AiFeatureCallerDto::DailyPlan => {
                json!({"date":supplied.get("date").and_then(Value::as_str).unwrap_or(""),"context":common,"allowedEvidenceKeys":common["allowedEvidenceKeys"].clone()})
            }
            AiFeatureCallerDto::ScorePrediction => {
                let kind = supplied
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("single");
                let id = parse_uuid(
                    supplied
                        .get("examId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| CoreError::message("score prediction requires examId"))?,
                )?;
                if kind == "comprehensive" {
                    let exam = workspace
                        .read_comprehensive_exams()
                        .map_err(CoreError::message)?
                        .into_iter()
                        .find(|value| value.id == id)
                        .ok_or_else(|| CoreError::message("comprehensive exam not found"))?;
                    let mut per_subject = Vec::new();
                    let mut total_full = 0.0;
                    for subject in &exam.subject {
                        let values = grades_for_subject(&grades, subject, None);
                        if values.len() < 4 {
                            return Err(CoreError::message(format!(
                                "{subject} needs at least 4 valid grades before prediction"
                            )));
                        }
                        let full = subjects
                            .iter()
                            .find(|value| value.name == *subject)
                            .map(|value| value.full_score)
                            .unwrap_or(100.0);
                        total_full += full;
                        per_subject.push(json!({"subject":subject,"fullScore":full,"grades":values.iter().map(|value| json!({"id":value.id,"score":value.score,"fullScore":value.full_score,"date":value.date,"examName":value.exam_name})).collect::<Vec<_>>() }));
                    }
                    json!({"kind":"comprehensive","examId":id,"examName":exam.name,"fullScore":total_full,"subjects":per_subject,"context":common,"allowedEvidenceKeys":common["allowedEvidenceKeys"].clone()})
                } else {
                    let exam = workspace
                        .read_exams()
                        .map_err(CoreError::message)?
                        .into_iter()
                        .find(|value| value.id == id)
                        .ok_or_else(|| CoreError::message("exam not found"))?;
                    let values = grades_for_subject(&grades, &exam.subject, Some(id));
                    if values.len() < 4 {
                        return Err(CoreError::message(
                            "this exam subject needs at least 4 valid grades before prediction",
                        ));
                    }
                    let full = subjects
                        .iter()
                        .find(|value| value.name == exam.subject)
                        .map(|value| value.full_score)
                        .unwrap_or(100.0);
                    json!({"kind":"single","examId":id,"examName":exam.name,"subject":exam.subject,"fullScore":full,"grades":values.iter().map(|value| json!({"id":value.id,"score":value.score,"fullScore":value.full_score,"date":value.date,"examName":value.exam_name})).collect::<Vec<_>>(),"context":common,"allowedEvidenceKeys":common["allowedEvidenceKeys"].clone()})
                }
            }
            AiFeatureCallerDto::PredictionDiscussion => {
                let id = parse_uuid(
                    supplied
                        .get("predictionId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            CoreError::message("prediction discussion requires predictionId")
                        })?,
                )?;
                let message = supplied
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        CoreError::message("prediction discussion requires a message")
                    })?;
                let prediction = workspace
                    .read_score_predictions()
                    .map_err(CoreError::message)?
                    .into_iter()
                    .find(|value| value.id == id)
                    .ok_or_else(|| CoreError::message("prediction not found"))?;
                json!({"prediction":prediction.payload,"message":message,"context":common})
            }
            AiFeatureCallerDto::ExamAutopsy => {
                let id = parse_uuid(
                    supplied
                        .get("examId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| CoreError::message("exam autopsy requires examId"))?,
                )?;
                let exam = workspace
                    .read_exams()
                    .map_err(CoreError::message)?
                    .into_iter()
                    .find(|value| value.id == id)
                    .ok_or_else(|| CoreError::message("exam not found"))?;
                json!({"examId":id,"examName":exam.name,"subject":exam.subject,"context":common})
            }
            _ => unreachable!(),
        };
        request.input_json = serde_json::to_string(&input).map_err(CoreError::message)?;
        Ok(request)
    }

    fn install_workspace(&self, workspace: Workspace) -> Result<(), CoreError> {
        // Installing a Workspace resets run/inspection identity and rebuilds the
        // Agent against the currently selected model.  An active timer blocks
        // the transition so it cannot be migrated across Workspace roots.
        let mut active_timer = self.active_timer.lock();
        if active_timer.is_some() {
            return Err(CoreError::message(
                "cannot switch Workspace while a timer is active",
            ));
        }
        *self.workspace.lock() = Some(workspace);
        let model = self.model.lock().clone();
        *self.runtime.lock() = model.map(|model| {
            AgentRuntime::new(
                self.workspace
                    .lock()
                    .as_ref()
                    .expect("Workspace was just installed")
                    .clone(),
                model,
            )
        });
        *self.last_run_id.lock() = None;
        self.inspections.lock().clear();
        self.clear_ai_state();
        // Keep the timer state explicitly scoped to this lifecycle boundary.
        *active_timer = None;
        Ok(())
    }

    fn rebuild_runtime(&self, model: Arc<dyn ModelClient>) -> Result<(), CoreError> {
        // Provider switching replaces the runtime so no future Agent call can
        // retain the old client's credentials or protocol implementation.  The
        // timer is Workspace-scoped, so changing the model is rejected while a
        // timer is active instead of silently losing it.
        let active_timer = self.active_timer.lock();
        if active_timer.is_some() {
            return Err(CoreError::message(
                "cannot rebuild runtime while a timer is active",
            ));
        }
        let workspace = self.workspace.lock().clone();
        *self.runtime.lock() = workspace.map(|workspace| AgentRuntime::new(workspace, model));
        *self.last_run_id.lock() = None;
        self.clear_ai_state();
        drop(active_timer);
        Ok(())
    }

    fn clear_ai_state(&self) {
        *self.ai_state.lock() = ai::AiFeatureState::default();
    }

    fn workspace(&self) -> Result<Workspace, CoreError> {
        // Return a clone of the cheap Workspace handle, not a mutable reference;
        // Workspace itself owns its write lock and atomic persistence rules.
        self.workspace
            .lock()
            .as_ref()
            .cloned()
            .ok_or_else(|| CoreError::message("no Workspace is open"))
    }

    fn ensure_no_active_timer(&self) -> Result<(), CoreError> {
        if self.active_timer.lock().is_some() {
            return Err(CoreError::message(
                "an active timer blocks this lifecycle change",
            ));
        }
        Ok(())
    }

    fn runtime(&self) -> Result<Arc<AgentRuntime>, CoreError> {
        // Distinguish “no Workspace” from “Workspace without AI provider” for
        // callers, while keeping the runtime object private to the facade.
        self.runtime.lock().as_ref().cloned().ok_or_else(|| {
            if self.workspace.lock().is_some() {
                CoreError::message("Cloud AI is not connected")
            } else {
                CoreError::message("no Workspace is open")
            }
        })
    }

    fn finish_ai_failure(
        &self,
        prepared: &ai::PreparedAiFeature,
        started: Instant,
        failure: ai::AiFailure,
    ) -> Result<String, CoreError> {
        if let Some(stale_json) = self.ai_state.lock().stale(&prepared.cache_key) {
            let mut diagnostic = ai::failure_diagnostic(
                prepared,
                started.elapsed().as_millis() as u64,
                failure.code,
            );
            diagnostic.stale_result = true;
            diagnostic.outcome = "stale".into();
            self.ai_state.lock().record(diagnostic.clone());
            return ai::envelope(prepared, &stale_json, diagnostic)
                .map_err(|error| CoreError::message(error.message));
        }
        let diagnostic =
            ai::failure_diagnostic(prepared, started.elapsed().as_millis() as u64, failure.code);
        self.ai_state.lock().record(diagnostic);
        Err(CoreError::message(format!(
            "AI {} failed: {}",
            prepared.caller.label(),
            failure.message
        )))
    }
}

/// Small, bounded evidence projections keep model prompts auditable while
/// preventing arbitrary Workspace identifiers from being cited in an answer.
fn ai_evidence(
    grades: &[Grade],
    mistakes: &[MistakeNoteFull],
    tasks: &[TaskItem],
) -> Vec<serde_json::Value> {
    let mut evidence = Vec::new();
    evidence.extend(grades.iter().rev().take(20).map(|grade| {
        serde_json::json!({
            "key": format!("grade:{}", grade.id),
            "type": "grade",
            "label": format!("{}: {}", grade.subject, grade.exam_name),
        })
    }));
    evidence.extend(mistakes.iter().take(20).map(|mistake| {
        serde_json::json!({
            "key": format!("mistake:{}", mistake.id),
            "type": "mistake",
            "label": mistake.title,
        })
    }));
    evidence.extend(
        tasks
            .iter()
            .filter(|task| !task.is_completed)
            .take(20)
            .map(|task| {
                serde_json::json!({
                    "key": format!("task:{}", task.id),
                    "type": "task",
                    "label": task.title,
                })
            }),
    );
    evidence
}

/// Prediction eligibility is intentionally based on valid subject history,
/// rather than requiring a link only newer desktop grades contain.  `exam_id`
/// is retained for provenance; legacy grades use their subject (and, elsewhere,
/// normalised exam name) without silently rewriting storage.
fn grades_for_subject<'a>(
    grades: &'a [Grade],
    subject: &str,
    _exam_id: Option<uuid::Uuid>,
) -> Vec<&'a Grade> {
    grades
        .iter()
        .filter(|grade| {
            grade.subject == subject
                && grade.score.is_finite()
                && grade.score >= 0.0
                && grade.full_score.unwrap_or(100.0).is_finite()
                && grade.full_score.unwrap_or(100.0) > 0.0
                && chrono::DateTime::parse_from_rfc3339(&grade.date).is_ok()
        })
        .collect()
}

fn run_async<T>(
    future: impl std::future::Future<Output = Result<T, studypulse_model_client::ModelError>>,
) -> Result<T, studypulse_model_client::ModelError> {
    // Keeping this executor local avoids requiring callers to understand Tokio
    // when invoking synchronous UniFFI methods.  Long-lived Agent execution is
    // still owned by AgentRuntime; this helper is reserved for bounded calls.
    // UniFFI methods are synchronous, so provider futures run on a short-lived
    // current-thread executor.  AgentRuntime uses its own executor for runs;
    // this helper is limited to one-shot auth/profile/logout calls.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| studypulse_model_client::ModelError::Request(error.to_string()))?
        .block_on(future)
}

fn phase3_action_uuid(record_id: uuid::Uuid, action_id: &str) -> uuid::Uuid {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(record_id.as_bytes());
    hasher.update([0]);
    hasher.update(action_id.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hasher.finalize()[..16]);
    // Mark the deterministic value as an RFC 4122 UUID without depending on
    // the optional uuid-v5 crate feature.
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

struct RestoreFlagGuard<'a>(&'a AtomicBool);

impl Drop for RestoreFlagGuard<'_> {
    // RAII guarantees a failed validation, conversion, or Workspace operation
    // cannot leave the facade permanently reporting an active restore.
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl From<WorkspaceInfo> for WorkspaceDto {
    // WorkspaceInfo already contains validated metadata; this projection keeps
    // paths and schema version in the FFI naming convention.
    fn from(value: WorkspaceInfo) -> Self {
        Self {
            id: value.id,
            root_path: value.root_path,
            schema_version: value.schema_version,
        }
    }
}

fn parse_uuid(value: &str) -> Result<uuid::Uuid, CoreError> {
    // UUID parsing happens at every id-taking facade method so domain methods
    // receive typed identities and error messages stay uniform.
    value
        .parse()
        .map_err(|error| CoreError::message(format!("invalid UUID {value}: {error}")))
}

fn parse_optional_uuid(value: Option<String>) -> Result<Option<uuid::Uuid>, CoreError> {
    // Optional links preserve “not associated” as None while still rejecting a
    // present malformed UUID instead of silently dropping it.
    value.as_deref().map(parse_uuid).transpose()
}

fn encode_extra(value: &BTreeMap<String, serde_json::Value>) -> String {
    // Unknown domain fields cross UniFFI as one JSON string, preserving forward
    // compatibility without asking Swift to model every future key.
    serde_json::to_string(value).unwrap_or_else(|_| "{}".into())
}

fn decode_extra(value: &str) -> Result<BTreeMap<String, serde_json::Value>, CoreError> {
    // Decode extras as an object only.  A scalar or array would not round-trip
    // to the domain flatten map and is rejected before a write.
    if value.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let parsed: serde_json::Value =
        serde_json::from_str(value).map_err(|error| CoreError::message(error.to_string()))?;
    match parsed {
        serde_json::Value::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(CoreError::message("extra_json must contain a JSON object")),
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, CoreError> {
    // Structured JSON edges share one error conversion so invalid DTO payloads
    // cannot bypass the facade's single CoreError boundary.
    serde_json::from_str(value).map_err(|error| CoreError::message(error.to_string()))
}

// Conversion implementations are deliberately kept in this facade instead of
// scattered across domain crates.  The domain models can evolve internally,
// while this file documents exactly what Swift receives and what validation
// occurs before a DTO is accepted for persistence.

// Task conversion is asymmetric by design: From exposes the domain record,
// while TryFrom parses UUIDs/extras and calls domain validation before writes.
// The conversion section follows one invariant throughout: From is a read
// projection, TryFrom is a write gate.  UUIDs are parsed, `extra_json` is
// decoded as an object, optional relationships stay optional, and domain
// validation is invoked before a Workspace mutation.  This is intentionally
// repetitive at the family boundaries because each DTO is independently
// generated for Swift and may be maintained without reading neighboring types.
impl From<TaskItem> for TaskDto {
    fn from(value: TaskItem) -> Self {
        Self {
            id: value.id.to_string(),
            title: value.title,
            task_type: match value.task_type {
                TaskType::Homework => TaskTypeDto::Homework,
                TaskType::Reading => TaskTypeDto::Reading,
            },
            due_date: value.due_date,
            reminder_date: value.reminder_date,
            subject: value.subject,
            importance: value.importance,
            notes: value.notes,
            is_completed: value.is_completed,
            reminder_event_id: value.reminder_event_id,
            reminder_calendar_id: value.reminder_calendar_id,
            created_at: value.created_at,
            phase_id: value.phase_id.map(|id| id.to_string()),
            coach_execution_data: value.coach_execution_data,
            coach_goal_id: value.coach_goal_id.map(|id| id.to_string()),
            coach_proposal_id: value.coach_proposal_id.map(|id| id.to_string()),
            extra_json: encode_extra(&value.extra),
        }
    }
}

// The subject/phase/grade family follows the same lossless projection rule:
// ids are strings at the wire edge, optional links stay optional, and `extra`
// is carried as JSON so older clients do not destroy newer fields on write.

// TryFrom is the write-side boundary: parse UUIDs/extras first, then let the
// domain validator reject empty titles, bad dates, or unsupported priorities.
impl TryFrom<TaskDto> for TaskItem {
    type Error = CoreError;

    fn try_from(value: TaskDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            title: value.title,
            task_type: match value.task_type {
                TaskTypeDto::Homework => TaskType::Homework,
                TaskTypeDto::Reading => TaskType::Reading,
            },
            due_date: value.due_date,
            reminder_date: value.reminder_date,
            subject: value.subject,
            importance: value.importance,
            notes: value.notes,
            is_completed: value.is_completed,
            reminder_event_id: value.reminder_event_id,
            reminder_calendar_id: value.reminder_calendar_id,
            created_at: value.created_at,
            phase_id: parse_optional_uuid(value.phase_id)?,
            coach_execution_data: value.coach_execution_data,
            coach_goal_id: parse_optional_uuid(value.coach_goal_id)?,
            coach_proposal_id: parse_optional_uuid(value.coach_proposal_id)?,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Subject conversion keeps display metadata and extension fields intact; the
// reverse path rejects malformed ids instead of creating a new subject.
impl From<Subject> for SubjectDto {
    fn from(value: Subject) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            enabled: value.enabled,
            full_score: value.full_score,
            display_name: value.display_name,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Subject writes preserve the caller's id and extension map; malformed values
// fail before Workspace receives a mutation.
impl TryFrom<SubjectDto> for Subject {
    type Error = CoreError;

    fn try_from(value: SubjectDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            name: value.name,
            enabled: value.enabled,
            full_score: value.full_score,
            display_name: value.display_name,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Phase goals are nested inside StudyPhase but still have their own UUID and
// extra map, so both directions use the same strict helper functions.
impl From<PhaseGoal> for PhaseGoalDto {
    fn from(value: PhaseGoal) -> Self {
        Self {
            id: value.id.to_string(),
            subject: value.subject,
            target_score: value.target_score,
            notes: value.notes,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Nested phase goals use the same strict id/extra conversion as top-level
// records, keeping a malformed child from being silently dropped.
impl TryFrom<PhaseGoalDto> for PhaseGoal {
    type Error = CoreError;

    fn try_from(value: PhaseGoalDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            subject: value.subject,
            target_score: value.target_score,
            notes: value.notes,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// StudyPhase conversion maps nested goals recursively and preserves archived
// timestamps as optional wire values rather than inventing defaults.
impl From<StudyPhase> for StudyPhaseDto {
    fn from(value: StudyPhase) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            start_date: value.start_date,
            end_date: value.end_date,
            is_archived: value.is_archived,
            archived_at: value.archived_at,
            goals: value.goals.into_iter().map(Into::into).collect(),
            created_at: value.created_at,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Phase conversion validates every nested goal and optional archive timestamp
// before the enclosing phase is persisted.
impl TryFrom<StudyPhaseDto> for StudyPhase {
    type Error = CoreError;

    fn try_from(value: StudyPhaseDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            name: value.name,
            start_date: value.start_date,
            end_date: value.end_date,
            is_archived: value.is_archived,
            archived_at: value.archived_at,
            goals: value
                .goals
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            created_at: value.created_at,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Grade DTOs carry optional image/ranking/phase data.  Conversion does not
// normalize scores; Workspace remains the source of score validation semantics.
// Assessment records keep raw/normalized score data separate.  The facade does
// not “helpfully” recompute rates or rankings during a transport conversion;
// analytics consumes the stored values later.
impl From<Grade> for GradeDto {
    // Grade projection preserves optional ranking, full score, image metadata,
    // and phase links because reports depend on those distinctions.
    fn from(value: Grade) -> Self {
        Self {
            id: value.id.to_string(),
            subject: value.subject,
            score: value.score,
            raw_score: value.raw_score,
            ranking: value.ranking,
            importance: value.importance,
            image_base64: value.image,
            image_file_name: value.image_file_name,
            date: value.date,
            exam_name: value.exam_name,
            exam_id: value.exam_id.map(|id| id.to_string()),
            full_score: value.full_score,
            phase_id: value.phase_id.map(|id| id.to_string()),
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Grade TryFrom parses optional foreign ids and image metadata without changing
// score values; domain validation remains authoritative.
impl TryFrom<GradeDto> for Grade {
    // Grade input validates ids and extension JSON before score-domain checks
    // run, keeping malformed imported records out of local storage.
    type Error = CoreError;

    fn try_from(value: GradeDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            subject: value.subject,
            score: value.score,
            raw_score: value.raw_score,
            ranking: value.ranking,
            importance: value.importance,
            image: value.image_base64,
            image_file_name: value.image_file_name,
            date: value.date,
            exam_name: value.exam_name,
            exam_id: parse_optional_uuid(value.exam_id)?,
            full_score: value.full_score,
            phase_id: parse_optional_uuid(value.phase_id)?,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Diary timestamps and mood/energy values cross as the existing scalar fields;
// the domain validator remains responsible for acceptable ranges and dates.
impl From<DiaryEntry> for DiaryEntryDto {
    // Diary projection is read-only and keeps the stored mood/energy values
    // intact.  No client-specific interpretation belongs in this adapter.
    fn from(value: DiaryEntry) -> Self {
        Self {
            id: value.id.to_string(),
            date: value.date,
            mood_score: value.mood_score,
            energy_score: value.energy_score,
            energy_tag: value.energy_tag,
            content: value.content,
            phase_id: value.phase_id.map(|id| id.to_string()),
            created_at: value.created_at,
            updated_at: value.updated_at,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Diary writes retain explicit timestamps and extension JSON so old entries can
// be edited without losing fields introduced by newer clients.
impl TryFrom<DiaryEntryDto> for DiaryEntry {
    // Incoming diary values use the same UUID, timestamp, and extension checks
    // as every persisted record before they reach Workspace.
    type Error = CoreError;

    fn try_from(value: DiaryEntryDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            date: value.date,
            mood_score: value.mood_score,
            energy_score: value.energy_score,
            energy_tag: value.energy_tag,
            content: value.content,
            phase_id: parse_optional_uuid(value.phase_id)?,
            created_at: value.created_at,
            updated_at: value.updated_at,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// ReviewState is shared by SRS analytics and mistake DTOs.  Keeping its
// conversion standalone avoids two clients drifting on ease/interval fields.
impl From<ReviewState> for ReviewStateDto {
    fn from(value: ReviewState) -> Self {
        Self {
            repetitions: value.repetitions,
            ease_factor: value.ease_factor,
            interval_days: value.interval_days,
            next_review_date: value.next_review_date,
            last_review_date: value.last_review_date,
            lapses: value.lapses,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// ReviewState conversion keeps SRS numeric values intact; apply_srs decides how
// they evolve, not the transport layer.
impl TryFrom<ReviewStateDto> for ReviewState {
    type Error = CoreError;

    fn try_from(value: ReviewStateDto) -> Result<Self, Self::Error> {
        Ok(Self {
            repetitions: value.repetitions,
            ease_factor: value.ease_factor,
            interval_days: value.interval_days,
            next_review_date: value.next_review_date,
            last_review_date: value.last_review_date,
            lapses: value.lapses,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Mastery history is append-only evidence for analytics.  The facade projects
// it without recalculating scores or timestamps.
impl From<MasteryHistoryEntry> for MasteryHistoryEntryDto {
    fn from(value: MasteryHistoryEntry) -> Self {
        Self {
            id: value.id.to_string(),
            timestamp: value.timestamp,
            score: value.score,
            quality: value.quality,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Mastery history input is validated as an entry but never re-derived from a
// current score, preserving historical evidence.
impl TryFrom<MasteryHistoryEntryDto> for MasteryHistoryEntry {
    type Error = CoreError;

    fn try_from(value: MasteryHistoryEntryDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            timestamp: value.timestamp,
            score: value.score,
            quality: value.quality,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Handwriting answers use base64 strings at the FFI boundary; media decoding is
// intentionally left to the client and is not performed during conversion.
impl From<HandwritingAnswerEntry> for HandwritingAnswerEntryDto {
    fn from(value: HandwritingAnswerEntry) -> Self {
        Self {
            id: value.id.to_string(),
            timestamp: value.timestamp,
            image_base64: value.image_data,
            quality: value.quality,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Handwriting DTO input remains base64 text at this boundary; media limits and
// decoding rules belong to the Workspace/media path.
impl TryFrom<HandwritingAnswerEntryDto> for HandwritingAnswerEntry {
    type Error = CoreError;

    fn try_from(value: HandwritingAnswerEntryDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            timestamp: value.timestamp,
            image_data: value.image_base64,
            quality: value.quality,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Mistake conversion is the largest learning-record projection: nested review,
// mastery, handwriting, tags, media names, and extension JSON all round-trip.
// TryFrom validates every nested UUID/date before persistence.
// Mistake notes are the main cross-domain graph: SRS state, mastery history,
// handwriting evidence, media names, and tags all need to survive a round
// trip.  Keeping the graph explicit also makes it clear where a future field
// belongs: DTO field, nested DTO, or extra_json.
impl From<MistakeNoteFull> for MistakeNoteDto {
    // Mistake history includes SRS state and mastery history; this projection
    // keeps both the current queue state and the audit trail visible to clients.
    fn from(value: MistakeNoteFull) -> Self {
        Self {
            id: value.id.to_string(),
            title: value.title,
            subject: value.subject,
            original_question: value.original_question,
            source: value.source,
            date: value.date,
            error_reason: value.error_reason,
            wrong_solution: value.wrong_solution,
            correct_solution: value.correct_solution,
            question_images: value.question_images,
            reason_images: value.reason_images,
            wrong_solution_images: value.wrong_solution_images,
            correct_solution_images: value.correct_solution_images,
            review_state: value.review_state.map(Into::into),
            phase_id: value.phase_id.map(|id| id.to_string()),
            exposure_count: value.exposure_count,
            mastery_score: value.mastery_score,
            mastery_history: value.mastery_history.into_iter().map(Into::into).collect(),
            handwriting_history: value
                .handwriting_history
                .into_iter()
                .map(Into::into)
                .collect(),
            difficulty: value.difficulty,
            tags: value.tags,
            audio_file_name: value.audio_file_name,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Mistake TryFrom recursively validates review and history children, then lets
// MistakeNoteFull::validate enforce the record-level invariants.
impl TryFrom<MistakeNoteDto> for MistakeNoteFull {
    type Error = CoreError;

    fn try_from(value: MistakeNoteDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            title: value.title,
            subject: value.subject,
            original_question: value.original_question,
            source: value.source,
            date: value.date,
            error_reason: value.error_reason,
            wrong_solution: value.wrong_solution,
            correct_solution: value.correct_solution,
            question_images: value.question_images,
            reason_images: value.reason_images,
            wrong_solution_images: value.wrong_solution_images,
            correct_solution_images: value.correct_solution_images,
            review_state: value.review_state.map(TryInto::try_into).transpose()?,
            phase_id: parse_optional_uuid(value.phase_id)?,
            exposure_count: value.exposure_count,
            mastery_score: value.mastery_score,
            mastery_history: value
                .mastery_history
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            handwriting_history: value
                .handwriting_history
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            difficulty: value.difficulty,
            tags: value.tags,
            audio_file_name: value.audio_file_name,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Exam time slots are small nested records, but their date strings remain
// domain-owned so the FFI never silently changes timezone or precision.
impl From<ExamTimeSlot> for ExamTimeSlotDto {
    fn from(value: ExamTimeSlot) -> Self {
        Self {
            start_time: value.start_time,
            end_time: value.end_time,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Exam slot input is parsed as a nested value so invalid times fail the whole
// exam conversion instead of becoming an incomplete schedule.
impl TryFrom<ExamTimeSlotDto> for ExamTimeSlot {
    type Error = CoreError;

    fn try_from(value: ExamTimeSlotDto) -> Result<Self, Self::Error> {
        Ok(Self {
            start_time: value.start_time,
            end_time: value.end_time,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Checklist conversion preserves stable ids and sort order; UI check state is
// not inferred from title or position.
impl From<ExamChecklistItem> for ExamChecklistItemDto {
    fn from(value: ExamChecklistItem) -> Self {
        Self {
            id: value.id.to_string(),
            title: value.title,
            is_checked: value.is_checked,
            sort_order: value.sort_order,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Checklist TryFrom preserves explicit sort order and checked state; no UI
// index is substituted for the stored value.
impl TryFrom<ExamChecklistItemDto> for ExamChecklistItem {
    type Error = CoreError;

    fn try_from(value: ExamChecklistItemDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            title: value.title,
            is_checked: value.is_checked,
            sort_order: value.sort_order,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Exam review notes and linked mistake ids remain a typed nested DTO graph so
// review history can be edited without reparsing the whole exam JSON.
impl From<ExamReview> for ExamReviewDto {
    fn from(value: ExamReview) -> Self {
        Self {
            id: value.id.to_string(),
            reviewed_at: value.reviewed_at,
            what_was_tested: value.what_was_tested,
            what_went_wrong: value.what_went_wrong,
            what_learned: value.what_learned,
            next_strategy: value.next_strategy,
            linked_mistake_ids: value
                .linked_mistake_ids
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Exam review input validates linked mistake ids and keeps the note fields
// untouched for round-trip editing.
impl TryFrom<ExamReviewDto> for ExamReview {
    type Error = CoreError;

    fn try_from(value: ExamReviewDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            reviewed_at: value.reviewed_at,
            what_was_tested: value.what_was_tested,
            what_went_wrong: value.what_went_wrong,
            what_learned: value.what_learned,
            next_strategy: value.next_strategy,
            linked_mistake_ids: value
                .linked_mistake_ids
                .into_iter()
                .map(|id| parse_uuid(&id))
                .collect::<Result<Vec<_>, _>>()?,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Exam conversion composes the slot/checklist/review projections and retains
// optional locations, notifications, and phase linkage exactly as stored.
// Exam projection preserves nested review/checklist relationships rather than
// flattening them into a JSON blob.  This lets Swift edit one child while the
// Workspace remains the owner of the enclosing record's validation.
impl From<ExamFull> for ExamDto {
    // Exam projection preserves nested slots, checklist items, and review
    // links as separate DTO records so the UI can edit each lifecycle safely.
    fn from(value: ExamFull) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            exam_date: value.exam_date,
            exam_end_date: value.exam_end_date,
            importance: value.importance,
            subject: value.subject,
            exam_name: value.exam_name,
            mastery_degree: value.mastery_degree,
            time_slot: value.time_slot.map(Into::into),
            phase_id: value.phase_id.map(|id| id.to_string()),
            checklist: value.checklist.into_iter().map(Into::into).collect(),
            location_school: value.location_school,
            location_classroom: value.location_classroom,
            location_seat: value.location_seat,
            countdown_notify_days: value.countdown_notify_days,
            exam_review: value.exam_review.map(Into::into),
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Single-exam conversion composes nested slot/checklist/review results and
// rejects any invalid child before the parent write.
impl TryFrom<ExamDto> for ExamFull {
    // Exam writes reconstruct typed ids and nested records before domain
    // validation; a malformed child cannot be hidden inside an outer DTO.
    type Error = CoreError;

    fn try_from(value: ExamDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            name: value.name,
            exam_date: value.exam_date,
            exam_end_date: value.exam_end_date,
            importance: value.importance,
            subject: value.subject,
            exam_name: value.exam_name,
            mastery_degree: value.mastery_degree,
            time_slot: value.time_slot.map(TryInto::try_into).transpose()?,
            phase_id: parse_optional_uuid(value.phase_id)?,
            checklist: value
                .checklist
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            location_school: value.location_school,
            location_classroom: value.location_classroom,
            location_seat: value.location_seat,
            countdown_notify_days: value.countdown_notify_days,
            exam_review: value.exam_review.map(TryInto::try_into).transpose()?,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Comprehensive exams differ from single exams only in subject/time-slot shape;
// keeping a separate conversion prevents accidental loss of multi-subject data.
impl From<ComprehensiveExamFull> for ComprehensiveExamDto {
    // Comprehensive exam DTOs are read projections of aggregate analytics, not
    // a second calculation path for readiness or grading.
    fn from(value: ComprehensiveExamFull) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            exam_date: value.exam_date,
            exam_end_date: value.exam_end_date,
            importance: value.importance,
            subjects: value.subject,
            exam_name: value.exam_name,
            mastery_degree: value.mastery_degree,
            subject_time_slots_json: value
                .subject_time_slots
                .map(|slots| serde_json::to_string(&slots).unwrap_or_else(|_| "{}".into())),
            phase_id: value.phase_id.map(|id| id.to_string()),
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Comprehensive exam input preserves its subject list and optional time-slot
// JSON as distinct fields; it is not coerced into a single-subject exam.
impl TryFrom<ComprehensiveExamDto> for ComprehensiveExamFull {
    type Error = CoreError;

    fn try_from(value: ComprehensiveExamDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            name: value.name,
            exam_date: value.exam_date,
            exam_end_date: value.exam_end_date,
            importance: value.importance,
            subject: value.subjects,
            exam_name: value.exam_name,
            mastery_degree: value.mastery_degree,
            subject_time_slots: value
                .subject_time_slots_json
                .as_deref()
                .map(parse_json)
                .transpose()?,
            phase_id: parse_optional_uuid(value.phase_id)?,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Routine enums are explicit exhaustive mappings.  Unknown variants cannot be
// silently represented at this FFI boundary, which makes additions reviewable.
impl From<RoutineType> for RoutineTypeDto {
    fn from(value: RoutineType) -> Self {
        match value {
            RoutineType::MistakeReview => Self::MistakeReview,
            RoutineType::Flashcard => Self::Flashcard,
            RoutineType::General => Self::General,
        }
    }
}

// Routine and routine-instance records are split because a generated instance
// owns completion state while the routine owns the reusable schedule.  A single
// DTO would invite callers to overwrite one lifecycle with the other.

impl From<RoutineTypeDto> for RoutineType {
    fn from(value: RoutineTypeDto) -> Self {
        match value {
            RoutineTypeDto::MistakeReview => Self::MistakeReview,
            RoutineTypeDto::Flashcard => Self::Flashcard,
            RoutineTypeDto::General => Self::General,
        }
    }
}

// Routine conversion keeps recurring schedule fields separate from generated
// instance state; persistence continues to happen in Workspace.
// Recurrence definitions and generated instances intentionally have separate
// conversion families.  A routine update therefore cannot accidentally change
// completion state already materialized for a date.
impl From<Routine> for RoutineDto {
    fn from(value: Routine) -> Self {
        Self {
            id: value.id.to_string(),
            title: value.title,
            routine_type: value.r#type.into(),
            subject: value.subject,
            weekdays: value.weekdays,
            start_time: value.start_time,
            end_time: value.end_time,
            enabled: value.enabled,
            created_at: value.created_at,
            phase_id: value.phase_id.map(|id| id.to_string()),
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Routine write conversion validates recurring schedule data and optional
// subject/phase links before Workspace stores the definition.
impl TryFrom<RoutineDto> for Routine {
    type Error = CoreError;

    fn try_from(value: RoutineDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            title: value.title,
            r#type: value.routine_type.into(),
            subject: value.subject,
            weekdays: value.weekdays,
            start_time: value.start_time,
            end_time: value.end_time,
            enabled: value.enabled,
            created_at: value.created_at,
            phase_id: parse_optional_uuid(value.phase_id)?,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Routine instances carry their date key and completion metadata independently
// so a generated occurrence can be updated without rewriting its definition.
impl From<RoutineInstance> for RoutineInstanceDto {
    fn from(value: RoutineInstance) -> Self {
        Self {
            id: value.id.to_string(),
            routine_id: value.routine_id.to_string(),
            title: value.title,
            routine_type: value.r#type.into(),
            subject: value.subject,
            start_time: value.start_time,
            end_time: value.end_time,
            date: value.date,
            date_key: value.date_key,
            is_completed: value.is_completed,
            completed_at: value.completed_at,
            spawned_mistake_count: value.spawned_mistake_count,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Generated-instance conversion is separate from Routine conversion so
// completion timestamps and spawned counts remain instance-local.
impl TryFrom<RoutineInstanceDto> for RoutineInstance {
    type Error = CoreError;

    fn try_from(value: RoutineInstanceDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            routine_id: parse_uuid(&value.routine_id)?,
            title: value.title,
            r#type: value.routine_type.into(),
            subject: value.subject,
            start_time: value.start_time,
            end_time: value.end_time,
            date: value.date,
            date_key: value.date_key,
            is_completed: value.is_completed,
            completed_at: value.completed_at,
            spawned_mistake_count: value.spawned_mistake_count,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Session intensity and source enums preserve provenance/effort labels without
// reinterpreting them in the facade.
impl From<SessionIntensity> for SessionIntensityDto {
    fn from(value: SessionIntensity) -> Self {
        match value {
            SessionIntensity::Peak => Self::Peak,
            SessionIntensity::DeepFocus => Self::DeepFocus,
            SessionIntensity::Steady => Self::Steady,
            SessionIntensity::Light => Self::Light,
            SessionIntensity::Recovery => Self::Recovery,
        }
    }
}

// Study-session conversions preserve provenance and intensity as enums rather
// than strings.  Exhaustive matches make a new domain variant fail compilation
// until the FFI contract and generated clients are updated together.

impl From<SessionIntensityDto> for SessionIntensity {
    fn from(value: SessionIntensityDto) -> Self {
        match value {
            SessionIntensityDto::Peak => Self::Peak,
            SessionIntensityDto::DeepFocus => Self::DeepFocus,
            SessionIntensityDto::Steady => Self::Steady,
            SessionIntensityDto::Light => Self::Light,
            SessionIntensityDto::Recovery => Self::Recovery,
        }
    }
}

// Timer/manual source is part of the stored session contract; conversion keeps
// it explicit for analytics and UI filtering.
impl From<StudySessionSource> for StudySessionSourceDto {
    fn from(value: StudySessionSource) -> Self {
        match value {
            StudySessionSource::Timer => Self::Timer,
            StudySessionSource::Manual => Self::Manual,
        }
    }
}

impl From<StudySessionSourceDto> for StudySessionSource {
    fn from(value: StudySessionSourceDto) -> Self {
        match value {
            StudySessionSourceDto::Timer => Self::Timer,
            StudySessionSourceDto::Manual => Self::Manual,
        }
    }
}

// Investment targets are UUID-linked domain values.  The reverse conversion
// validates both kind and id before they participate in aggregates.
impl From<InvestmentTarget> for InvestmentTargetDto {
    fn from(value: InvestmentTarget) -> Self {
        match value {
            InvestmentTarget::Subject(id) => Self {
                kind: "subject".into(),
                id: id.to_string(),
            },
            InvestmentTarget::SubTask(id) => Self {
                kind: "subTask".into(),
                id: id.to_string(),
            },
        }
    }
}

// Time-investment DTOs carry both aggregate summaries and their typed targets.
// The facade does not calculate totals; it projects the values already derived
// by Workspace so iOS, desktop, and tests share one analytics implementation.

// Investment target ids are parsed at the boundary and then validated by the
// domain type; aggregates never receive a raw wire string.
impl TryFrom<InvestmentTargetDto> for InvestmentTarget {
    type Error = CoreError;

    fn try_from(value: InvestmentTargetDto) -> Result<Self, Self::Error> {
        let id = parse_uuid(&value.id)?;
        match value.kind.as_str() {
            "subject" => Ok(Self::Subject(id)),
            "subTask" | "subtask" => Ok(Self::SubTask(id)),
            _ => Err(CoreError::message(format!(
                "unknown investment target kind: {}",
                value.kind
            ))),
        }
    }
}

// Physiological samples are optional session children.  Their conversion is
// lossless and does not apply health interpretation in the transport layer.
impl From<HeartRateSample> for HeartRateSampleDto {
    fn from(value: HeartRateSample) -> Self {
        Self {
            id: value.id.to_string(),
            timestamp: value.timestamp,
            bpm: value.bpm,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Heart-rate samples preserve timestamp/bpm values and reject malformed ids or
// extras before they can be attached to a study session.
impl TryFrom<HeartRateSampleDto> for HeartRateSample {
    type Error = CoreError;

    fn try_from(value: HeartRateSampleDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            timestamp: value.timestamp,
            bpm: value.bpm,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Difficulty annotations retain optional heart-rate context and subject links;
// analytics can decide how to use them after the round trip.
impl From<DifficultyAnnotation> for DifficultyAnnotationDto {
    fn from(value: DifficultyAnnotation) -> Self {
        Self {
            id: value.id.to_string(),
            timestamp: value.timestamp,
            heart_rate: value.heart_rate,
            note: value.note,
            subject_id: value.subject_id.map(|id| id.to_string()),
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Difficulty annotations are optional session evidence; conversion does not
// classify difficulty or infer a subject from the note.
impl TryFrom<DifficultyAnnotationDto> for DifficultyAnnotation {
    type Error = CoreError;

    fn try_from(value: DifficultyAnnotationDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            timestamp: value.timestamp,
            heart_rate: value.heart_rate,
            note: value.note,
            subject_id: parse_optional_uuid(value.subject_id)?,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// StudySession conversion recursively projects samples, annotations, and target
// while preserving the optional-vs-empty distinction used by older records.
// Sessions are the bridge between timer/manual input and analytics.  Recursive
// conversion keeps optional heart-rate/difficulty evidence and investment
// target identity visible without running any analysis at this layer.
impl From<StudySession> for StudySessionDto {
    // Session projection copies analytics inputs without recalculating elapsed
    // time or altering source/intensity provenance.
    fn from(value: StudySession) -> Self {
        Self {
            id: value.id.to_string(),
            start_date: value.start_date,
            duration_seconds: value.duration_seconds,
            intensity: value.intensity.into(),
            completed: value.completed,
            heart_rate_samples: value
                .heart_rate_samples
                .map(|values| values.into_iter().map(Into::into).collect()),
            difficulty_annotations: value
                .difficulty_annotations
                .map(|values| values.into_iter().map(Into::into).collect()),
            investment_target: value.investment_target.map(Into::into),
            source: value.source.into(),
            time_zone_identifier: value.time_zone_identifier,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// StudySession TryFrom recursively converts optional child vectors and target,
// preserving None versus an explicitly empty history.
impl TryFrom<StudySessionDto> for StudySession {
    // Session input is parsed before persistence so optional annotations remain
    // either valid records or explicit None values, never silent placeholders.
    type Error = CoreError;

    fn try_from(value: StudySessionDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            start_date: value.start_date,
            duration_seconds: value.duration_seconds,
            intensity: value.intensity.into(),
            completed: value.completed,
            heart_rate_samples: value
                .heart_rate_samples
                .map(|values| {
                    values
                        .into_iter()
                        .map(TryInto::try_into)
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            difficulty_annotations: value
                .difficulty_annotations
                .map(|values| {
                    values
                        .into_iter()
                        .map(TryInto::try_into)
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            investment_target: value.investment_target.map(TryInto::try_into).transpose()?,
            source: value.source.into(),
            time_zone_identifier: value.time_zone_identifier,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Themes are display enums at the boundary, but exhaustive conversion keeps
// stored theme values stable across Swift and Rust.
impl From<TimeInvestmentTheme> for TimeInvestmentThemeDto {
    fn from(value: TimeInvestmentTheme) -> Self {
        match value {
            TimeInvestmentTheme::Ocean => Self::Ocean,
            TimeInvestmentTheme::Coral => Self::Coral,
            TimeInvestmentTheme::Violet => Self::Violet,
            TimeInvestmentTheme::Sunshine => Self::Sunshine,
            TimeInvestmentTheme::Mint => Self::Mint,
        }
    }
}

impl From<TimeInvestmentThemeDto> for TimeInvestmentTheme {
    fn from(value: TimeInvestmentThemeDto) -> Self {
        match value {
            TimeInvestmentThemeDto::Ocean => Self::Ocean,
            TimeInvestmentThemeDto::Coral => Self::Coral,
            TimeInvestmentThemeDto::Violet => Self::Violet,
            TimeInvestmentThemeDto::Sunshine => Self::Sunshine,
            TimeInvestmentThemeDto::Mint => Self::Mint,
        }
    }
}

// Investment subjects preserve ordering, archive status, symbol, and theme so
// the client can render the same hierarchy without domain knowledge.
// Investment hierarchy conversion preserves parent/order/archive metadata so
// clients can render and reorder the graph without reconstructing domain ids.
impl From<TimeInvestmentSubject> for TimeInvestmentSubjectDto {
    // Subject investment records keep target seconds and theme metadata typed
    // so aggregate summaries can be displayed without string parsing.
    fn from(value: TimeInvestmentSubject) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            symbol_name: value.symbol_name,
            theme: value.theme.into(),
            start_date: value.start_date,
            sort_order: value.sort_order,
            created_at: value.created_at,
            is_archived: value.is_archived,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Investment subject input keeps archive/order fields explicit and validates the
// UUID before it enters time-investment aggregation.
impl TryFrom<TimeInvestmentSubjectDto> for TimeInvestmentSubject {
    // The reverse projection parses the subject identity and extra object before
    // the record enters the Workspace investment collection.
    type Error = CoreError;

    fn try_from(value: TimeInvestmentSubjectDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            name: value.name,
            symbol_name: value.symbol_name,
            theme: value.theme.into(),
            start_date: value.start_date,
            sort_order: value.sort_order,
            created_at: value.created_at,
            is_archived: value.is_archived,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// SubTask conversion keeps parent linkage optional for roots and retains sort
// order as a persisted value rather than using UI array order.
impl From<SubTask> for SubTaskDto {
    fn from(value: SubTask) -> Self {
        Self {
            id: value.id.to_string(),
            subject_id: value.subject_id.to_string(),
            parent_sub_task_id: value.parent_sub_task_id.map(|id| id.to_string()),
            name: value.name,
            sort_order: value.sort_order,
            created_at: value.created_at,
            is_archived: value.is_archived,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Subtask conversion preserves optional parent linkage so hierarchy cycles and
// malformed ids remain domain validation concerns.
impl TryFrom<SubTaskDto> for SubTask {
    type Error = CoreError;

    fn try_from(value: SubTaskDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            subject_id: parse_uuid(&value.subject_id)?,
            parent_sub_task_id: parse_optional_uuid(value.parent_sub_task_id)?,
            name: value.name,
            sort_order: value.sort_order,
            created_at: value.created_at,
            is_archived: value.is_archived,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Goal rewards expose threshold/unlocked state as a snapshot; unlocking logic
// remains in Workspace and is not triggered by conversion.
impl From<GoalReward> for GoalRewardDto {
    // Reward projection preserves the goal link and earned timestamps as stored
    // values; the facade never grants or recalculates rewards.
    fn from(value: GoalReward) -> Self {
        Self {
            id: value.id.to_string(),
            title: value.title,
            symbol_name: value.symbol_name,
            target: value.target.into(),
            threshold_seconds: value.threshold_seconds,
            created_at: value.created_at,
            unlocked_at: value.unlocked_at,
            extra_json: encode_extra(&value.extra),
        }
    }
}

// Goal reward input validates its target and ids while leaving unlock status to
// the Workspace reward logic.
impl TryFrom<GoalRewardDto> for GoalReward {
    // Reward writes are identity-checked conversions, so a client cannot turn a
    // display-only string into an unrelated stored reward.
    type Error = CoreError;

    fn try_from(value: GoalRewardDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            title: value.title,
            symbol_name: value.symbol_name,
            target: value.target.try_into()?,
            threshold_seconds: value.threshold_seconds,
            created_at: value.created_at,
            unlocked_at: value.unlocked_at,
            extra: decode_extra(&value.extra_json)?,
        })
    }
}

// Summaries are derived counters and intentionally have no TryFrom: clients do
// not write aggregate results back into Workspace.
impl From<TimeInvestmentSummary> for TimeInvestmentSummaryDto {
    fn from(value: TimeInvestmentSummary) -> Self {
        Self {
            target_id: value.target_id,
            direct_seconds: value.direct_seconds,
            total_seconds: value.total_seconds,
            session_count: value.session_count as u64,
        }
    }
}

// TodaySnapshot is another read-only projection of analytics output.  Suggestions
// are transported as generated strings and are not treated as commands.
impl From<TodaySnapshot> for TodaySnapshotDto {
    fn from(value: TodaySnapshot) -> Self {
        Self {
            open_task_count: value.open_task_count as u64,
            completed_task_count: value.completed_task_count as u64,
            study_minutes: value.study_minutes,
            due_mistake_count: value.due_mistake_count as u64,
            due_mistake_ids: Vec::new(),
            upcoming_exam_ids: value
                .upcoming_exams
                .into_iter()
                .map(|exam| exam.id.to_string())
                .collect(),
            streak_days: value.streak_days,
            assigned_investment_seconds: value.assigned_seconds,
            suggestions: value.suggestions,
        }
    }
}

// Analytics conversion is intentionally read-only.  Suggestions, streaks,
// trend classifications, and SRS windows have already passed through the pure
// Workspace algorithms and must not be recomputed with platform date rules.

// SRS overview conversion preserves due/upcoming/enrolled counts calculated by
// the shared algorithm; the facade does not recompute date windows.
impl From<studypulse_workspace::SrsOverview> for SrsOverviewDto {
    // SRS counts are already computed against the shared UTC window.  The FFI
    // only transports them and must not apply client timezone adjustments.
    fn from(value: studypulse_workspace::SrsOverview) -> Self {
        Self {
            due_count: value.due_count as u64,
            upcoming_count: value.upcoming_count as u64,
            total_enrolled: value.total_enrolled as u64,
        }
    }
}

// Daily trend points retain optional mood/energy averages and activity counters
// exactly as the analytics function produced them.
impl From<studypulse_workspace::DailyTrendPoint> for DailyTrendPointDto {
    // Daily points preserve one row per calendar date, including optional mood
    // and energy averages that may legitimately be absent.
    fn from(value: studypulse_workspace::DailyTrendPoint) -> Self {
        Self {
            date: value.date,
            study_minutes: value.study_minutes,
            activity_points: value.activity_points,
            completed_session_count: value.completed_session_count as u64,
            review_count: value.review_count as u64,
            grade_count: value.grade_count as u64,
            mood_score: value.mood_score,
            energy_score: value.energy_score,
        }
    }
}

// Subject trend strings and attention flags are already classified by the
// domain analytics; conversion is intentionally a pure field projection.
impl From<studypulse_workspace::SubjectTrend> for SubjectTrendDto {
    fn from(value: studypulse_workspace::SubjectTrend) -> Self {
        Self {
            subject: value.subject,
            display_name: value.display_name,
            average_score_rate: value.average_score_rate,
            latest_score_rate: value.latest_score_rate,
            average_ranking: value.average_ranking,
            latest_ranking: value.latest_ranking,
            grade_count: value.grade_count as u64,
            mistake_count: value.mistake_count as u64,
            due_mistake_count: value.due_mistake_count as u64,
            trend: value.trend,
            needs_attention: value.needs_attention,
        }
    }
}

// TrendsSnapshot composes daily, subject, and SRS projections so Swift receives
// one immutable analysis response without re-running calculations.
// Analytics snapshots are read-only DTOs.  Their counters, averages, and
// classifications are already computed by Workspace and must not be altered
// by a platform-specific presentation adapter.
impl From<TrendsSnapshot> for TrendsSnapshotDto {
    // The complete trend snapshot is projected as one graph so clients cannot
    // accidentally combine ranges or recompute the current streak differently.
    fn from(value: TrendsSnapshot) -> Self {
        Self {
            start_date: value.start_date,
            end_date: value.end_date,
            active_days: value.active_days as u64,
            current_streak: value.current_streak,
            total_study_minutes: value.total_study_minutes,
            average_mood: value.average_mood,
            average_energy: value.average_energy,
            daily_points: value.daily_points.into_iter().map(Into::into).collect(),
            subjects: value.subjects.into_iter().map(Into::into).collect(),
            srs: value.srs.into(),
        }
    }
}

// A review result carries both new state and next date; exposing both avoids a
// client guessing the next interval from quality alone.
impl From<studypulse_workspace::SrsReviewResult> for SrsReviewResultDto {
    fn from(value: studypulse_workspace::SrsReviewResult) -> Self {
        Self {
            state: value.state.into(),
            next_review_date: value.next_review_date,
        }
    }
}

fn elapsed_seconds(timer: &ActiveTimer) -> i64 {
    // Timer elapsed time uses monotonic Instants for the active segment and a
    // stored accumulator for paused segments; wall-clock changes are irrelevant.
    // Combine paused accumulation with the current monotonic segment.  This is
    // process-local elapsed time, not a wall-clock difference that can jump.
    timer.elapsed_before_pause
        + timer
            .running_since
            .map(|started| started.elapsed().as_secs() as i64)
            .unwrap_or(0)
}

fn ensure_timer_workspace(timer: &ActiveTimer, workspace: &Workspace) -> Result<(), CoreError> {
    if timer.workspace_id != workspace.info().id {
        return Err(CoreError::message(
            "active timer belongs to a different Workspace",
        ));
    }
    Ok(())
}

fn timer_snapshot(timer: &ActiveTimer) -> TimerSnapshotDto {
    // A timer snapshot is derived state.  Only lifecycle commands mutate the
    // hidden timer, so a DTO read cannot extend or finish a session.
    // Snapshot status is derived from `running_since`; callers never mutate the
    // hidden Instant or bypass the timer lifecycle methods.
    TimerSnapshotDto {
        status: if timer.running_since.is_some() {
            TimerStatusKindDto::Running
        } else {
            TimerStatusKindDto::Paused
        },
        session_id: Some(timer.session_id.to_string()),
        started_at: Some(timer.started_at.clone()),
        elapsed_seconds: elapsed_seconds(timer),
        target_duration_seconds: timer.target_duration_seconds,
        intensity: Some(timer.intensity),
        investment_target: timer.investment_target.clone(),
    }
}

// Auth token conversion is kept narrow and local to the provider edge.  The
// facade can transport tokens to secure host storage, but account/status DTOs do
// not contain them.
impl From<CloudAuthTokens> for CloudAuthTokensDto {
    // Authentication results are projected only after the provider has
    // validated their prefixes.  Storage policy is enforced by the host, while
    // this conversion keeps the record shape stable for the handoff.
    fn from(value: CloudAuthTokens) -> Self {
        Self {
            access_token: value.access_token,
            refresh_token: value.refresh_token,
        }
    }
}

// CloudProfile becomes a redacted account view with plan/model metadata; no
// access or refresh token is copied into this status structure.
impl From<CloudProfile> for CloudAccountDto {
    // Profile projection contains account capability/status fields only; it
    // deliberately has no token or provider client reference.
    fn from(value: CloudProfile) -> Self {
        Self {
            email: value.email,
            role: value.role,
            membership_type: value.membership_type,
            membership_expires_at: value.membership_expires_at,
            plan_name: value.plan_name,
            available_models: value.available_models,
        }
    }
}

// BYOK configuration is deliberately non-secret.  This conversion is safe to
// return to UI because the client key is owned by OpenAICompatibleModelClient.
impl From<ByokConfig> for ByokConfigDto {
    fn from(value: ByokConfig) -> Self {
        Self {
            base_url: value.base_url,
            model: value.model,
        }
    }
}

// The remaining projections describe Agent and backup state.  They preserve
// event cursors, permission labels, and recovery identifiers exactly because UI
// behavior depends on those values being stable across polling calls.

// File entries expose relative metadata only.  Path resolution and symlink
// checks remain in Workspace when a source is actually opened.
impl From<FileEntry> for FileEntryDto {
    fn from(value: FileEntry) -> Self {
        Self {
            relative_path: value.relative_path,
            is_directory: value.is_directory,
            size_bytes: value.size_bytes,
            modified_at: value.modified_at,
        }
    }
}

// Notebook conversion recursively projects messages and source selection.  It
// does not read files or run tools, so serialization remains a pure snapshot.
// Notebook projection carries source selection and transcript history, but no
// live Agent runtime.  That distinction prevents a saved notebook from being
// mistaken for an active run after app restart.
impl From<AgentNotebook> for AgentNotebookDto {
    // Notebook projection is a pure snapshot of local history and selected
    // paths; it does not expose live locks, providers, or execution handles.
    fn from(value: AgentNotebook) -> Self {
        Self {
            id: value.id.to_string(),
            title: value.title,
            source_paths: value.source_paths,
            messages: value.messages.into_iter().map(Into::into).collect(),
            last_goal: value.last_goal,
            last_answer: value.last_answer,
            updated_at: value.updated_at,
        }
    }
}

// Incoming notebook DTOs are parsed back into domain values before Workspace
// writes them; this keeps message/history validation out of Swift.
// Notebook TryFrom is the write-side protection for source paths and message
// history; it runs before the identity-checked notebook save.
impl TryFrom<AgentNotebookDto> for AgentNotebook {
    // Notebook input is the write-side gate for ids, message roles, and source
    // selection.  Execution still requires a separate Agent command.
    type Error = CoreError;

    fn try_from(value: AgentNotebookDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id.parse().map_err(CoreError::message)?,
            title: value.title,
            source_paths: value.source_paths,
            messages: value
                .messages
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            last_goal: value.last_goal,
            last_answer: value.last_answer,
            updated_at: value.updated_at,
        })
    }
}

// Agent messages use an explicit role mapping so the Swift enum cannot depend on
// serde's Rust enum representation.
impl From<AgentMessage> for AgentMessageDto {
    fn from(value: AgentMessage) -> Self {
        Self {
            id: value.id.to_string(),
            role: match value.role {
                AgentMessageRole::User => AgentMessageRoleDto::User,
                AgentMessageRole::Assistant => AgentMessageRoleDto::Assistant,
            },
            content: value.content,
            created_at: value.created_at,
            turn_id: value.turn_id,
            source_refs_json: value.source_refs_json,
            artifact_refs_json: value.artifact_refs_json,
        }
    }
}

// The reverse role mapping is exhaustive and preserves the original timestamp
// string for Workspace validation.
// Agent message input maps the FFI role enum and preserves content/timestamps;
// it does not execute the represented conversation.
impl TryFrom<AgentMessageDto> for AgentMessage {
    type Error = CoreError;

    fn try_from(value: AgentMessageDto) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id.parse().map_err(CoreError::message)?,
            role: match value.role {
                AgentMessageRoleDto::User => AgentMessageRole::User,
                AgentMessageRoleDto::Assistant => AgentMessageRole::Assistant,
            },
            content: value.content,
            created_at: value.created_at,
            turn_id: value.turn_id,
            source_refs_json: value.source_refs_json,
            artifact_refs_json: value.artifact_refs_json,
        })
    }
}

// Search matches contain bounded snippets and relative paths; conversion does
// not canonicalize or open the path a second time.
impl From<SearchMatch> for SearchMatchDto {
    fn from(value: SearchMatch) -> Self {
        Self {
            relative_path: value.relative_path,
            line_number: value.line_number,
            snippet: value.snippet,
        }
    }
}

// Permission conversion mirrors the host risk taxonomy.  It is descriptive
// metadata and never substitutes for AgentRuntime confirmation.
impl From<PermissionLevel> for PermissionDto {
    // Permission labels are advisory metadata for presentation.  Runtime
    // confirmation remains authoritative even if a client displays the value.
    fn from(value: PermissionLevel) -> Self {
        match value {
            PermissionLevel::Read => Self::Read,
            PermissionLevel::Write => Self::Write,
            PermissionLevel::Destructive => Self::Destructive,
            PermissionLevel::Execute => Self::Execute,
        }
    }
}

// Run status conversion keeps transitional and terminal states distinct so a
// Swift poller can render cancellation/confirmation accurately.
impl From<RunStatus> for RunStatusDto {
    fn from(value: RunStatus) -> Self {
        match value {
            RunStatus::Started => Self::Started,
            RunStatus::Running => Self::Running,
            RunStatus::WaitingForConfirmation => Self::WaitingForConfirmation,
            RunStatus::Cancelling => Self::Cancelling,
            RunStatus::Failed => Self::Failed,
            RunStatus::Cancelled => Self::Cancelled,
            RunStatus::Completed => Self::Completed,
        }
    }
}

// Event-kind conversion is exhaustive because adding a runtime event must also
// be reflected in the generated Swift enum and client polling logic.
impl From<AgentEventKind> for AgentEventKindDto {
    // Event kind mapping is exhaustive so a new runtime event cannot silently
    // disappear from Swift polling or UI rendering.
    fn from(value: AgentEventKind) -> Self {
        match value {
            AgentEventKind::Started => Self::Started,
            AgentEventKind::StatusChanged => Self::StatusChanged,
            AgentEventKind::TextDelta => Self::TextDelta,
            AgentEventKind::ToolRequested => Self::ToolRequested,
            AgentEventKind::ToolCompleted => Self::ToolCompleted,
            AgentEventKind::ConfirmationRequired => Self::ConfirmationRequired,
            AgentEventKind::StageStarted => Self::StageStarted,
            AgentEventKind::StageProgress => Self::StageProgress,
            AgentEventKind::StageCompleted => Self::StageCompleted,
            AgentEventKind::InputRequired => Self::InputRequired,
            AgentEventKind::ArtifactCreated => Self::ArtifactCreated,
            AgentEventKind::Observation => Self::Observation,
            AgentEventKind::Sources => Self::Sources,
            AgentEventKind::Result => Self::Result,
            AgentEventKind::Usage => Self::Usage,
            AgentEventKind::TurnRecovered => Self::TurnRecovered,
            AgentEventKind::Failed => Self::Failed,
            AgentEventKind::Cancelled => Self::Cancelled,
            AgentEventKind::Completed => Self::Completed,
        }
    }
}

// AgentEvent projection preserves the sequence cursor and all optional payloads
// verbatim.  The FFI must not reorder, renumber, or use vector indices here.
// Event projection is protocol-sensitive: sequence remains monotonic, timestamp
// remains the runtime's RFC3339 value, and optional fields are copied without
// changing their meaning based on UI assumptions.
impl From<AgentEvent> for AgentEventDto {
    // Event projection keeps sequence and optional payloads untouched.  The FFI
    // must never renumber events based on the vector position.
    fn from(value: AgentEvent) -> Self {
        Self {
            run_id: value.run_id,
            sequence: value.sequence,
            timestamp: value.timestamp,
            kind: value.kind.into(),
            status: value.status.map(Into::into),
            text: value.text,
            tool_call_id: value.tool_call_id,
            tool_name: value.tool_name,
            permission: value.permission.map(Into::into),
            preview: value.preview,
            confirmation_id: value.confirmation_id,
            payload_json: value.payload_json,
            mode: value.mode.map(Into::into),
            stage: value.stage,
            progress: value.progress,
        }
    }
}

// Mode conversion keeps the public list synchronized with AgentRuntime's
// capability manifests and serialized mode names.
impl From<AgentMode> for AgentModeDto {
    // Mode values mirror the Agent capability manifest and remain explicit for
    // generated clients rather than relying on serde spelling conventions.
    fn from(value: AgentMode) -> Self {
        match value {
            AgentMode::Chat => Self::Chat,
            AgentMode::DeepSolve => Self::DeepSolve,
            AgentMode::Mastery => Self::Mastery,
            AgentMode::DeepResearch => Self::DeepResearch,
            AgentMode::QuestionLab => Self::QuestionLab,
            AgentMode::Visualize => Self::Visualize,
            AgentMode::Coach => Self::Coach,
            AgentMode::ExamSimulation => Self::ExamSimulation,
            AgentMode::ReversePlanner => Self::ReversePlanner,
        }
    }
}

// Incoming mode values are exhaustive and cannot introduce a mode that the
// runtime has not implemented.
impl From<AgentModeDto> for AgentMode {
    fn from(value: AgentModeDto) -> Self {
        match value {
            AgentModeDto::Chat => Self::Chat,
            AgentModeDto::DeepSolve => Self::DeepSolve,
            AgentModeDto::Mastery => Self::Mastery,
            AgentModeDto::DeepResearch => Self::DeepResearch,
            AgentModeDto::QuestionLab => Self::QuestionLab,
            AgentModeDto::Visualize => Self::Visualize,
            AgentModeDto::Coach => Self::Coach,
            AgentModeDto::ExamSimulation => Self::ExamSimulation,
            AgentModeDto::ReversePlanner => Self::ReversePlanner,
        }
    }
}

// Capability manifests are immutable snapshots of stage labels and loop caps;
// clients display them but do not get to mutate runtime limits.
impl From<CapabilityManifest> for CapabilityManifestDto {
    // Capability DTOs are snapshots of the runtime manifest.  Clients may
    // render stage labels, but loop caps remain enforced in AgentRuntime.
    fn from(value: CapabilityManifest) -> Self {
        Self {
            mode: value.mode.into(),
            title: value.title,
            description: value.description,
            stages: value.stages,
            max_loops: value.max_loops,
            tools_used: value.tools_used,
            result_kind: value.result_kind,
            request_schema_json: value.request_schema_json,
            config_defaults_json: value.config_defaults_json,
        }
    }
}

impl From<WorkspaceAgentTurn> for AgentTurnDto {
    fn from(value: WorkspaceAgentTurn) -> Self {
        Self {
            id: value.id,
            mode: value.mode,
            goal: value.goal,
            notebook_id: value.notebook_id,
            status: value.status,
            stage: value.stage,
            loop_index: value.loop_index,
            last_sequence: value.last_sequence,
            resume_safe: value.resume_safe,
            checkpoint: value.checkpoint,
            error: value.error,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl From<SourceRef> for SourceRefDto {
    fn from(value: SourceRef) -> Self {
        Self {
            source_type: value.source_type,
            locator: value.locator,
            title: value.title,
            excerpt: value.excerpt,
            tool_call_id: value.tool_call_id,
        }
    }
}

impl From<ArtifactRef> for ArtifactRefDto {
    fn from(value: ArtifactRef) -> Self {
        Self {
            artifact_id: value.artifact_id,
            path: value.path,
            extension: value.extension,
            render_type: value.render_type,
        }
    }
}

impl From<UsageSummary> for UsageSummaryDto {
    fn from(value: UsageSummary) -> Self {
        Self {
            prompt_tokens: value.prompt_tokens,
            completion_tokens: value.completion_tokens,
            total_tokens: value.total_tokens,
            model_calls: value.model_calls,
            estimated: value.estimated,
        }
    }
}

impl From<TurnResult> for TurnResultDto {
    fn from(value: TurnResult) -> Self {
        Self {
            schema_version: value.schema_version,
            mode: value.mode.into(),
            result_kind: value.result_kind,
            text: value.text,
            output_json: value.output_json,
            sources: value.sources.into_iter().map(Into::into).collect(),
            artifacts: value.artifacts.into_iter().map(Into::into).collect(),
            usage: value.usage.into(),
        }
    }
}

// BackupInspectionDto is a pre-apply report.  Conflict keys and warnings are
// exposed for UI resolution, while the staged archive remains Rust-owned.
// Backup inspection is intentionally a borrowed projection: creating the DTO
// does not consume or apply the staged inspection, so the same id can be used
// for a later resolution command.
impl From<&BackupInspection> for BackupInspectionDto {
    // Inspection is borrowed because the staged archive remains owned by the
    // facade until apply or cancel; projecting it must not consume that state.
    fn from(value: &BackupInspection) -> Self {
        Self {
            id: value.id.clone(),
            schema_version: value.manifest.schema_version,
            created_at: value.manifest.created_at.clone(),
            added_records: value.added_records,
            identical_records: value.identical_records,
            conflicts: value
                .conflicts
                .iter()
                .map(|conflict| BackupConflictDto {
                    key: conflict.key.clone(),
                    domain: conflict.domain.clone(),
                    record_id: conflict.record_id.clone(),
                    display_name: conflict.display_name.clone(),
                })
                .collect(),
            warnings: value.warnings.clone(),
        }
    }
}

// Import reports carry counts and the recovery path produced by Workspace after
// apply.  The facade does not hide recovery information from the client.
impl From<ImportReport> for ImportReportDto {
    // Applying a backup returns counts and a recovery path, so clients can
    // explain exactly what changed and where to recover if a later operation
    // needs to be reversed.
    // Import results include recovery information by design, allowing the UI to
    // surface the restore point instead of hiding a safety-critical path.
    fn from(value: ImportReport) -> Self {
        Self {
            imported_records: value.imported_records,
            kept_local_records: value.kept_local_records,
            recovery_path: value.recovery_path,
            warnings: value.warnings,
        }
    }
}

// Facade tests should exercise public lifecycle methods rather than private
// implementation details.  They are especially valuable here because a DTO
// conversion can compile while still changing a wire spelling or dropping an
// extension field.  Temporary Workspaces keep these checks local and prevent
// test state from touching a user's actual study data.
//
// Agent tests also assert that event sequence is exclusive, terminal states
// release the active slot, and confirmation remains required for writes.  The
// timer and backup tests cover process-local state separately from persisted
// Workspace records, preserving the distinction documented by the facade.
//
// Keeping these assertions near the facade also catches accidental changes to
// generated Swift signatures when a Rust domain type gains a new field.  A
// passing domain test alone would not prove that the transport projection is
// still compatible with existing desktop clients.
#[cfg(test)]
mod tests {
    // Facade tests exercise the public lifecycle through temporary Workspaces:
    // DTO conversion, Agent event/cursor behavior, SRS round trips, and the
    // guarantee that process-local state does not masquerade as persistence.
    use super::*;

    #[test]
    fn timer_requires_workspace_and_retains_state_when_persistence_fails() {
        let temp = tempfile::tempdir().unwrap();
        let core = StudyPulseCore::new();

        assert!(
            core.start_timer(SessionIntensityDto::Steady, 60, None)
                .is_err()
        );
        assert!(matches!(
            core.active_timer().status,
            TimerStatusKindDto::Idle
        ));

        let workspace_path = temp.path().join("Workspace");
        core.create_workspace(workspace_path.to_string_lossy().into_owned())
            .unwrap();
        let started = core
            .start_timer(SessionIntensityDto::Steady, 60, None)
            .unwrap();

        let sessions_path = workspace_path.join("Data/study_sessions.jsonl");
        std::fs::remove_file(&sessions_path).unwrap();
        std::fs::create_dir(&sessions_path).unwrap();
        assert!(core.finish_timer().is_err());

        let after_failure = core.active_timer();
        assert!(matches!(after_failure.status, TimerStatusKindDto::Running));
        assert_eq!(after_failure.session_id, started.session_id);

        std::fs::remove_dir(&sessions_path).unwrap();
        std::fs::File::create(&sessions_path).unwrap();
        let finished = core.finish_timer().unwrap();
        assert_eq!(finished.id, started.session_id.unwrap());
        assert!(matches!(
            core.active_timer().status,
            TimerStatusKindDto::Idle
        ));
        assert_eq!(core.get_study_sessions().unwrap().len(), 1);
    }

    #[test]
    fn timer_blocks_workspace_lifecycle_changes_and_checks_workspace_identity() {
        let temp = tempfile::tempdir().unwrap();
        let first_path = temp.path().join("First");
        let second_path = temp.path().join("Second");
        let core = StudyPulseCore::new();
        core.create_workspace(first_path.to_string_lossy().into_owned())
            .unwrap();
        Workspace::create(&second_path).unwrap();

        let started = core
            .start_timer(SessionIntensityDto::DeepFocus, 60, None)
            .unwrap();
        assert!(
            core.open_workspace(second_path.to_string_lossy().into_owned())
                .is_err()
        );
        assert!(core.close_workspace().is_err());
        assert!(
            core.rebuild_runtime(Arc::new(studypulse_model_client::MockModelClient))
                .is_err()
        );
        assert_eq!(core.active_timer().session_id, started.session_id);

        let first_workspace = core.workspace.lock().clone().unwrap();
        *core.workspace.lock() = None;
        assert!(core.finish_timer().is_err());
        assert_eq!(core.active_timer().session_id, started.session_id);
        *core.workspace.lock() = Some(first_workspace.clone());

        // Simulate an accidental internal Workspace replacement. The identity
        // check must still prevent the old timer from being written elsewhere,
        // and the timer must remain available for recovery.
        let second_workspace = Workspace::open(&second_path).unwrap();
        *core.workspace.lock() = Some(second_workspace.clone());
        assert!(core.finish_timer().is_err());
        assert_eq!(second_workspace.read_study_sessions().unwrap().len(), 0);
        assert_eq!(core.active_timer().session_id, started.session_id);
        *core.workspace.lock() = Some(first_workspace);

        core.cancel_timer().unwrap();
        core.open_workspace(second_path.to_string_lossy().into_owned())
            .unwrap();
        assert!(matches!(
            core.active_timer().status,
            TimerStatusKindDto::Idle
        ));
        assert!(
            core.start_timer(SessionIntensityDto::Light, 60, None)
                .is_ok()
        );
    }

    #[test]
    fn ffi_facade_runs_mock_agent() {
        let temp = tempfile::tempdir().unwrap();
        let core = StudyPulseCore::new();
        *core.model.lock() = Some(Arc::new(studypulse_model_client::MockModelClient));
        core.create_workspace(temp.path().join("Workspace").to_string_lossy().into_owned())
            .unwrap();
        let run_id = core
            .start_agent("Review chemistry".into(), Vec::new(), Vec::new())
            .unwrap();
        let mut cursor = 0;
        loop {
            let events = core
                .wait_for_agent_events(run_id.clone(), cursor, 500)
                .unwrap();
            if let Some(last) = events.last() {
                cursor = last.sequence;
            }
            if let Some(confirmation) = events
                .iter()
                .find_map(|event| event.confirmation_id.clone())
            {
                core.submit_confirmation(
                    run_id.clone(),
                    confirmation,
                    ConfirmationDecisionDto::Allow,
                )
                .unwrap();
            }
            if events
                .iter()
                .any(|event| matches!(event.kind, AgentEventKindDto::Completed))
            {
                break;
            }
        }
        assert_eq!(core.get_tasks().unwrap().len(), 1);
        assert_eq!(core.list_agent_turns().unwrap().len(), 1);
        let result = core.get_agent_turn_result(run_id).unwrap();
        assert_eq!(result.schema_version, 1);
        assert!(result.usage.model_calls >= 1);
    }

    #[test]
    fn p1_diary_srs_and_trends_round_trip_through_facade() {
        let temp = tempfile::tempdir().unwrap();
        let core = StudyPulseCore::new();
        core.create_workspace(temp.path().join("Workspace").to_string_lossy().into_owned())
            .unwrap();
        let now = "2026-07-31T00:00:00Z".to_string();
        let diary_id = uuid::Uuid::new_v4().to_string();
        core.upsert_diary_entry(DiaryEntryDto {
            id: diary_id.clone(),
            date: now.clone(),
            mood_score: 4,
            energy_score: 3,
            energy_tag: "focused".into(),
            content: "A short note".into(),
            phase_id: None,
            created_at: now.clone(),
            updated_at: now.clone(),
            extra_json: r#"{"futureField":true}"#.into(),
        })
        .unwrap();
        assert_eq!(core.get_diary_entries().unwrap()[0].id, diary_id);

        let mistake_id = uuid::Uuid::new_v4().to_string();
        core.upsert_mistake(MistakeNoteDto {
            id: mistake_id.clone(),
            title: "Fractions".into(),
            subject: "Math".into(),
            original_question: "1/2 + 1/3".into(),
            source: "Manual".into(),
            date: now,
            error_reason: "Skipped the common denominator".into(),
            wrong_solution: "2/5".into(),
            correct_solution: "5/6".into(),
            question_images: Vec::new(),
            reason_images: Vec::new(),
            wrong_solution_images: Vec::new(),
            correct_solution_images: Vec::new(),
            review_state: None,
            phase_id: None,
            exposure_count: 0,
            mastery_score: 0.0,
            mastery_history: Vec::new(),
            handwriting_history: Vec::new(),
            difficulty: 3,
            tags: Vec::new(),
            audio_file_name: None,
            extra_json: "{}".into(),
        })
        .unwrap();
        let enrolled = core.enroll_mistake(mistake_id.clone()).unwrap();
        assert_eq!(enrolled.repetitions, 0);
        assert_eq!(core.get_due_mistakes().unwrap().len(), 1);
        let reviewed = core.review_mistake(mistake_id, 5).unwrap();
        assert_eq!(reviewed.state.repetitions, 1);
        let trends = core.get_learning_trends(7).unwrap();
        assert_eq!(trends.srs.total_enrolled, 1);
        assert_eq!(trends.srs.due_count, 0);
        assert_eq!(trends.daily_points.len(), 7);
    }

    #[test]
    fn mistake_ai_patch_preserves_srs_history_and_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let core = StudyPulseCore::new();
        core.create_workspace(temp.path().join("Workspace").to_string_lossy().into_owned())
            .unwrap();
        let mistake_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        core.upsert_mistake(MistakeNoteDto {
            id: mistake_id.clone(),
            title: "Algebra".into(),
            subject: "Math".into(),
            original_question: "x / 2 = 3".into(),
            source: "Manual".into(),
            date: now,
            error_reason: "".into(),
            wrong_solution: "".into(),
            correct_solution: "".into(),
            question_images: Vec::new(),
            reason_images: Vec::new(),
            wrong_solution_images: Vec::new(),
            correct_solution_images: Vec::new(),
            review_state: None,
            phase_id: None,
            exposure_count: 0,
            mastery_score: 0.0,
            mastery_history: Vec::new(),
            handwriting_history: Vec::new(),
            difficulty: 3,
            tags: Vec::new(),
            audio_file_name: None,
            extra_json: "{}".into(),
        })
        .unwrap();
        core.enroll_mistake(mistake_id.clone()).unwrap();
        core.review_mistake(mistake_id.clone(), 4).unwrap();
        let patched = core.apply_mistake_ai_patch(
            mistake_id.clone(),
            r#"{"error_reason":"Divided instead of multiplying","wrong_solution":"x=1.5","correct_solution":"Multiply both sides by 2","tags":["inverse operation"],"result":{"confidence":0.9}}"#.into(),
        ).unwrap();
        assert_eq!(patched.review_state.as_ref().unwrap().repetitions, 1);
        assert_eq!(patched.mastery_history.len(), 1);
        assert_eq!(patched.correct_solution, "Multiply both sides by 2");
        core.save_mistake_ai_session(
            mistake_id.clone(),
            "similar_questions".into(),
            r#"{"questions":[]}"#.into(),
        )
        .unwrap();
        let stored = core.get_mistakes().unwrap().remove(0);
        let extra: serde_json::Value = serde_json::from_str(&stored.extra_json).unwrap();
        assert_eq!(extra["studypulseAiSessions"].as_array().unwrap().len(), 1);
        assert_eq!(extra["studypulseAiAnalysis"]["confidence"], 0.9);
    }
}
