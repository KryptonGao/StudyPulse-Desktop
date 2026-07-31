use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use parking_lot::Mutex;
use studypulse_agent::{
    AgentEvent, AgentEventKind, AgentMode, AgentRuntime, CapabilityManifest, ConfirmationDecision,
    ConversationMessage, ConversationRole, RunStatus, capability_manifests,
};
use studypulse_model_client::{
    ByokConfig, CloudAuthTokens, CloudModelClient, CloudProfile, DEFAULT_CLOUD_API_BASE_URL,
    ModelClient, OpenAICompatibleModelClient,
};
use studypulse_tools::PermissionLevel;
use studypulse_workspace::{
    AgentMessage, AgentMessageRole, AgentNotebook, BackupExportOptions, BackupInspection,
    BackupResolution, ComprehensiveExamFull, DifficultyAnnotation, ExamChecklistItem, ExamFull,
    ExamReview, ExamTimeSlot, FileEntry, GoalReward, Grade, HandwritingAnswerEntry,
    HeartRateSample, ImportReport, InvestmentTarget, MasteryHistoryEntry, MistakeNoteFull,
    PhaseGoal, RestoreMode, ReviewState, Routine, RoutineInstance, RoutineType, SearchMatch,
    SessionIntensity, StudyPhase, StudySession, StudySessionSource, SubTask, Subject, TaskItem,
    TaskType, TimeInvestmentSubject, TimeInvestmentSummary, TimeInvestmentTheme, TodaySnapshot,
    Workspace, WorkspaceInfo,
};
use thiserror::Error;

uniffi::setup_scaffolding!();

#[derive(Debug, Error, uniffi::Error)]
pub enum CoreError {
    #[error("{message}")]
    Failure { message: String },
}

