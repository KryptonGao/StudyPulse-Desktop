#![cfg_attr(windows, allow(linker_messages))]

// This module is the Tauri host boundary: it translates untrusted frontend
// commands into typed Core calls and turns backend failures into safe wire
// errors. Keeping that boundary explicit is more important than duplicating
// business rules that already live in the Rust Core.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use studypulse_ffi::{
    AgentEventDto, AgentMessageDto, AgentModeDto, AgentNotebookDto, AgentTurnDto, AiFeatureRequestDto, BackupExportOptionsDto,
    TurnResultDto,
    BackupExportResultDto, BackupInspectionDto, BackupResolutionDto, ByokConfigDto,
    CloudAccountDto, CloudAuthTokensDto, ComprehensiveExamDto, ConfirmationDecisionDto, CoreError,
    DiaryEntryDto, ExamDto, FileEntryDto, GradeDto, ImportReportDto, MistakeNoteDto, OperationEventDto,
    RestoreModeDto, ReviewStateDto, RunStatusDto, SearchMatchDto, SessionIntensityDto,
    StudyPhaseDto, StudySessionDto, SubjectDto, TaskDto, TimeInvestmentSubjectDto, TurnRequestDto,
    TimerSnapshotDto, TodaySnapshotDto, TrendsSnapshotDto, WorkspaceDto,
};
use tauri::{Manager, State};
use thiserror::Error;

const CLOUD_SERVICE: &str = "space.chenkai.StudyPulse-Desktop.CloudAI";
const CLOUD_ACCOUNT: &str = "session-token-pair";
const BYOK_SERVICE: &str = "space.chenkai.StudyPulse-Desktop.BYOK";
const BYOK_ACCOUNT: &str = "openai-compatible";
// The host repeats the Core's import limit before reading a user-selected
// file. This prevents an oversized path from becoming an oversized payload
// even when the caller is not the trusted React UI.
const MAX_SOURCE_BYTES: u64 = 1_048_576;

// Cloud tokens are serialized only as the value of this keyring entry; they
// are never placed in Preferences, the Workspace, or a frontend DTO.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredCloudTokens {
    access_token: String,
    refresh_token: String,
}

// BYOK retains the connection details needed to restore a provider after a
// restart. The API key follows the same system-credential-only rule as Cloud
// tokens, while ProviderStatus exposes only the non-secret configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredByok {
    api_key: String,
    base_url: String,
    model: String,
}

// Preferences are deliberately small application metadata. A Workspace path
// and the selected provider may be restored here, but credentials must not.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Preferences {
    workspace_path: Option<String>,
    provider: Option<String>,
}

// This is the redacted provider view sent to React. `has_saved_byok` conveys
// availability without serializing the saved API key itself.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub cloud_account: Option<CloudAccountDto>,
    pub byok_config: Option<ByokConfigDto>,
    pub has_saved_byok: bool,
    pub active_provider: Option<String>,
}

// The initial snapshot lets the UI restore its shell in one command while
// keeping all actual Workspace and credential access inside the host/Core.
#[derive(Debug, Clone, Serialize)]
pub struct AppSnapshot {
    pub workspace: Option<WorkspaceDto>,
    pub provider: ProviderStatus,
}

// AppError is the host's stable error envelope. The variants intentionally
// classify failures without adding secret-bearing fields to the serialized
// response that crosses the Tauri boundary.
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
    // CoreError already carries the detailed operation context; this adapter
    // preserves it under the host's `core` category for frontend handling.
    fn from(value: CoreError) -> Self {
        Self::Core {
            message: value.to_string(),
        }
    }
}

impl From<keyring::Error> for AppError {
    // Keyring failures remain credential-category errors, but only their
    // ordinary error text is returned; the secret value is never interpolated.
    fn from(value: keyring::Error) -> Self {
        Self::Credentials {
            message: value.to_string(),
        }
    }
}

// AppState owns the long-lived Core handle and the small amount of host state
// needed to restore preferences. BYOK configuration is an in-memory redacted
// view; the secret itself remains in the OS credential store.
#[derive(Clone)]
pub struct AppState {
    pub core: Arc<studypulse_ffi::StudyPulseCore>,
    preferences_path: PathBuf,
    preferences: Arc<Mutex<Preferences>>,
    byok_config: Arc<Mutex<Option<ByokConfigDto>>>,
}

