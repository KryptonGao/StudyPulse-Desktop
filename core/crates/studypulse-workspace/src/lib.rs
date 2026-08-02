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
    AgentMessage, AgentMessageRole, AgentNotebook, ComprehensiveExam, ComprehensiveExamFull,
    DiaryEntry, DifficultyAnnotation, Exam, ExamChecklistItem, ExamFull, ExamReview, ExamTimeSlot,
    FileEntry, GoalReward, Grade, HandwritingAnswerEntry, HeartRateSample, InvestmentTarget,
    IosRecord, MasteryHistoryEntry, MistakeNote, MistakeNoteFull, PhaseGoal, ReviewState, Routine,
    RoutineInstance, RoutineType, SearchMatch, SessionIntensity, StudyPhase, StudySession,
    StudySessionSource, SubTask, Subject, TaskItem, TaskType, TimeInvestmentSubject,
    TimeInvestmentTheme, WorkspaceInfo,
};
pub use safe_path::{SafeRelativePath, validate_wire_relative_path};
pub use workspace::Workspace;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("invalid workspace path: {0}")]
    InvalidPath(String),
    #[error("workspace access escaped its root: {0}")]
    PathEscape(String),
    #[error("symbolic links are not allowed in workspace operations: {0}")]
    SymbolicLink(String),
    #[error("workspace metadata is missing or invalid")]
    InvalidWorkspace,
    #[error("workspace is busy")]
    Busy,
    #[error("data is malformed at {path}: {detail}")]
    MalformedData { path: String, detail: String },
    #[error("backup is invalid: {0}")]
    InvalidBackup(String),
    #[error("backup schema {0} is not supported")]
    UnsupportedBackupSchema(u32),
    #[error("backup import session was not found")]
    ImportSessionNotFound,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;
