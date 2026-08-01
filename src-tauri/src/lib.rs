use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use keyring::Entry;
use serde::{Deserialize, Serialize};
use studypulse_ffi::{
    AgentEventDto, AgentMessageDto, AgentModeDto, AgentNotebookDto, BackupExportOptionsDto,
    BackupExportResultDto, BackupInspectionDto, BackupResolutionDto, ByokConfigDto,
    CloudAccountDto, CloudAuthTokensDto, ConfirmationDecisionDto, CoreError, DiaryEntryDto,
    ExamDto, FileEntryDto, GradeDto, ImportReportDto, MistakeNoteDto, OperationEventDto,
    RestoreModeDto, ReviewStateDto, RunStatusDto, SearchMatchDto, SessionIntensityDto,
    StudyPhaseDto, StudySessionDto, SubjectDto, TaskDto, TimeInvestmentSubjectDto,
    TimerSnapshotDto, TodaySnapshotDto, TrendsSnapshotDto, WorkspaceDto,
};
use tauri::{Manager, State};
use thiserror::Error;

const CLOUD_SERVICE: &str = "space.chenkai.StudyPulse-Desktop.CloudAI";
const CLOUD_ACCOUNT: &str = "session-token-pair";
const BYOK_SERVICE: &str = "space.chenkai.StudyPulse-Desktop.BYOK";
const BYOK_ACCOUNT: &str = "openai-compatible";
const MAX_SOURCE_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredCloudTokens {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredByok {
    api_key: String,
    base_url: String,
    model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Preferences {
    workspace_path: Option<String>,
    provider: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub cloud_account: Option<CloudAccountDto>,
    pub byok_config: Option<ByokConfigDto>,
    pub has_saved_byok: bool,
    pub active_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSnapshot {
    pub workspace: Option<WorkspaceDto>,
    pub provider: ProviderStatus,
}

#[derive(Debug, Error, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AppError {
    #[error("Core operation failed: {message}")]
    Core { message: String },
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
    #[error("Credential storage failed: {message}")]
    Credentials { message: String },
    #[error("File operation failed: {message}")]
    File { message: String },
    #[error("Application state failed: {message}")]
    State { message: String },
}

impl From<CoreError> for AppError {
    fn from(value: CoreError) -> Self {
        Self::Core {
            message: value.to_string(),
        }
    }
}

impl From<keyring::Error> for AppError {
    fn from(value: keyring::Error) -> Self {
        Self::Credentials {
            message: value.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<studypulse_ffi::StudyPulseCore>,
    preferences_path: PathBuf,
    preferences: Arc<Mutex<Preferences>>,
    byok_config: Arc<Mutex<Option<ByokConfigDto>>>,
}

impl AppState {
    fn new(app: &tauri::AppHandle) -> Result<Self, AppError> {
        let directory = app
            .path()
            .app_config_dir()
            .map_err(|error| AppError::State {
                message: error.to_string(),
            })?;
        fs::create_dir_all(&directory).map_err(|error| AppError::File {
            message: error.to_string(),
        })?;
        let preferences_path = directory.join("preferences.json");
        let preferences = read_preferences(&preferences_path)?;
        let state = Self {
            core: studypulse_ffi::StudyPulseCore::new(),
            preferences_path,
            preferences: Arc::new(Mutex::new(preferences)),
            byok_config: Arc::new(Mutex::new(None)),
        };
        if let Some(path) = state.preferences_snapshot()?.workspace_path {
            if Path::new(&path).is_dir() {
                let _ = state.core.open_workspace(path);
            }
        }
        Ok(state)
    }

    fn preferences_snapshot(&self) -> Result<Preferences, AppError> {
        self.preferences
            .lock()
            .map(|value| value.clone())
            .map_err(|_| AppError::State {
                message: "preferences lock poisoned".into(),
            })
    }

    fn update_preferences(&self, update: impl FnOnce(&mut Preferences)) -> Result<(), AppError> {
        let snapshot = {
            let mut preferences = self.preferences.lock().map_err(|_| AppError::State {
                message: "preferences lock poisoned".into(),
            })?;
            update(&mut preferences);
            preferences.clone()
        };
        let data = serde_json::to_vec_pretty(&snapshot).map_err(|error| AppError::State {
            message: error.to_string(),
        })?;
        fs::write(&self.preferences_path, data).map_err(|error| AppError::File {
            message: error.to_string(),
        })
    }
}

fn read_preferences(path: &Path) -> Result<Preferences, AppError> {
    match fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).map_err(|error| AppError::State {
            message: error.to_string(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(error) => Err(AppError::File {
            message: error.to_string(),
        }),
    }
}

fn keyring_entry(service: &str, account: &str) -> Result<Entry, AppError> {
    Entry::new(service, account).map_err(AppError::from)
}

fn load_cloud_tokens() -> Result<Option<StoredCloudTokens>, AppError> {
    let entry = keyring_entry(CLOUD_SERVICE, CLOUD_ACCOUNT)?;
    match entry.get_password() {
        Ok(value) => {
            serde_json::from_str(&value)
                .map(Some)
                .map_err(|error| AppError::Credentials {
                    message: error.to_string(),
                })
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn save_cloud_tokens(tokens: &CloudAuthTokensDto) -> Result<(), AppError> {
    let entry = keyring_entry(CLOUD_SERVICE, CLOUD_ACCOUNT)?;
    let value = serde_json::to_string(&StoredCloudTokens {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone(),
    })
    .map_err(|error| AppError::Credentials {
        message: error.to_string(),
    })?;
    entry.set_password(&value).map_err(AppError::from)
}

fn clear_cloud_tokens() -> Result<(), AppError> {
    let entry = keyring_entry(CLOUD_SERVICE, CLOUD_ACCOUNT)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn load_byok() -> Result<Option<StoredByok>, AppError> {
    let entry = keyring_entry(BYOK_SERVICE, BYOK_ACCOUNT)?;
    match entry.get_password() {
        Ok(value) => {
            serde_json::from_str(&value)
                .map(Some)
                .map_err(|error| AppError::Credentials {
                    message: error.to_string(),
                })
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn save_byok(value: &StoredByok) -> Result<(), AppError> {
    let entry = keyring_entry(BYOK_SERVICE, BYOK_ACCOUNT)?;
    let encoded = serde_json::to_string(value).map_err(|error| AppError::Credentials {
        message: error.to_string(),
    })?;
    entry.set_password(&encoded).map_err(AppError::from)
}

fn clear_byok() -> Result<(), AppError> {
    let entry = keyring_entry(BYOK_SERVICE, BYOK_ACCOUNT)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn provider_status(state: &AppState) -> Result<ProviderStatus, AppError> {
    let preferences = state.preferences_snapshot()?;
    let byok_config = state
        .byok_config
        .lock()
        .map_err(|_| AppError::State {
            message: "BYOK state lock poisoned".into(),
        })?
        .clone();
    Ok(ProviderStatus {
        cloud_account: state.core.cloud_ai_account(),
        byok_config,
        has_saved_byok: load_byok()?.is_some(),
        active_provider: preferences.provider,
    })
}

async fn core_call<T, F>(state: State<'_, AppState>, operation: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(Arc<studypulse_ffi::StudyPulseCore>) -> Result<T, CoreError> + Send + 'static,
{
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || operation(core))
        .await
        .map_err(|error| AppError::State {
            message: error.to_string(),
        })?
        .map_err(AppError::from)
}

#[tauri::command]
async fn app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, AppError> {
    let provider = provider_status(&state)?;
    Ok(AppSnapshot {
        workspace: state.core.current_workspace(),
        provider,
    })
}

#[tauri::command]
async fn create_workspace(
    path: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceDto, AppError> {
    let core_path = path.clone();
    let result = core_call(state.clone(), move |core| core.create_workspace(core_path)).await?;
    state.update_preferences(|preferences| preferences.workspace_path = Some(path))?;
    Ok(result)
}

#[tauri::command]
async fn open_workspace(
    path: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceDto, AppError> {
    let core_path = path.clone();
    let result = core_call(state.clone(), move |core| core.open_workspace(core_path)).await?;
    state.update_preferences(|preferences| preferences.workspace_path = Some(path))?;
    Ok(result)
}

#[tauri::command]
async fn close_workspace(state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, |core| core.close_workspace()).await
}

#[tauri::command]
async fn cloud_ai_login_url(state: State<'_, AppState>) -> Result<String, AppError> {
    core_call(state, |core| core.cloud_ai_login_url()).await
}

#[tauri::command]
async fn complete_cloud_ai_auth(
    callback_url: String,
    state: State<'_, AppState>,
) -> Result<ProviderStatus, AppError> {
    let tokens = core_call(state.clone(), move |core| {
        core.parse_cloud_ai_auth_callback(callback_url)
    })
    .await?;
    let stored_tokens = tokens.clone();
    core_call(state.clone(), move |core| {
        core.connect_cloud_ai(tokens.access_token.clone(), tokens.refresh_token.clone())
    })
    .await?;
    save_cloud_tokens(&stored_tokens)?;
    *state.byok_config.lock().map_err(|_| AppError::State {
        message: "BYOK state lock poisoned".into(),
    })? = None;
    state.update_preferences(|preferences| preferences.provider = Some("cloud".into()))?;
    provider_status(&state)
}

#[derive(Debug, Clone, Deserialize)]
pub struct ByokInput {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

#[tauri::command]
async fn save_byok_configuration(
    input: ByokInput,
    state: State<'_, AppState>,
) -> Result<ProviderStatus, AppError> {
    let previous = load_byok()?;
    let api_key = if input.api_key.trim().is_empty() {
        previous
            .as_ref()
            .map(|value| value.api_key.clone())
            .ok_or_else(|| AppError::InvalidInput {
                message: "API key is required".into(),
            })?
    } else {
        input.api_key.trim().to_owned()
    };
    let base_url = input.base_url.trim().to_owned();
    let model = input.model.trim().to_owned();
    let core_api_key = api_key.clone();
    let core_base_url = base_url.clone();
    let core_model = model.clone();
    let config = core_call(state.clone(), move |core| {
        core.connect_byok(core_api_key, core_base_url, core_model)
    })
    .await?;
    save_byok(&StoredByok {
        api_key,
        base_url,
        model,
    })?;
    *state.byok_config.lock().map_err(|_| AppError::State {
        message: "BYOK state lock poisoned".into(),
    })? = Some(config.clone());
    state.update_preferences(|preferences| preferences.provider = Some("byok".into()))?;
    Ok(ProviderStatus {
        cloud_account: None,
        byok_config: Some(config),
        has_saved_byok: true,
        active_provider: Some("byok".into()),
    })
}

#[tauri::command]
async fn restore_ai_configuration(state: State<'_, AppState>) -> Result<ProviderStatus, AppError> {
    if let Some(tokens) = load_cloud_tokens()? {
        let result = core_call(state.clone(), {
            let access_token = tokens.access_token.clone();
            let refresh_token = tokens.refresh_token.clone();
            move |core| core.connect_cloud_ai(access_token, refresh_token)
        })
        .await;
        if result.is_ok() {
            *state.byok_config.lock().map_err(|_| AppError::State {
                message: "BYOK state lock poisoned".into(),
            })? = None;
            state.update_preferences(|preferences| preferences.provider = Some("cloud".into()))?;
            return provider_status(&state);
        }
        let refreshed = core_call(state.clone(), {
            let refresh_token = tokens.refresh_token.clone();
            move |core| core.refresh_cloud_ai(refresh_token)
        })
        .await;
        if let Ok(refreshed) = refreshed {
            save_cloud_tokens(&refreshed)?;
            core_call(state.clone(), move |core| {
                core.connect_cloud_ai(refreshed.access_token, refreshed.refresh_token)
            })
            .await?;
            *state.byok_config.lock().map_err(|_| AppError::State {
                message: "BYOK state lock poisoned".into(),
            })? = None;
            state.update_preferences(|preferences| preferences.provider = Some("cloud".into()))?;
            return provider_status(&state);
        }
    }
    if let Some(value) = load_byok()? {
        let config = core_call(state.clone(), move |core| {
            core.connect_byok(value.api_key, value.base_url, value.model)
        })
        .await?;
        *state.byok_config.lock().map_err(|_| AppError::State {
            message: "BYOK state lock poisoned".into(),
        })? = Some(config.clone());
        state.update_preferences(|preferences| preferences.provider = Some("byok".into()))?;
        return Ok(ProviderStatus {
            cloud_account: None,
            byok_config: Some(config),
            has_saved_byok: true,
            active_provider: Some("byok".into()),
        });
    }
    provider_status(&state)
}

#[tauri::command]
async fn disconnect_ai(state: State<'_, AppState>) -> Result<ProviderStatus, AppError> {
    core_call(state.clone(), |core| core.disconnect_cloud_ai()).await?;
    core_call(state.clone(), |core| core.disconnect_byok()).await?;
    clear_cloud_tokens()?;
    clear_byok()?;
    *state.byok_config.lock().map_err(|_| AppError::State {
        message: "BYOK state lock poisoned".into(),
    })? = None;
    state.update_preferences(|preferences| preferences.provider = None)?;
    provider_status(&state)
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentStartInput {
    pub mode: AgentModeDto,
    pub goal: String,
    pub source_paths: Vec<String>,
    pub history: Vec<AgentMessageDto>,
}

#[tauri::command]
async fn list_agent_capabilities(
    state: State<'_, AppState>,
) -> Result<Vec<studypulse_ffi::CapabilityManifestDto>, AppError> {
    let core = state.core.clone();
    Ok(core.list_agent_capabilities())
}

#[tauri::command]
async fn start_agent(
    input: AgentStartInput,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    core_call(state, move |core| {
        core.start_agent_with_mode(input.mode, input.goal, input.source_paths, input.history)
    })
    .await
}

#[tauri::command]
async fn wait_agent_events(
    run_id: String,
    after_sequence: u64,
    timeout_ms: u32,
    state: State<'_, AppState>,
) -> Result<Vec<AgentEventDto>, AppError> {
    core_call(state, move |core| {
        core.wait_for_agent_events(run_id, after_sequence, timeout_ms)
    })
    .await
}

#[tauri::command]
async fn cancel_agent(run_id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.cancel_agent(run_id)).await
}

#[tauri::command]
async fn submit_confirmation(
    run_id: String,
    confirmation_id: String,
    decision: ConfirmationDecisionDto,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    core_call(state, move |core| {
        core.submit_confirmation(run_id, confirmation_id, decision)
    })
    .await
}

#[tauri::command]
async fn submit_agent_input(
    run_id: String,
    input_id: String,
    answer_json: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    core_call(state, move |core| {
        core.submit_agent_input(run_id, input_id, answer_json)
    })
    .await
}

#[tauri::command]
async fn get_run_state(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<RunStatusDto, AppError> {
    core_call(state, move |core| core.get_run_state(run_id)).await
}

#[tauri::command]
async fn get_agent_notebooks(
    state: State<'_, AppState>,
) -> Result<Vec<AgentNotebookDto>, AppError> {
    core_call(state, |core| core.get_agent_notebooks()).await
}

#[tauri::command]
async fn save_agent_notebooks(
    workspace_id: String,
    notebooks: Vec<AgentNotebookDto>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    core_call(state, move |core| {
        core.save_agent_notebooks(workspace_id, notebooks)
    })
    .await
}

macro_rules! read_command {
    ($name:ident, $method:ident, $type:ty) => {
        #[tauri::command]
        async fn $name(state: State<'_, AppState>) -> Result<$type, AppError> {
            core_call(state, |core| core.$method()).await
        }
    };
}

macro_rules! delete_command {
    ($name:ident, $method:ident) => {
        #[tauri::command]
        async fn $name(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
            core_call(state, move |core| core.$method(id)).await
        }
    };
}

read_command!(get_tasks, get_tasks, Vec<TaskDto>);
read_command!(get_subjects, get_subjects, Vec<SubjectDto>);
read_command!(get_phases, get_phases, Vec<StudyPhaseDto>);
read_command!(get_grades, get_grades, Vec<GradeDto>);
read_command!(get_mistakes, get_mistakes, Vec<MistakeNoteDto>);
read_command!(get_due_mistakes, get_due_mistakes, Vec<MistakeNoteDto>);
read_command!(get_diary_entries, get_diary_entries, Vec<DiaryEntryDto>);
read_command!(get_exams, get_exams, Vec<ExamDto>);
read_command!(get_study_sessions, get_study_sessions, Vec<StudySessionDto>);
read_command!(
    get_time_investment_subjects,
    get_time_investment_subjects,
    Vec<TimeInvestmentSubjectDto>
);
read_command!(get_today_snapshot, get_today_snapshot, TodaySnapshotDto);
read_command!(list_library_files, list_library_files, Vec<FileEntryDto>);

#[tauri::command]
async fn get_learning_trends(
    range_days: u32,
    state: State<'_, AppState>,
) -> Result<TrendsSnapshotDto, AppError> {
    core_call(state, move |core| core.get_learning_trends(range_days as i64)).await
}

#[tauri::command]
async fn active_timer(state: State<'_, AppState>) -> Result<TimerSnapshotDto, AppError> {
    let core = state.core.clone();
    Ok(core.active_timer())
}

#[tauri::command]
async fn upsert_task(task: TaskDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_task(task)).await
}

#[tauri::command]
async fn set_task_completed(
    id: String,
    completed: bool,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    core_call(state, move |core| core.set_task_completed(id, completed)).await
}

delete_command!(delete_task, delete_task);
delete_command!(delete_subject, delete_subject);
delete_command!(delete_phase, delete_phase);
delete_command!(delete_grade, delete_grade);
delete_command!(delete_mistake, delete_mistake);
delete_command!(delete_diary_entry, delete_diary_entry);
delete_command!(delete_exam, delete_exam);
delete_command!(delete_study_session, delete_study_session);
delete_command!(
    delete_time_investment_subject,
    delete_time_investment_subject
);

#[tauri::command]
async fn upsert_subject(value: SubjectDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_subject(value)).await
}

#[tauri::command]
async fn upsert_phase(value: StudyPhaseDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_phase(value)).await
}

#[tauri::command]
async fn upsert_grade(value: GradeDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_grade(value)).await
}

#[tauri::command]
async fn upsert_mistake(value: MistakeNoteDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_mistake(value)).await
}

#[tauri::command]
async fn upsert_diary_entry(
    value: DiaryEntryDto,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_diary_entry(value)).await
}

#[tauri::command]
async fn review_mistake(
    id: String,
    quality: i64,
    state: State<'_, AppState>,
) -> Result<studypulse_ffi::SrsReviewResultDto, AppError> {
    core_call(state, move |core| core.review_mistake(id, quality)).await
}

#[tauri::command]
async fn enroll_mistake(
    id: String,
    state: State<'_, AppState>,
) -> Result<ReviewStateDto, AppError> {
    core_call(state, move |core| core.enroll_mistake(id)).await
}

#[tauri::command]
async fn upsert_exam(value: ExamDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_exam(value)).await
}

#[tauri::command]
async fn upsert_time_investment_subject(
    value: TimeInvestmentSubjectDto,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    core_call(state, move |core| {
        core.upsert_time_investment_subject(value)
    })
    .await
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimerInput {
    pub intensity: SessionIntensityDto,
    pub target_duration_seconds: i64,
    pub investment_target: Option<studypulse_ffi::InvestmentTargetDto>,
}

#[tauri::command]
async fn start_timer(
    input: TimerInput,
    state: State<'_, AppState>,
) -> Result<TimerSnapshotDto, AppError> {
    core_call(state, move |core| {
        core.start_timer(
            input.intensity,
            input.target_duration_seconds,
            input.investment_target,
        )
    })
    .await
}

#[tauri::command]
async fn pause_timer(state: State<'_, AppState>) -> Result<TimerSnapshotDto, AppError> {
    core_call(state, |core| core.pause_timer()).await
}

#[tauri::command]
async fn resume_timer(state: State<'_, AppState>) -> Result<TimerSnapshotDto, AppError> {
    core_call(state, |core| core.resume_timer()).await
}

#[tauri::command]
async fn finish_timer(state: State<'_, AppState>) -> Result<StudySessionDto, AppError> {
    core_call(state, |core| core.finish_timer()).await
}

#[tauri::command]
async fn cancel_timer(state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, |core| core.cancel_timer()).await
}

#[tauri::command]
async fn search_library(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<SearchMatchDto>, AppError> {
    core_call(state, move |core| core.search_library(query)).await
}

#[tauri::command]
async fn import_library_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<FileEntryDto, AppError> {
    let path_value = PathBuf::from(&path);
    let metadata = fs::metadata(&path_value).map_err(|error| AppError::File {
        message: error.to_string(),
    })?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_BYTES {
        return Err(AppError::InvalidInput {
            message: "source files must be regular files no larger than 1 MiB".into(),
        });
    }
    let contents = fs::read(&path_value).map_err(|error| AppError::File {
        message: error.to_string(),
    })?;
    let file_name = path_value
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::InvalidInput {
            message: "source file name is invalid".into(),
        })?
        .to_owned();
    core_call(state, move |core| {
        core.import_library_source(file_name, contents)
    })
    .await
}

#[tauri::command]
async fn read_media(relative_path: String, state: State<'_, AppState>) -> Result<String, AppError> {
    let contents = core_call(state, move |core| core.read_media(relative_path)).await?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        contents,
    ))
}

#[tauri::command]
async fn export_backup(
    options: BackupExportOptionsDto,
    state: State<'_, AppState>,
) -> Result<BackupExportResultDto, AppError> {
    core_call(state, move |core| core.export_backup(options)).await
}

#[tauri::command]
async fn inspect_backup(
    path: String,
    state: State<'_, AppState>,
) -> Result<BackupInspectionDto, AppError> {
    core_call(state, move |core| core.inspect_backup(path)).await
}

#[tauri::command]
async fn apply_backup(
    inspection_id: String,
    mode: RestoreModeDto,
    resolutions: Vec<BackupResolutionDto>,
    state: State<'_, AppState>,
) -> Result<ImportReportDto, AppError> {
    core_call(state, move |core| {
        core.apply_backup(inspection_id, mode, resolutions)
    })
    .await
}

#[tauri::command]
async fn cancel_backup(inspection_id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.cancel_backup(inspection_id)).await
}

#[tauri::command]
async fn wait_operation_events(
    operation_id: String,
    after_sequence: u64,
    state: State<'_, AppState>,
) -> Result<Vec<OperationEventDto>, AppError> {
    let core = state.core.clone();
    Ok(core.wait_for_operation_events(operation_id, after_sequence, 0))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle()).map_err(|error| error.to_string())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_snapshot,
            create_workspace,
            open_workspace,
            close_workspace,
            cloud_ai_login_url,
            complete_cloud_ai_auth,
            save_byok_configuration,
            restore_ai_configuration,
            disconnect_ai,
            list_agent_capabilities,
            start_agent,
            wait_agent_events,
            cancel_agent,
            submit_confirmation,
            submit_agent_input,
            get_run_state,
            get_agent_notebooks,
            save_agent_notebooks,
            get_tasks,
            upsert_task,
            set_task_completed,
            delete_task,
            get_subjects,
            upsert_subject,
            delete_subject,
            get_phases,
            upsert_phase,
            delete_phase,
            get_grades,
            upsert_grade,
            delete_grade,
            get_mistakes,
            upsert_mistake,
            delete_mistake,
            delete_diary_entry,
            get_due_mistakes,
            get_diary_entries,
            get_learning_trends,
            review_mistake,
            enroll_mistake,
            upsert_diary_entry,
            get_exams,
            upsert_exam,
            delete_exam,
            get_study_sessions,
            delete_study_session,
            get_time_investment_subjects,
            upsert_time_investment_subject,
            delete_time_investment_subject,
            get_today_snapshot,
            active_timer,
            start_timer,
            pause_timer,
            resume_timer,
            finish_timer,
            cancel_timer,
            list_library_files,
            search_library,
            import_library_file,
            read_media,
            export_backup,
            inspect_backup,
            apply_backup,
            cancel_backup,
            wait_operation_events
        ])
        .run(tauri::generate_context!())
        .expect("error while running StudyPulse");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_error_serializes_without_secret_fields() {
        let error = AppError::Credentials {
            message: "keychain unavailable".into(),
        };
        let encoded = serde_json::to_string(&error).expect("error should serialize");
        assert!(!encoded.contains("api_key"));
        assert!(encoded.contains("keychain unavailable"));
    }

    #[test]
    fn preferences_round_trip() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("preferences.json");
        let preferences = Preferences {
            workspace_path: Some("/tmp/studypulse".into()),
            provider: Some("byok".into()),
        };
        fs::write(&path, serde_json::to_vec(&preferences).expect("encode")).expect("write");
        assert_eq!(
            read_preferences(&path).expect("read").provider,
            Some("byok".into())
        );
    }

    #[test]
    fn core_accepts_frontend_create_payloads() {
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace_path = directory.path().join("Workspace");
        let core = studypulse_ffi::StudyPulseCore::new();
        core.create_workspace(workspace_path.to_string_lossy().into_owned())
            .expect("workspace should be created");

        let now = "2026-08-01T10:00:00.000Z".to_string();
        core.upsert_task(studypulse_ffi::TaskDto {
            id: "11111111-1111-4111-8111-111111111111".into(),
            title: "Review algebra".into(),
            task_type: studypulse_ffi::TaskTypeDto::Homework,
            due_date: now.clone(),
            reminder_date: now.clone(),
            subject: "Mathematics".into(),
            importance: 3,
            notes: String::new(),
            is_completed: false,
            reminder_event_id: None,
            reminder_calendar_id: None,
            created_at: now.clone(),
            phase_id: None,
            coach_execution_data: None,
            coach_goal_id: None,
            coach_proposal_id: None,
            extra_json: "{}".into(),
        })
        .expect("task payload should be accepted");
        core.upsert_subject(studypulse_ffi::SubjectDto {
            id: "22222222-2222-4222-8222-222222222222".into(),
            name: "Mathematics".into(),
            enabled: true,
            full_score: 100.0,
            display_name: "Mathematics".into(),
            extra_json: "{}".into(),
        })
        .expect("subject payload should be accepted");
        core.upsert_exam(studypulse_ffi::ExamDto {
            id: "33333333-3333-4333-8333-333333333333".into(),
            name: "Algebra quiz".into(),
            exam_date: now.clone(),
            exam_end_date: None,
            importance: 3,
            subject: "Mathematics".into(),
            exam_name: "Algebra quiz".into(),
            mastery_degree: 0,
            time_slot: None,
            phase_id: None,
            checklist: Vec::new(),
            location_school: String::new(),
            location_classroom: String::new(),
            location_seat: String::new(),
            countdown_notify_days: Some(vec![7, 1]),
            exam_review: None,
            extra_json: "{}".into(),
        })
        .expect("exam payload should be accepted");
        core.upsert_time_investment_subject(studypulse_ffi::TimeInvestmentSubjectDto {
            id: "44444444-4444-4444-8444-444444444444".into(),
            name: "Mathematics".into(),
            symbol_name: "book.closed".into(),
            theme: studypulse_ffi::TimeInvestmentThemeDto::Ocean,
            start_date: now.clone(),
            sort_order: 0,
            created_at: now,
            is_archived: false,
            extra_json: "{}".into(),
        })
        .expect("time investment payload should be accepted");

        assert_eq!(core.get_tasks().expect("tasks should be readable").len(), 1);
        assert_eq!(
            core.get_subjects()
                .expect("subjects should be readable")
                .len(),
            1
        );
        assert_eq!(core.get_exams().expect("exams should be readable").len(), 1);
        assert_eq!(
            core.get_time_investment_subjects()
                .expect("investment subjects should be readable")
                .len(),
            1
        );
    }
}