impl AppState {
    // Startup creates the app-config directory, loads non-secret preferences,
    // and opportunistically reopens an existing Workspace. A stale path is
    // ignored so startup does not fail merely because a removable volume is
    // unavailable.
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
        if let Some(path) = state.preferences_snapshot()?.workspace_path
            && Path::new(&path).is_dir()
        {
            let _ = state.core.open_workspace(path);
        }
        Ok(state)
    }

    // Clone under the mutex so callers can perform a consistent read without
    // holding the lock while they call Core or serialize a response.
    fn preferences_snapshot(&self) -> Result<Preferences, AppError> {
        self.preferences
            .lock()
            .map(|value| value.clone())
            .map_err(|_| AppError::State {
                message: "preferences lock poisoned".into(),
            })
    }

    // Updates are applied to the in-memory snapshot first and then persisted
    // as the complete small JSON document. This keeps the selected Workspace
    // and provider synchronized with the host's process state.
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

// Missing preferences are a normal first-launch state; malformed or unreadable
// files remain errors so the host does not silently discard user settings.
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

// Entry construction is centralized so every credential operation uses the
// same service/account namespace and maps keyring errors consistently.
fn keyring_entry(service: &str, account: &str) -> Result<Entry, AppError> {
    Entry::new(service, account).map_err(AppError::from)
}

// Cloud authentication is loaded from the OS keychain only. `NoEntry` means
// the user has not configured Cloud AI and is therefore not an error.
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

// The DTO is copied into a private storage type before serialization so the
// keyring format stays independent from the frontend-facing FFI record.
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

