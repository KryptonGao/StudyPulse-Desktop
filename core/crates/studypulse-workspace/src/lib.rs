//! Local-first Workspace storage and value-level analytics.
//!
//! The crate is the persistence boundary shared by the desktop host and FFI
//! layer.  It owns the on-disk format, validation rules, path guards, backup
//! transactions, and pure calculations that must remain compatible with the
//! iOS payloads.
//!
//! Most records use an `IosRecord<T>` JSONL envelope.  Keeping that envelope
//! here, instead of rebuilding it in callers, preserves unknown fields and
//! gives every read path the same duplicate-ID and envelope-ID checks.
//!
//! Public re-exports intentionally expose value types and safe Workspace
//! operations while keeping file-layout helpers private to this crate.
mod analytics;
mod backup;
mod features;
mod models;
mod platform;
mod safe_path;
mod workspace;

pub use analytics::{
    DailyTrendPoint, SrsOverview, SrsReviewResult, SubjectTrend, TimeInvestmentSummary,
    TodaySnapshot, TrendsSnapshot, apply_srs, current_streak, due_mistakes, investment_summary,
    learning_trends, srs_overview, today_snapshot,
};
pub use backup::{
    BackupConflict, BackupExportOptions, BackupExportResult, BackupInspection, BackupManifest,
    BackupResolution, ImportReport, RestoreMode,
};
pub use features::{
    CoachAnalysis, CoachAnalysisResponse, CoachChat, CoachConversationMessage, CoachData,
    CoachDataRow, CoachEvidence, CoachGoal, CoachGoalStatus, CoachGoalSubject, CoachPrediction,
    CoachProposal, CoachProposalItem, CoachProposalStatus, CoachRisk, CoachStopCondition,
    DailyExamTask, DailyReportPoint, ExamGoal, ExamGradeResponse, ExamPlan, ExamPlanPhase,
    ExamQuestion, ExamQuestionKind, ExamQuestionRecord, ExamQuestionResult, ExamRoleAnalysis,
    ExamSimulation, ExamSimulationEvent, ExamSimulationEventKind, ExamSimulationStatus,
    ExamWeakPoint, LearningReport, ReversePlannerResponse, coach_row_payload, decode_coach_payload,
    default_simulation, expired, learning_report, make_coach_row, parse_structured_json,
    proposal_task, validate_report_range,
};
pub use models::{
    AgentMessage, AgentMessageRole, AgentNotebook, AgentTurn, ComprehensiveExam,
    ComprehensiveExamFull, DiaryEntry, DifficultyAnnotation, Exam, ExamChecklistItem, ExamFull,
    ExamReview, ExamTimeSlot, FileEntry, GoalReward, Grade, HandwritingAnswerEntry,
    HeartRateSample, InvestmentTarget, IosRecord, MasteryHistoryEntry, MistakeNote,
    MistakeNoteFull, PhaseGoal, ReviewState, Routine, RoutineInstance, RoutineType, SearchMatch,
    SessionIntensity, StudyPhase, StudySession, StudySessionSource, SubTask, Subject, TaskItem,
    TaskType, TimeInvestmentSubject, TimeInvestmentTheme, WorkspaceInfo,
};
pub use safe_path::{SafeRelativePath, validate_wire_relative_path};
pub use workspace::Workspace;

use thiserror::Error;

/// Errors returned when a Workspace boundary rejects data or cannot complete
/// an operation.  The variants intentionally distinguish unsafe paths,
/// malformed records, unsupported versions, and ordinary I/O so callers can
/// present a useful failure without exposing implementation details.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    /// A wire or caller-provided path violates the relative-path policy.
    #[error("invalid workspace path: {0}")]
    InvalidPath(String),
    /// Canonicalization showed that access would leave the Workspace root.
    #[error("workspace access escaped its root: {0}")]
    PathEscape(String),
    /// A symlink/reparse point was found in a guarded Workspace path.
    #[error("symbolic links are not allowed in workspace operations: {0}")]
    SymbolicLink(String),
    /// Metadata is absent, malformed, or belongs to a newer/other format.
    #[error("workspace metadata is missing or invalid")]
    InvalidWorkspace,
    /// A multi-file operation could not obtain its process-local write guard.
    #[error("workspace is busy")]
    Busy,
    /// A typed record or JSONL line violates a model/storage invariant.
    #[error("data is malformed at {path}: {detail}")]
    MalformedData { path: String, detail: String },
    /// An archive failed path, size, checksum, or semantic backup validation.
    #[error("backup is invalid: {0}")]
    InvalidBackup(String),
    /// The archive schema is newer than the supported compatibility readers.
    #[error("backup schema {0} is not supported")]
    UnsupportedBackupSchema(u32),
    /// The caller attempted to apply/cancel a cleaned staging session.
    #[error("backup import session was not found")]
    ImportSessionNotFound,
    /// Underlying filesystem failure, kept separate from malformed data.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON syntax/serialization failure outside a line-aware parser.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// ZIP container failure while creating or extracting an archive.
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Result alias used by all Workspace operations and pure validation helpers.
pub type Result<T> = std::result::Result<T, WorkspaceError>;