impl CoreError {
    fn message(error: impl std::fmt::Display) -> Self {
        Self::Failure {
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct WorkspaceDto {
    pub id: String,
    pub root_path: String,
    pub schema_version: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CloudAuthTokensDto {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CloudAccountDto {
    pub email: String,
    pub role: String,
    pub membership_type: String,
    pub membership_expires_at: Option<String>,
    pub plan_name: String,
    pub available_models: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ByokConfigDto {
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum TaskTypeDto {
    Homework,
    Reading,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct SubjectDto {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub full_score: f64,
    pub display_name: String,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PhaseGoalDto {
    pub id: String,
    pub subject: String,
    pub target_score: f64,
    pub notes: String,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct ReviewStateDto {
    pub repetitions: i64,
    pub ease_factor: f64,
    pub interval_days: i64,
    pub next_review_date: String,
    pub last_review_date: Option<String>,
    pub lapses: i64,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MasteryHistoryEntryDto {
    pub id: String,
    pub timestamp: String,
    pub score: f64,
    pub quality: i64,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct HandwritingAnswerEntryDto {
    pub id: String,
    pub timestamp: String,
    pub image_base64: String,
    pub quality: i64,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExamTimeSlotDto {
    pub start_time: String,
    pub end_time: String,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExamChecklistItemDto {
    pub id: String,
    pub title: String,
    pub is_checked: bool,
    pub sort_order: i64,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExamDto {
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

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum RoutineTypeDto {
    MistakeReview,
    Flashcard,
    General,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum SessionIntensityDto {
    Peak,
    DeepFocus,
    Steady,
    Light,
    Recovery,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum StudySessionSourceDto {
    Timer,
    Manual,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct InvestmentTargetDto {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct HeartRateSampleDto {
    pub id: String,
    pub timestamp: String,
    pub bpm: f64,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DifficultyAnnotationDto {
    pub id: String,
    pub timestamp: String,
    pub heart_rate: Option<f64>,
    pub note: String,
    pub subject_id: Option<String>,
    pub extra_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct StudySessionDto {
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

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum TimeInvestmentThemeDto {
    Ocean,
    Coral,
    Violet,
    Sunshine,
    Mint,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct TimeInvestmentSummaryDto {
    pub target_id: String,
    pub direct_seconds: i64,
    pub total_seconds: i64,
    pub session_count: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct SrsReviewResultDto {
    pub state: ReviewStateDto,
    pub next_review_date: String,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum TimerStatusKindDto {
    Idle,
    Running,
    Paused,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TimerSnapshotDto {
    pub status: TimerStatusKindDto,
    pub session_id: Option<String>,
    pub started_at: Option<String>,
    pub elapsed_seconds: i64,
    pub target_duration_seconds: i64,
    pub intensity: Option<SessionIntensityDto>,
    pub investment_target: Option<InvestmentTargetDto>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BackupExportOptionsDto {
    pub archive_path: String,
    pub includes_media: bool,
    pub includes_derived_health_data: bool,
    pub app_version: String,
    pub app_build: String,
    pub locale: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BackupExportResultDto {
    pub archive_path: String,
    pub schema_version: u32,
    pub record_counts_json: String,
    pub warnings: Vec<String>,
}

struct ActiveTimer {
    session_id: uuid::Uuid,
    started_at: String,
    running_since: Option<std::time::Instant>,
    elapsed_before_pause: i64,
    target_duration_seconds: i64,
    intensity: SessionIntensityDto,
    investment_target: Option<InvestmentTargetDto>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FileEntryDto {
    pub relative_path: String,
    pub is_directory: bool,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AgentMessageRoleDto {
    User,
    Assistant,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AgentMessageDto {
    pub id: String,
    pub role: AgentMessageRoleDto,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AgentNotebookDto {
    pub id: String,
    pub title: String,
    pub source_paths: Vec<String>,
    pub messages: Vec<AgentMessageDto>,
    pub last_goal: String,
    pub last_answer: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AgentModeDto {
    Chat,
    DeepSolve,
    Mastery,
    DeepResearch,
    QuestionLab,
    Visualize,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CapabilityManifestDto {
    pub mode: AgentModeDto,
    pub title: String,
    pub description: String,
    pub stages: Vec<String>,
    pub max_loops: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchMatchDto {
    pub relative_path: String,
    pub line_number: Option<u32>,
    pub snippet: String,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum PermissionDto {
    Read,
    Write,
    Destructive,
    Execute,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum RunStatusDto {
    Started,
    Running,
    WaitingForConfirmation,
    Cancelling,
    Failed,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
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
    Failed,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, uniffi::Record)]
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

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum ConfirmationDecisionDto {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum RestoreModeDto {
    Replace,
    Merge,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BackupConflictDto {
    pub key: String,
    pub domain: String,
    pub record_id: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BackupInspectionDto {
    pub id: String,
    pub schema_version: u32,
    pub created_at: String,
    pub added_records: u64,
    pub identical_records: u64,
    pub conflicts: Vec<BackupConflictDto>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BackupResolutionDto {
    pub conflict_key: String,
    pub use_incoming: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportReportDto {
    pub imported_records: u64,
    pub kept_local_records: u64,
    pub recovery_path: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct OperationEventDto {
    pub operation_id: String,
    pub sequence: u64,
    pub kind: String,
    pub progress: f64,
    pub message: String,
}

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
    active_timer: Mutex<Option<ActiveTimer>>,
    restore_active: AtomicBool,
}

#[uniffi::export]
impl StudyPulseCore {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
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
            active_timer: Mutex::new(None),
            restore_active: AtomicBool::new(false),
        })
    }

    pub fn create_workspace(&self, path: String) -> Result<WorkspaceDto, CoreError> {
        self.ensure_no_active_run()?;
        let workspace = Workspace::create(path).map_err(CoreError::message)?;
        let dto = workspace.info().into();
        self.install_workspace(workspace);
        Ok(dto)
    }

    pub fn open_workspace(&self, path: String) -> Result<WorkspaceDto, CoreError> {
        self.ensure_no_active_run()?;
        let workspace = Workspace::open(path).map_err(CoreError::message)?;
        let dto = workspace.info().into();
        self.install_workspace(workspace);
        Ok(dto)
    }

    pub fn close_workspace(&self) -> Result<(), CoreError> {
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
        *self.runtime.lock() = None;
        *self.workspace.lock() = None;
        *self.last_run_id.lock() = None;
        self.inspections.lock().clear();
        Ok(())
    }

    pub fn current_workspace(&self) -> Option<WorkspaceDto> {
        self.workspace
            .lock()
            .as_ref()
            .map(|value| value.info().into())
    }

    pub fn cloud_ai_login_url(&self) -> Result<String, CoreError> {
        CloudModelClient::login_url("studypulse://auth/callback").map_err(CoreError::message)
    }

    pub fn parse_cloud_ai_auth_callback(
        &self,
        callback_url: String,
    ) -> Result<CloudAuthTokensDto, CoreError> {
        CloudModelClient::parse_auth_callback(&callback_url)
            .map(Into::into)
            .map_err(CoreError::message)
    }

    pub fn connect_cloud_ai(
        &self,
        access_token: String,
        refresh_token: String,
    ) -> Result<CloudAccountDto, CoreError> {
        self.ensure_no_active_run()?;
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
        *self.model.lock() = Some(Arc::clone(&model));
        *self.cloud_client.lock() = Some(client);
        *self.cloud_account.lock() = Some(account.clone());
        *self.byok_client.lock() = None;
        *self.byok_config.lock() = None;
        self.rebuild_runtime(model);
        Ok(account)
    }

    pub fn connect_byok(
        &self,
        api_key: String,
        base_url: String,
        model: String,
    ) -> Result<ByokConfigDto, CoreError> {
        self.ensure_no_active_run()?;
        let client = OpenAICompatibleModelClient::new(&base_url, api_key, model)
            .map_err(CoreError::message)?;
        let config = ByokConfigDto::from(client.config());
        let model: Arc<dyn ModelClient> = Arc::new(client.clone());
        *self.model.lock() = Some(Arc::clone(&model));
        *self.cloud_client.lock() = None;
        *self.cloud_account.lock() = None;
        *self.byok_client.lock() = Some(client);
        *self.byok_config.lock() = Some(config.clone());
        self.rebuild_runtime(model);
        Ok(config)
    }

    pub fn refresh_cloud_ai(&self, refresh_token: String) -> Result<CloudAuthTokensDto, CoreError> {
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
        self.ensure_no_active_run()?;
        let client = self.cloud_client.lock().take();
        *self.cloud_account.lock() = None;
        if client.is_some() {
            *self.model.lock() = None;
            *self.runtime.lock() = None;
            *self.last_run_id.lock() = None;
        }
        if let Some(client) = client {
            run_async(client.logout()).map_err(CoreError::message)?;
        }
        Ok(())
    }

    pub fn disconnect_byok(&self) -> Result<(), CoreError> {
        self.ensure_no_active_run()?;
        let client = self.byok_client.lock().take();
        *self.byok_config.lock() = None;
        if client.is_some() {
            *self.model.lock() = None;
            *self.runtime.lock() = None;
            *self.last_run_id.lock() = None;
        }
        Ok(())
    }

    pub fn cloud_ai_account(&self) -> Option<CloudAccountDto> {
        self.cloud_account.lock().clone()
    }

    pub fn start_agent(
        &self,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<AgentMessageDto>,
    ) -> Result<String, CoreError> {
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

    pub fn list_agent_capabilities(&self) -> Vec<CapabilityManifestDto> {
        capability_manifests().into_iter().map(Into::into).collect()
    }

    pub fn cancel_agent(&self, run_id: String) -> Result<(), CoreError> {
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
        self.runtime()?
            .submit_input(&run_id, &input_id, answer_json)
            .map_err(CoreError::message)
    }

    pub fn get_run_state(&self, run_id: String) -> Result<RunStatusDto, CoreError> {
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
        self.runtime()?
            .wait_for_events(&run_id, after_sequence, timeout_ms)
            .map(|events| events.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn get_tasks(&self) -> Result<Vec<TaskDto>, CoreError> {
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
        self.workspace()?
            .read_mistakes()
            .map(|values| values.into_iter().map(Into::into).collect())
            .map_err(CoreError::message)
    }

    pub fn get_due_mistakes(&self) -> Result<Vec<MistakeNoteDto>, CoreError> {
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

    pub fn get_exams(&self) -> Result<Vec<ExamDto>, CoreError> {
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

    pub fn get_routines(&self) -> Result<Vec<RoutineDto>, CoreError> {
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

    pub fn get_today_snapshot(&self) -> Result<TodaySnapshotDto, CoreError> {
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
        self.ensure_no_active_run()?;
        if target_duration_seconds < 0 {
            return Err(CoreError::message("timer duration cannot be negative"));
        }
        let mut timer = self.active_timer.lock();
        if timer.is_some() {
            return Err(CoreError::message("a timer is already active"));
        }
        let value = ActiveTimer {
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
        let mut timer = self.active_timer.lock();
        let value = timer
            .as_mut()
            .ok_or_else(|| CoreError::message("no active timer"))?;
        if let Some(running_since) = value.running_since.take() {
            value.elapsed_before_pause += running_since.elapsed().as_secs() as i64;
        }
        Ok(timer_snapshot(value))
    }

    pub fn resume_timer(&self) -> Result<TimerSnapshotDto, CoreError> {
        let mut timer = self.active_timer.lock();
        let value = timer
            .as_mut()
            .ok_or_else(|| CoreError::message("no active timer"))?;
        if value.running_since.is_none() {
            value.running_since = Some(std::time::Instant::now());
        }
        Ok(timer_snapshot(value))
    }

    pub fn finish_timer(&self) -> Result<StudySessionDto, CoreError> {
        let mut timer = self.active_timer.lock();
        let value = timer
            .take()
            .ok_or_else(|| CoreError::message("no active timer"))?;
        let elapsed = elapsed_seconds(&value);
        let session = StudySession {
            id: value.session_id,
            start_date: value.started_at,
            duration_seconds: elapsed,
            intensity: value.intensity.into(),
            completed: true,
            heart_rate_samples: None,
            difficulty_annotations: None,
            investment_target: value.investment_target.map(TryInto::try_into).transpose()?,
            source: StudySessionSource::Timer,
            time_zone_identifier: Some("UTC".into()),
            extra: BTreeMap::new(),
        };
        self.workspace()?
            .upsert_study_session(session.clone())
            .map_err(CoreError::message)?;
        Ok(session.into())
    }

    pub fn cancel_timer(&self) -> Result<(), CoreError> {
        let mut timer = self.active_timer.lock();
        if timer.take().is_none() {
            return Err(CoreError::message("no active timer"));
        }
        Ok(())
    }

    pub fn active_timer(&self) -> TimerSnapshotDto {
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
        self.workspace()?
            .read_media(&relative_path)
            .map_err(CoreError::message)
    }

    pub fn write_media(
        &self,
        relative_path: String,
        contents: Vec<u8>,
    ) -> Result<String, CoreError> {
        self.workspace()?
            .write_media(&relative_path, &contents)
            .map_err(CoreError::message)
    }

    pub fn export_backup(
        &self,
        options: BackupExportOptionsDto,
    ) -> Result<BackupExportResultDto, CoreError> {
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
    fn ensure_no_active_run(&self) -> Result<(), CoreError> {
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

    fn install_workspace(&self, workspace: Workspace) {
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
    }

    fn rebuild_runtime(&self, model: Arc<dyn ModelClient>) {
        let workspace = self.workspace.lock().clone();
        *self.runtime.lock() = workspace.map(|workspace| AgentRuntime::new(workspace, model));
        *self.last_run_id.lock() = None;
    }

    fn workspace(&self) -> Result<Workspace, CoreError> {
        self.workspace
            .lock()
            .as_ref()
            .cloned()
            .ok_or_else(|| CoreError::message("no Workspace is open"))
    }

    fn runtime(&self) -> Result<Arc<AgentRuntime>, CoreError> {
        self.runtime.lock().as_ref().cloned().ok_or_else(|| {
            if self.workspace.lock().is_some() {
                CoreError::message("Cloud AI is not connected")
            } else {
                CoreError::message("no Workspace is open")
            }
        })
    }
}

fn run_async<T>(
    future: impl std::future::Future<Output = Result<T, studypulse_model_client::ModelError>>,
) -> Result<T, studypulse_model_client::ModelError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| studypulse_model_client::ModelError::Request(error.to_string()))?
        .block_on(future)
}

struct RestoreFlagGuard<'a>(&'a AtomicBool);

impl Drop for RestoreFlagGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl From<WorkspaceInfo> for WorkspaceDto {
    fn from(value: WorkspaceInfo) -> Self {
        Self {
            id: value.id,
            root_path: value.root_path,
            schema_version: value.schema_version,
        }
    }
}

fn parse_uuid(value: &str) -> Result<uuid::Uuid, CoreError> {
    value
        .parse()
        .map_err(|error| CoreError::message(format!("invalid UUID {value}: {error}")))
}

fn parse_optional_uuid(value: Option<String>) -> Result<Option<uuid::Uuid>, CoreError> {
    value.as_deref().map(parse_uuid).transpose()
}

fn encode_extra(value: &BTreeMap<String, serde_json::Value>) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".into())
}

fn decode_extra(value: &str) -> Result<BTreeMap<String, serde_json::Value>, CoreError> {
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
    serde_json::from_str(value).map_err(|error| CoreError::message(error.to_string()))
}

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

impl From<Grade> for GradeDto {
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

impl TryFrom<GradeDto> for Grade {
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

impl From<MistakeNoteFull> for MistakeNoteDto {
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

impl From<ExamTimeSlot> for ExamTimeSlotDto {
    fn from(value: ExamTimeSlot) -> Self {
        Self {
            start_time: value.start_time,
            end_time: value.end_time,
            extra_json: encode_extra(&value.extra),
        }
    }
}

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

impl From<ExamFull> for ExamDto {
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

impl TryFrom<ExamDto> for ExamFull {
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

impl From<ComprehensiveExamFull> for ComprehensiveExamDto {
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

impl From<RoutineType> for RoutineTypeDto {
    fn from(value: RoutineType) -> Self {
        match value {
            RoutineType::MistakeReview => Self::MistakeReview,
            RoutineType::Flashcard => Self::Flashcard,
            RoutineType::General => Self::General,
        }
    }
}

impl From<RoutineTypeDto> for RoutineType {
    fn from(value: RoutineTypeDto) -> Self {
        match value {
            RoutineTypeDto::MistakeReview => Self::MistakeReview,
            RoutineTypeDto::Flashcard => Self::Flashcard,
            RoutineTypeDto::General => Self::General,
        }
    }
}

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

impl From<StudySession> for StudySessionDto {
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

impl TryFrom<StudySessionDto> for StudySession {
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

impl From<TimeInvestmentSubject> for TimeInvestmentSubjectDto {
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

impl TryFrom<TimeInvestmentSubjectDto> for TimeInvestmentSubject {
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

impl From<GoalReward> for GoalRewardDto {
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

impl TryFrom<GoalRewardDto> for GoalReward {
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

impl From<studypulse_workspace::SrsReviewResult> for SrsReviewResultDto {
    fn from(value: studypulse_workspace::SrsReviewResult) -> Self {
        Self {
            state: value.state.into(),
            next_review_date: value.next_review_date,
        }
    }
}

fn elapsed_seconds(timer: &ActiveTimer) -> i64 {
    timer.elapsed_before_pause
        + timer
            .running_since
            .map(|started| started.elapsed().as_secs() as i64)
            .unwrap_or(0)
}

fn timer_snapshot(timer: &ActiveTimer) -> TimerSnapshotDto {
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

impl From<CloudAuthTokens> for CloudAuthTokensDto {
    fn from(value: CloudAuthTokens) -> Self {
        Self {
            access_token: value.access_token,
            refresh_token: value.refresh_token,
        }
    }
}

impl From<CloudProfile> for CloudAccountDto {
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

impl From<ByokConfig> for ByokConfigDto {
    fn from(value: ByokConfig) -> Self {
        Self {
            base_url: value.base_url,
            model: value.model,
        }
    }
}

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

impl From<AgentNotebook> for AgentNotebookDto {
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

impl TryFrom<AgentNotebookDto> for AgentNotebook {
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
        }
    }
}

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
        })
    }
}

impl From<SearchMatch> for SearchMatchDto {
    fn from(value: SearchMatch) -> Self {
        Self {
            relative_path: value.relative_path,
            line_number: value.line_number,
            snippet: value.snippet,
        }
    }
}

impl From<PermissionLevel> for PermissionDto {
    fn from(value: PermissionLevel) -> Self {
        match value {
            PermissionLevel::Read => Self::Read,
            PermissionLevel::Write => Self::Write,
            PermissionLevel::Destructive => Self::Destructive,
            PermissionLevel::Execute => Self::Execute,
        }
    }
}

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

impl From<AgentEventKind> for AgentEventKindDto {
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
            AgentEventKind::Failed => Self::Failed,
            AgentEventKind::Cancelled => Self::Cancelled,
            AgentEventKind::Completed => Self::Completed,
        }
    }
}

impl From<AgentEvent> for AgentEventDto {
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

impl From<AgentMode> for AgentModeDto {
    fn from(value: AgentMode) -> Self {
        match value {
            AgentMode::Chat => Self::Chat,
            AgentMode::DeepSolve => Self::DeepSolve,
            AgentMode::Mastery => Self::Mastery,
            AgentMode::DeepResearch => Self::DeepResearch,
            AgentMode::QuestionLab => Self::QuestionLab,
            AgentMode::Visualize => Self::Visualize,
        }
    }
}

impl From<AgentModeDto> for AgentMode {
    fn from(value: AgentModeDto) -> Self {
        match value {
            AgentModeDto::Chat => Self::Chat,
            AgentModeDto::DeepSolve => Self::DeepSolve,
            AgentModeDto::Mastery => Self::Mastery,
            AgentModeDto::DeepResearch => Self::DeepResearch,
            AgentModeDto::QuestionLab => Self::QuestionLab,
            AgentModeDto::Visualize => Self::Visualize,
        }
    }
}

impl From<CapabilityManifest> for CapabilityManifestDto {
    fn from(value: CapabilityManifest) -> Self {
        Self {
            mode: value.mode.into(),
            title: value.title,
            description: value.description,
            stages: value.stages,
            max_loops: value.max_loops,
        }
    }
}

impl From<&BackupInspection> for BackupInspectionDto {
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

impl From<ImportReport> for ImportReportDto {
    fn from(value: ImportReport) -> Self {
        Self {
            imported_records: value.imported_records,
            kept_local_records: value.kept_local_records,
            recovery_path: value.recovery_path,
            warnings: value.warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