// Disconnect is idempotent at the host boundary: deleting an absent keyring
// item has the same successful result as deleting an existing one.
fn clear_cloud_tokens() -> Result<(), AppError> {
    let entry = keyring_entry(CLOUD_SERVICE, CLOUD_ACCOUNT)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

// BYOK follows the same load policy as Cloud AI, but stores endpoint metadata
// together with the key so the connection can be restored after relaunch.
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

// Only this function serializes the private BYOK storage record. No caller
// receives the encoded value back through ProviderStatus or a command result.
fn save_byok(value: &StoredByok) -> Result<(), AppError> {
    let entry = keyring_entry(BYOK_SERVICE, BYOK_ACCOUNT)?;
    let encoded = serde_json::to_string(value).map_err(|error| AppError::Credentials {
        message: error.to_string(),
    })?;
    entry.set_password(&encoded).map_err(AppError::from)
}

// Clearing BYOK is also idempotent, which makes repeated disconnect requests
// safe without leaking whether a credential had existed.
fn clear_byok() -> Result<(), AppError> {
    let entry = keyring_entry(BYOK_SERVICE, BYOK_ACCOUNT)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

// ProviderStatus joins Core's cloud-account view with host preferences and a
// redacted BYOK view. The keyring is queried only for the boolean presence
// flag, never to populate a secret-bearing response.
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

// Tauri commands are async at the IPC boundary, but StudyPulseCore exposes
// synchronous methods. Every Core operation therefore moves to Tauri's
// blocking pool so file I/O and mutex waits cannot block the event loop.
// The closure owns an Arc clone and returns through one common error adapter,
// keeping command implementations small without weakening this boundary.
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
// This is the browser-preview-safe read path: it reports current host state
// without forcing the UI to issue separate provider and Workspace requests.
async fn app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, AppError> {
    let provider = provider_status(&state)?;
    Ok(AppSnapshot {
        workspace: state.core.current_workspace(),
        provider,
    })
}

#[tauri::command]
// Workspace creation is delegated to Core first; preferences are updated only
// after success so a failed create cannot advertise a non-existent Workspace.
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
// Opening follows the same commit-after-success ordering as creation. The
// frontend-provided path is still treated as untrusted by the Core layer.
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
// Closing only changes Core's active Workspace; the last selected path remains
// in preferences for the next explicit open/startup restore decision.
async fn close_workspace(state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, |core| core.close_workspace()).await
}

#[tauri::command]
// Login URL generation is synchronous Core work behind the same blocking-pool
// adapter; the host does not construct or rewrite authentication URLs itself.
async fn cloud_ai_login_url(state: State<'_, AppState>) -> Result<String, AppError> {
    core_call(state, |core| core.cloud_ai_login_url()).await
}

#[tauri::command]
// The deep-link lifecycle ends here: App.tsx forwards the callback URL, Core
// validates/parses it, and only then does the host connect Core and persist the
// returned tokens in the OS keychain. Raw callback material never becomes a
// ProviderStatus field or a Workspace preference.
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

// This input is intentionally separate from StoredByok so deserialization can
// accept only the frontend's public shape while persistence remains private.
#[derive(Debug, Clone, Deserialize)]
pub struct ByokInput {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

#[tauri::command]
// An empty API key means "keep the saved key" for an existing configuration;
// it never means "write an empty credential". All values are trimmed before
// Core validation, then the private record is saved only after connect_byok
// succeeds.
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
// Restoration prefers Cloud AI, attempts a refresh when the access token is
// stale, and falls back to saved BYOK. Each successful branch updates the
// provider preference only after Core has accepted the credentials.
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
// Disconnect clears both Core clients, both keyring entries, the in-memory
// redacted BYOK state, and the selected-provider preference as one host action.
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
// AgentStartInput carries the selected-source boundary into Core. The host
// passes source paths through unchanged; Core remains responsible for the
// authoritative path and permission checks before any tool executes.
pub struct AgentStartInput {
    pub mode: AgentModeDto,
    pub goal: String,
    pub source_paths: Vec<String>,
    pub history: Vec<AgentMessageDto>,
}

#[tauri::command]
// Capability discovery is read-only and does not need the blocking adapter:
// it returns static Core metadata without touching the Workspace.
async fn list_agent_capabilities(
    state: State<'_, AppState>,
) -> Result<Vec<studypulse_ffi::CapabilityManifestDto>, AppError> {
    let core = state.core.clone();
    Ok(core.list_agent_capabilities())
}

#[tauri::command]
// Starting an agent only queues the request in Core. Subsequent event polling,
// confirmation, input, and cancellation commands all use the same async-safe
// bridge so the UI never directly drives synchronous agent state.
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
async fn start_turn(
    request: TurnRequestDto,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    core_call(state, move |core| core.start_turn(request)).await
}

#[tauri::command]
async fn list_agent_turns(
    state: State<'_, AppState>,
) -> Result<Vec<AgentTurnDto>, AppError> {
    core_call(state, |core| core.list_agent_turns()).await
}

#[tauri::command]
async fn resume_agent_turn(
    turn_id: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    core_call(state, move |core| core.resume_agent_turn(turn_id)).await
}

#[tauri::command]
async fn get_agent_turn_result(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<TurnResultDto, AppError> {
    core_call(state, move |core| core.get_agent_turn_result(run_id)).await
}

#[tauri::command]
// Feature callers keep prompt construction, output validation, cache policy,
// and stale-result decisions in Core. The host only transports the structured
// request and returns the redacted result envelope.
async fn run_ai_feature(
    request: AiFeatureRequestDto,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    core_call(state, move |core| core.run_ai_feature_json(request)).await
}

#[tauri::command]
// Diagnostics contain timing/cache/outcome metadata only; Core never exposes
// prompts, raw responses, Workspace text, or credential-bearing request data.
async fn get_ai_diagnostics(state: State<'_, AppState>) -> Result<String, AppError> {
    let core = state.core.clone();
    Ok(core.get_ai_diagnostics_json())
}

#[tauri::command]
// `after_sequence` is a monotonic cursor, not an array offset. Keeping it in
// the command contract lets the frontend resume polling without losing events
// when multiple events arrive between requests.
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
// Cancellation is forwarded to Core, which wakes the model and any pending
// confirmation/input waiters; the host does not invent a second run state.
async fn cancel_agent(run_id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.cancel_agent(run_id)).await
}

#[tauri::command]
// A confirmation decision is a user authorization boundary. The host forwards
// the opaque run/confirmation identifiers and lets Core enforce their match.
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
// Agent answers remain JSON strings at this boundary because the active tool
// owns the schema. Core validates size and content before releasing the wait.
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
// Run-state reads are deliberately sourced from Core so terminal status and
// cancellation semantics stay consistent with the event timeline.
async fn get_run_state(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<RunStatusDto, AppError> {
    core_call(state, move |core| core.get_run_state(run_id)).await
}

#[tauri::command]
// Notebook reads/writes go through Core, which applies Workspace ownership and
// serialization rules rather than trusting a frontend-selected file location.
async fn get_agent_notebooks(
    state: State<'_, AppState>,
) -> Result<Vec<AgentNotebookDto>, AppError> {
    core_call(state, |core| core.get_agent_notebooks()).await
}

#[tauri::command]
// The workspace_id is part of the Core-side persistence contract; this host
// command does not use it to construct a path or bypass safe path handling.
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

// These macros keep repetitive CRUD commands on the same core_call path. A
// new command still has to be registered in generate_handler! below.
macro_rules! read_command {
    ($name:ident, $method:ident, $type:ty) => {
        #[tauri::command]
        async fn $name(state: State<'_, AppState>) -> Result<$type, AppError> {
            core_call(state, |core| core.$method()).await
        }
    };
}

// Deletion accepts only an opaque ID here; record lookup, validation, and
// atomic persistence remain Core responsibilities.
macro_rules! delete_command {
    ($name:ident, $method:ident) => {
        #[tauri::command]
        async fn $name(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
            core_call(state, move |core| core.$method(id)).await
        }
    };
}

// Read commands intentionally have no frontend-controlled filesystem access;
// Core reads the active Workspace and returns DTOs through this typed boundary.
read_command!(get_tasks, get_tasks, Vec<TaskDto>);
read_command!(get_subjects, get_subjects, Vec<SubjectDto>);
read_command!(get_phases, get_phases, Vec<StudyPhaseDto>);
read_command!(get_grades, get_grades, Vec<GradeDto>);
read_command!(get_mistakes, get_mistakes, Vec<MistakeNoteDto>);
read_command!(get_due_mistakes, get_due_mistakes, Vec<MistakeNoteDto>);
read_command!(get_diary_entries, get_diary_entries, Vec<DiaryEntryDto>);
// The due-mistake view is computed by Core at read time, so this host does not
// cache a potentially stale review queue between frontend requests.
read_command!(get_exams, get_exams, Vec<ExamDto>);
read_command!(get_comprehensive_exams, get_comprehensive_exams, Vec<ComprehensiveExamDto>);
read_command!(get_coach_data_json, get_coach_data_json, String);
read_command!(get_exam_goals_json, get_exam_goals_json, Vec<String>);
read_command!(get_exam_plans_json, get_exam_plans_json, Vec<String>);
read_command!(get_exam_simulations_json, get_exam_simulations_json, Vec<String>);
read_command!(get_study_sessions, get_study_sessions, Vec<StudySessionDto>);
// Investment and daily-snapshot reads are grouped here because both are
// derived Core views, not independent host-side caches.
read_command!(
    get_time_investment_subjects,
    get_time_investment_subjects,
    Vec<TimeInvestmentSubjectDto>
);
read_command!(get_today_snapshot, get_today_snapshot, TodaySnapshotDto);
read_command!(list_library_files, list_library_files, Vec<FileEntryDto>);

#[tauri::command]
// The range is passed as a numeric value to Core, where the analytics layer
// applies its supported clamp and computes the deterministic snapshot.
async fn get_learning_trends(
    range_days: u32,
    state: State<'_, AppState>,
) -> Result<TrendsSnapshotDto, AppError> {
    core_call(state, move |core| core.get_learning_trends(range_days as i64)).await
}

#[tauri::command]
// The active timer is process-local Core state. This read does not touch disk,
// so it can return directly without moving synchronous persistence work.
async fn active_timer(state: State<'_, AppState>) -> Result<TimerSnapshotDto, AppError> {
    let core = state.core.clone();
    Ok(core.active_timer())
}

#[tauri::command]
// Typed task payloads are accepted from the wire and validated by Core before
// they reach Workspace storage; the host only provides the async boundary.
async fn upsert_task(task: TaskDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_task(task)).await
}

#[tauri::command]
// Completion is a narrow mutation that keeps the record's other fields under
// Core control instead of allowing the frontend to rewrite the whole object.
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
delete_command!(delete_comprehensive_exam, delete_comprehensive_exam);
delete_command!(delete_coach_goal, delete_coach_goal);
delete_command!(delete_exam_goal, delete_exam_goal);
delete_command!(delete_exam_plan, delete_exam_plan);
delete_command!(delete_exam_simulation, delete_exam_simulation);
// Deletes intentionally have no separate host validation: Core owns UUID
// parsing, record existence, and the atomic write lock for every collection.
delete_command!(delete_study_session, delete_study_session);
delete_command!(
    delete_time_investment_subject,
    delete_time_investment_subject
);

#[tauri::command]
// The typed upserts below preserve the generated command names and the
// frontend DTO contract while sharing one asynchronous Core boundary.
async fn upsert_subject(value: SubjectDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_subject(value)).await
}

// Subject, phase, grade, mistake, and diary writes all use typed DTOs. Their
// validation and preservation of unknown fields remain in the Core model layer.
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
async fn apply_mistake_ai_patch(
    id: String,
    patch_json: String,
    state: State<'_, AppState>,
) -> Result<MistakeNoteDto, AppError> {
    core_call(state, move |core| core.apply_mistake_ai_patch(id, patch_json)).await
}

#[tauri::command]
async fn save_mistake_ai_session(
    id: String,
    kind: String,
    payload_json: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    core_call(state, move |core| core.save_mistake_ai_session(id, kind, payload_json)).await
}

#[tauri::command]
async fn upsert_diary_entry(
    value: DiaryEntryDto,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_diary_entry(value)).await
}

// SRS commands return Core DTOs so the frontend can update its queue from the
// authoritative result rather than reproducing scheduling calculations.
#[tauri::command]
// Review quality is intentionally forwarded as the shared 1/3/4/5 SRS value;
// the Core analytics implementation owns interval and ease-factor changes.
async fn review_mistake(
    id: String,
    quality: i64,
    state: State<'_, AppState>,
) -> Result<studypulse_ffi::SrsReviewResultDto, AppError> {
    core_call(state, move |core| core.review_mistake(id, quality)).await
}

#[tauri::command]
// Enrollment only places an existing mistake into the review queue. It does
// not accept a client-supplied next-review date or mutate SRS state in React.
async fn enroll_mistake(
    id: String,
    state: State<'_, AppState>,
) -> Result<ReviewStateDto, AppError> {
    core_call(state, move |core| core.enroll_mistake(id)).await
}

#[tauri::command]
// Coach and exam records remain JSON strings at this layer for compatibility
// with their existing schema. Core parses and validates them before storage.
async fn upsert_exam(value: ExamDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_exam(value)).await
}

#[tauri::command]
async fn upsert_comprehensive_exam(value: ComprehensiveExamDto, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_comprehensive_exam(value)).await
}

// The JSON commands below intentionally do not inspect user-visible strings;
// the Core methods provide the schema-specific error context.
// Feature JSON forwarding intentionally keeps all parsing and validation in
// Core, where compatibility and unknown-field handling are centralized.
#[tauri::command]
// JSON payload forwarding is deliberately boring: changing or normalizing
// these strings in the host could break unknown-field compatibility.
async fn upsert_coach_goal(value_json: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_coach_goal_json(value_json)).await
}

#[tauri::command]
// The host does not deserialize feature-specific Coach JSON, so newer Core
// schemas can pass through without a second host-side migration.
async fn upsert_coach_analysis(value_json: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_coach_analysis_json(value_json)).await
}

#[tauri::command]
async fn upsert_coach_proposal(value_json: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_coach_proposal_json(value_json)).await
}

#[tauri::command]
async fn upsert_coach_chat(value_json: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_coach_chat_json(value_json)).await
}

#[tauri::command]
async fn upsert_coach_message(value_json: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_coach_message_json(value_json)).await
}

#[tauri::command]
// Proposal resolution carries an expected version so Core can reject stale UI
// writes instead of allowing a late frontend response to overwrite newer data.
async fn resolve_coach_proposal(
    proposal_id: String,
    decision: String,
    expected_goal_version: i64,
    state: State<'_, AppState>,
) -> Result<Vec<String>, AppError> {
    core_call(state, move |core| core.resolve_coach_proposal(proposal_id, decision, expected_goal_version)).await
}

#[tauri::command]
// Exam planning/simulation JSON follows the same pass-through compatibility
// rule as Coach data; Core remains the single persistence authority.
async fn upsert_exam_goal(value_json: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_exam_goal_json(value_json)).await
}

#[tauri::command]
async fn upsert_exam_plan(value_json: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_exam_plan_json(value_json)).await
}

#[tauri::command]
async fn upsert_exam_simulation(value_json: String, state: State<'_, AppState>) -> Result<(), AppError> {
    core_call(state, move |core| core.upsert_exam_simulation_json(value_json)).await
}

// Simulation deletion and creation share the same Core-owned schema boundary;
// the host does not infer defaults from the subject string.
#[tauri::command]
// A new simulation is created by Core so IDs, defaults, and persistence remain
// consistent with the existing exam-domain implementation.
async fn new_exam_simulation(subject: String, state: State<'_, AppState>) -> Result<String, AppError> {
    core_call(state, move |core| core.new_exam_simulation_json(subject)).await
}

#[tauri::command]
// Reports are generated by Core first; file output is kept as a separate host
// capability because it crosses from Workspace data into a user-chosen path.
async fn get_learning_report(range_days: i64, state: State<'_, AppState>) -> Result<String, AppError> {
    core_call(state, move |core| core.get_learning_report_json(range_days)).await
}

// Report paths are resolved by the host because they are outside the normal
// Workspace write API. The extension check, canonical parent, and Workspace
// exclusion prevent a frontend path from silently targeting protected data.
fn report_destination(path: &str, extension: &str, state: &AppState) -> Result<PathBuf, AppError> {
    let candidate = PathBuf::from(path);
    if candidate.file_name().is_none() || candidate.extension().and_then(|value| value.to_str()).map(|value| value.eq_ignore_ascii_case(extension)) != Some(true) {
        return Err(AppError::InvalidInput { message: format!("report path must end with .{extension}") });
    }
    let parent = candidate.parent().ok_or_else(|| AppError::InvalidInput { message: "report path has no parent directory".into() })?;
    let parent = parent.canonicalize().map_err(|error| AppError::File { message: error.to_string() })?;
    let candidate = parent.join(candidate.file_name().expect("checked above"));
    if let Some(workspace) = state.core.current_workspace() {
        let root = Path::new(&workspace.root_path).canonicalize().map_err(|error| AppError::File { message: error.to_string() })?;
        if candidate.starts_with(root) {
            return Err(AppError::InvalidInput { message: "reports must be saved outside the Workspace data directory".into() });
        }
    }
    Ok(candidate)
}

#[tauri::command]
// Text reports use the same destination policy for every supported extension;
// the host writes only after the path has passed that policy.
async fn write_report_file(path: String, extension: String, contents: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let destination = report_destination(&path, &extension, &state)?;
    fs::write(destination, contents).map_err(|error| AppError::File { message: error.to_string() })
}

#[tauri::command]
// Image assets arrive as base64 over the command boundary. Decode and size
// checks happen before writing so malformed or oversized payloads fail early.
async fn write_report_asset(path: String, contents_base64: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let destination = report_destination(&path, "png", &state)?;
    let contents = BASE64.decode(contents_base64).map_err(|error| AppError::InvalidInput { message: error.to_string() })?;
    if contents.len() > 20 * 1024 * 1024 {
        return Err(AppError::InvalidInput { message: "report image is too large".into() });
    }
    fs::write(destination, contents).map_err(|error| AppError::File { message: error.to_string() })
}

// Report sharing is kept separate from report generation so opening an OS file
// cannot be triggered as an incidental side effect of data generation.
#[tauri::command]
// Sharing accepts an existing regular file but refuses files under the active
// Workspace, then delegates opening to the platform opener plugin.
async fn share_report(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let candidate = PathBuf::from(&path);
    if !candidate.is_file() {
        return Err(AppError::File { message: "report file does not exist".into() });
    }
    if let Some(workspace) = state.core.current_workspace() {
        let root = Path::new(&workspace.root_path).canonicalize().map_err(|error| AppError::File { message: error.to_string() })?;
        if candidate.canonicalize().map_err(|error| AppError::File { message: error.to_string() })?.starts_with(root) {
            return Err(AppError::InvalidInput { message: "cannot share a Workspace data file as a report".into() });
        }
    }
    tauri_plugin_opener::open_path(&candidate, None::<&str>).map_err(|error| AppError::File {
        message: error.to_string(),
    })?;
    Ok(())
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
// TimerInput is the narrow wire shape for process-local timer state. Core
// validates duration and investment references before starting the timer.
pub struct TimerInput {
    pub intensity: SessionIntensityDto,
    pub target_duration_seconds: i64,
    pub investment_target: Option<studypulse_ffi::InvestmentTargetDto>,
}

#[tauri::command]
// Start/pause/resume/finish/cancel all use Core's in-process timer state; the
// host does not emulate elapsed time or persist a second timer representation.
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
// The frontend supplies a source path, but the host repeats the regular-file
// and 1 MiB checks before reading bytes. Core then applies filename, UTF-8,
// NUL, duplicate-name, and Workspace path rules during import.
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
// Media bytes are read and path-checked by Core, then encoded for the JSON IPC
// response. The host does not expose a raw filesystem path to the frontend.
async fn read_media(relative_path: String, state: State<'_, AppState>) -> Result<String, AppError> {
    let contents = core_call(state, move |core| core.read_media(relative_path)).await?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        contents,
    ))
}

#[tauri::command]
async fn write_media(
    relative_path: String,
    data_base64: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    if data_base64.len() > 90 * 1024 * 1024 {
        return Err(AppError::InvalidInput {
            message: "media payload is too large".into(),
        });
    }
    let contents = BASE64.decode(data_base64).map_err(|_| AppError::InvalidInput {
        message: "media payload is not valid base64".into(),
    })?;
    core_call(state, move |core| core.write_media(relative_path, contents)).await
}

#[tauri::command]
// Backup export/inspection/application are Core-managed operation state
// machines. The host only transports options, inspection IDs, and events.
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

// Apply/cancel/wait use operation IDs issued by Core. The host never accepts a
// caller-provided filesystem destination for an already-staged import.
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
// Operation polling uses the same monotonic sequence convention as agent
// events, allowing the UI to resume without treating a vector length as a
// cursor.
async fn wait_operation_events(
    operation_id: String,
    after_sequence: u64,
    state: State<'_, AppState>,
) -> Result<Vec<OperationEventDto>, AppError> {
    let core = state.core.clone();
    Ok(core.wait_for_operation_events(operation_id, after_sequence, 0))
}

// The deep-link plugin is installed before setup so the app can receive the
// `studypulse://auth/callback` lifecycle event. App.tsx consumes that event and
// calls complete_cloud_ai_auth; this host function only owns registration and
// credential persistence after Core accepts the callback.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // AppState is managed once for the process; commands receive a typed
        // State handle and never construct independent Core instances.
        .setup(|app| {
            let state = AppState::new(app.handle()).map_err(|error| error.to_string())?;
            app.manage(state);
            Ok(())
        })
        // Every frontend-visible command must appear in this generated list.
        // Keeping the full inventory here makes a missing registration fail at
        // review time instead of becoming a runtime "command not found" error.
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
            start_turn,
            list_agent_turns,
            resume_agent_turn,
            get_agent_turn_result,
            run_ai_feature,
            get_ai_diagnostics,
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
            apply_mistake_ai_patch,
            save_mistake_ai_session,
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
            get_comprehensive_exams,
            upsert_comprehensive_exam,
            delete_comprehensive_exam,
            get_coach_data_json,
            upsert_coach_goal,
            upsert_coach_analysis,
            upsert_coach_proposal,
            upsert_coach_chat,
            upsert_coach_message,
            resolve_coach_proposal,
            delete_coach_goal,
            get_exam_goals_json,
            upsert_exam_goal,
            delete_exam_goal,
            get_exam_plans_json,
            upsert_exam_plan,
            delete_exam_plan,
            get_exam_simulations_json,
            new_exam_simulation,
            upsert_exam_simulation,
            delete_exam_simulation,
            get_learning_report,
            write_report_file,
            write_report_asset,
            share_report,
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
            write_media,
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
// Host tests focus on wire safety, preference persistence, and a small Core
// integration contract instead of duplicating the full Core test suite.
// Keeping these checks local protects the IPC-facing behavior this module owns.
mod tests {
    use super::*;

    #[test]
    // This regression guard checks the serialized host envelope rather than
    // relying on a visual inspection of the error enum's private fields.
    fn app_error_serializes_without_secret_fields() {
        let error = AppError::Credentials {
            message: "keychain unavailable".into(),
        };
        let encoded = serde_json::to_string(&error).expect("error should serialize");
        assert!(!encoded.contains("api_key"));
        assert!(encoded.contains("keychain unavailable"));
    }

    #[test]
    // Preferences are non-secret startup metadata. The round trip verifies
    // that the file format used by AppState remains readable after a restart.
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
    // This smoke test exercises the actual FFI/Core boundary with frontend-like
    // DTOs, ensuring host forwarding still accepts the established payload
    // shape while annotation-only changes remain behavior neutral.
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
