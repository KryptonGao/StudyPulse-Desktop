use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration as StdDuration,
};

use chrono::{DateTime, SecondsFormat, Utc};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use studypulse_model_client::{
    ChatMessage, ModelClient, ModelImageAttachment, ModelRequest, ModelTextDeltaHandler,
    ModelToolCall, ModelToolDefinition, ModelUsage,
};
use studypulse_tools::{PermissionLevel, ToolRegistry};
use studypulse_workspace::{AgentTurn, Workspace};
use thiserror::Error;
use uuid::Uuid;

// The Agent runtime is deliberately kept above the model client and tool
// registry: it owns the run lifecycle, while providers only return text and
// proposed calls, and tools only execute after the host has validated them.
// This separation lets every front end observe the same event protocol and
// keeps confirmation, cancellation, and Workspace writes in one place.

pub const MAX_AGENT_LOOPS: usize = 8;
const MAX_PARALLEL_READ_TOOLS: usize = 4;

#[derive(Debug, Clone, Copy)]
struct ToolPolicy {
    allow_tools: bool,
    read_only: bool,
}

#[derive(Debug, Default)]
struct TurnMetadata {
    notebook_id: Option<String>,
    config_json: Option<String>,
}

#[derive(Debug)]
struct RunOptions {
    metadata: TurnMetadata,
    tool_policy: ToolPolicy,
}

// Modes describe user-visible workflows rather than separate runtimes.  The
// manifest below supplies stage labels and a loop budget, while the runtime
// still uses one common state machine for all modes.  Keeping this mapping in
// the core prevents UI clients from inventing incompatible lifecycle rules.
/// User-selectable Agent experiences.  The enum is intentionally kept in the
/// platform-neutral core so Swift and future clients share the same contract.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityManifest {
    pub mode: AgentMode,
    pub title: String,
    pub description: String,
    pub stages: Vec<String>,
    pub max_loops: u32,
    pub tools_used: Vec<String>,
    pub result_kind: String,
    pub request_schema_json: String,
    pub config_defaults_json: String,
}

/// A capability owns the user-facing contract while AgentRuntime owns the
/// lifecycle and permission state machine. The first-party registry is
/// declarative today; specialized pipelines can implement this trait without
/// changing the FFI or event protocol.
pub trait Capability: Send + Sync {
    fn manifest(&self) -> CapabilityManifest;
    fn validate_result(&self, text: &str) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct ManifestCapability {
    manifest: CapabilityManifest,
}

impl ManifestCapability {
    pub fn new(manifest: CapabilityManifest) -> Self {
        Self { manifest }
    }
}

impl Capability for ManifestCapability {
    fn manifest(&self) -> CapabilityManifest {
        self.manifest.clone()
    }

    fn validate_result(&self, text: &str) -> Result<(), String> {
        validate_capability_result(self.manifest.mode, text)
    }
}

#[derive(Default)]
pub struct CapabilityRegistry {
    capabilities: Vec<Arc<dyn Capability>>,
}

impl CapabilityRegistry {
    pub fn built_in() -> Self {
        Self {
            capabilities: capability_manifests()
                .into_iter()
                .map(|manifest| Arc::new(ManifestCapability::new(manifest)) as Arc<dyn Capability>)
                .collect(),
        }
    }

    pub fn manifests(&self) -> Vec<CapabilityManifest> {
        self.capabilities
            .iter()
            .map(|capability| capability.manifest())
            .collect()
    }

    pub fn manifest(&self, mode: AgentMode) -> Option<CapabilityManifest> {
        self.manifests()
            .into_iter()
            .find(|manifest| manifest.mode == mode)
    }

    pub fn validate_result(&self, mode: AgentMode, text: &str) -> Result<(), String> {
        self.capabilities
            .iter()
            .find(|capability| capability.manifest().mode == mode)
            .map_or(Ok(()), |capability| capability.validate_result(text))
    }
}

// A manifest is serialized for the UI and also drives the model request.  The
// stages are descriptive progress markers, not permission boundaries; tool
// access remains host-controlled and is checked when a call is executed.
pub fn capability_manifests() -> Vec<CapabilityManifest> {
    vec![
        manifest(
            AgentMode::Chat,
            "Chat",
            "Source-grounded learning conversation.",
            &["exploring", "responding"],
            8,
        ),
        manifest(
            AgentMode::DeepSolve,
            "Deep Solve",
            "Plan, solve, verify, and explain a problem.",
            &["planning", "solving", "verifying", "writing"],
            12,
        ),
        manifest(
            AgentMode::Mastery,
            "Mastery",
            "Adaptive teaching, quizzing, grading, and review.",
            &["diagnosing", "teaching", "quizzing", "reviewing"],
            10,
        ),
        manifest(
            AgentMode::DeepResearch,
            "Deep Research",
            "Decompose a question and produce a cited report.",
            &["rephrasing", "decomposing", "researching", "reporting"],
            16,
        ),
        manifest(
            AgentMode::QuestionLab,
            "Question Lab",
            "Generate a sourced question set and answer key.",
            &["ideating", "blueprinting", "generating", "checking"],
            10,
        ),
        manifest(
            AgentMode::Visualize,
            "Visualize",
            "Create and validate a teaching visualization.",
            &["analyzing", "generating", "reviewing"],
            5,
        ),
        manifest(
            AgentMode::Coach,
            "AI Coach",
            "Analyze a goal and propose reviewable study tasks as strict JSON.",
            &["diagnosing", "forecasting", "proposing", "checking"],
            8,
        ),
        manifest(
            AgentMode::ExamSimulation,
            "Exam Simulation",
            "Generate or grade a timed exam session as strict JSON.",
            &["blueprinting", "generating", "grading", "analyzing"],
            8,
        ),
        manifest(
            AgentMode::ReversePlanner,
            "Reverse Planner",
            "Turn an exam target into a weak-point and daily route as strict JSON.",
            &["contextualizing", "prioritizing", "routing", "checking"],
            8,
        ),
    ]
}

// Constructing manifests through one helper keeps their wire shape uniform and
// makes the public list easy to extend without duplicating conversion code.
fn manifest(
    mode: AgentMode,
    title: &str,
    description: &str,
    stages: &[&str],
    max_loops: u32,
) -> CapabilityManifest {
    let tools_used = match mode {
        AgentMode::DeepResearch => vec![
            "list_workspace_files",
            "search_workspace",
            "read_source",
            "read_memory",
            "web_search",
            "paper_search",
            "save_artifact",
            "ask_user",
        ],
        AgentMode::QuestionLab => vec![
            "list_workspace_files",
            "search_workspace",
            "read_source",
            "web_search",
            "paper_search",
            "save_artifact",
            "ask_user",
        ],
        AgentMode::Visualize => vec![
            "list_workspace_files",
            "search_workspace",
            "read_source",
            "save_artifact",
            "code_execution",
            "ask_user",
        ],
        _ => vec![
            "list_workspace_files",
            "search_workspace",
            "read_source",
            "read_memory",
            "write_memory",
            "web_search",
            "paper_search",
            "code_execution",
            "save_artifact",
            "ask_user",
            "get_tasks",
            "create_task",
        ],
    };
    let result_kind = match mode {
        AgentMode::DeepResearch => "research_report",
        AgentMode::QuestionLab => "question_set",
        AgentMode::Visualize => "visualization",
        AgentMode::Coach | AgentMode::ExamSimulation | AgentMode::ReversePlanner => {
            "structured_feature"
        }
        _ => "markdown",
    };
    CapabilityManifest {
        mode,
        title: title.into(),
        description: description.into(),
        stages: stages.iter().map(|stage| (*stage).into()).collect(),
        max_loops,
        tools_used: tools_used.into_iter().map(str::to_owned).collect(),
        result_kind: result_kind.into(),
        request_schema_json: r#"{"type":"object","properties":{"goal":{"type":"string"}}}"#.into(),
        config_defaults_json: r#"{"maxToolCalls":32,"maxOutputBytes":262144}"#.into(),
    }
}

// `Started` and `Cancelling` are transitional states.  Only the three states
// recognized here release the single-active-run slot and wake event consumers
// as terminal outcomes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Started,
    Running,
    WaitingForConfirmation,
    Cancelling,
    Failed,
    Cancelled,
    Completed,
}

impl RunStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::Completed)
    }
}

// Each event kind has a stable serialized spelling because the desktop UI
// persists a cursor and may observe events produced by a different process
// build.  Adding a kind is therefore a protocol change, not merely logging.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventKind {
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

// `ToolRequested` is the preview/authorization point, `ToolCompleted` is the
// structured model-facing result, and `ArtifactCreated` is an additional UI
// signal for successful archive writes.  These events are separate so clients
// can render consent without guessing from payload JSON.
//
// Event payload fields are optional because one timeline carries status,
// streamed text, tool previews, stage progress, and terminal errors.  The
// `sequence` is the durable cursor: consumers request events strictly after
// it, so a cursor must never be replaced with the current vector length.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub run_id: String,
    pub sequence: u64,
    pub timestamp: String,
    pub kind: AgentEventKind,
    pub status: Option<RunStatus>,
    pub text: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub permission: Option<PermissionLevel>,
    pub preview: Option<String>,
    pub confirmation_id: Option<String>,
    pub payload_json: Option<String>,
    pub mode: Option<AgentMode>,
    pub stage: Option<String>,
    pub progress: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub source_type: String,
    pub locator: String,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub path: String,
    pub extension: String,
    pub render_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub model_calls: u32,
    pub estimated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub schema_version: u32,
    pub mode: AgentMode,
    pub result_kind: String,
    pub text: String,
    pub output_json: Option<String>,
    pub sources: Vec<SourceRef>,
    pub artifacts: Vec<ArtifactRef>,
    pub usage: UsageSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnRequest {
    pub mode: AgentMode,
    pub goal: String,
    #[serde(default)]
    pub source_paths: Vec<String>,
    #[serde(default)]
    pub history: Vec<ConversationMessage>,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub config_json: Option<String>,
}

pub const TURN_RESULT_SCHEMA_VERSION: u32 = 1;

// Confirmation is intentionally a small explicit decision type.  A missing
// decision is different from denial: the former means the run is still
// waiting, while the latter is returned to the model as a structured result.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConfirmationDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConversationRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub content: String,
}

// History accepts only user/assistant messages at this boundary.  Tool messages
// are generated by the runtime after a prepared invocation completes, preventing
// a caller from injecting a fabricated tool result into the first request.
//
// A clock is injected so event timestamps and default dates can be deterministic
// in tests without changing the production event or Workspace formats.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

// Runtime errors intentionally do not expose provider internals or secrets.
// The FFI layer folds these variants into its single transportable error type,
// while callers can still distinguish busy runs from invalid submissions.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("another Agent run is already active")]
    Busy,
    #[error("Agent run was not found")]
    RunNotFound,
    #[error("confirmation request was not found")]
    ConfirmationNotFound,
    #[error("Agent input request was not found")]
    InputNotFound,
    #[error("Agent runtime failed: {0}")]
    Runtime(String),
}

// These small records are process-local coordination state.  They are not
// serialized into Workspace: event history is persisted separately, while a
// pending confirmation/input is meaningful only to the current run process.
#[derive(Default)]
struct EventBuffer {
    events: Vec<AgentEvent>,
}

struct ConfirmationState {
    id: String,
    decision: Option<ConfirmationDecision>,
}

struct InputState {
    id: String,
    answer: Option<String>,
}

// `RunControl` combines the state machines that can block the Agent.  The
// atomic cancellation flag is observed by model callbacks; Condvars wake
// synchronous confirmation/input waiters; Notify interrupts async model I/O.
struct RunControl {
    run_id: String,
    status: Mutex<RunStatus>,
    events: Mutex<EventBuffer>,
    events_changed: Condvar,
    next_sequence: AtomicU64,
    cancelled: AtomicBool,
    cancellation_changed: tokio::sync::Notify,
    confirmation: Mutex<Option<ConfirmationState>>,
    confirmation_changed: Condvar,
    input: Mutex<Option<InputState>>,
    input_changed: Condvar,
    turn: Mutex<AgentTurn>,
    usage: Mutex<UsageSummary>,
    sources: Mutex<Vec<SourceRef>>,
    artifacts: Mutex<Vec<ArtifactRef>>,
    final_text: Mutex<String>,
}

// Sequence numbers start at one so zero is an unambiguous initial cursor for a
// newly opened UI.  Every event is assigned by this control object before it
// is made visible to either the in-memory buffer or the persisted JSONL log.
impl RunControl {
    fn new(run_id: String, turn: AgentTurn) -> Self {
        Self {
            run_id,
            status: Mutex::new(RunStatus::Started),
            events: Mutex::new(EventBuffer::default()),
            events_changed: Condvar::new(),
            next_sequence: AtomicU64::new(1),
            cancelled: AtomicBool::new(false),
            cancellation_changed: tokio::sync::Notify::new(),
            confirmation: Mutex::new(None),
            confirmation_changed: Condvar::new(),
            input: Mutex::new(None),
            input_changed: Condvar::new(),
            turn: Mutex::new(turn),
            usage: Mutex::new(UsageSummary::default()),
            sources: Mutex::new(Vec::new()),
            artifacts: Mutex::new(Vec::new()),
            final_text: Mutex::new(String::new()),
        }
    }
}

// `RuntimeState` enforces the product rule that at most one Agent run is active
// while retaining completed controls long enough for clients to drain events.
struct RuntimeState {
    active_run: Option<String>,
    runs: HashMap<String, Arc<RunControl>>,
}

// The run map provides stable lookup for polling and submission commands.  It is
// intentionally not a persistence index; persisted JSONL is for replay/audit,
// while live controls contain wait handles and atomics that cannot be restored.
//
// AgentRuntime is the orchestration boundary: Workspace supplies local data,
// ToolRegistry supplies validated capabilities, ModelClient supplies model
// responses, and the clock supplies timestamps.  None of those collaborators
// owns the run lifecycle or user-consent decisions.
pub struct AgentRuntime {
    workspace: Workspace,
    tools: ToolRegistry,
    model: Arc<dyn ModelClient>,
    clock: Arc<dyn Clock>,
    capabilities: CapabilityRegistry,
    state: Mutex<RuntimeState>,
}

impl AgentRuntime {
    // Production construction uses the real clock; tests use `with_clock` to
    // make timestamps stable without adding a second runtime implementation.
    pub fn new(workspace: Workspace, model: Arc<dyn ModelClient>) -> Arc<Self> {
        Self::with_clock(workspace, model, Arc::new(SystemClock))
    }

    fn recover_persisted_turns(&self) {
        // A process cannot safely restore an in-flight provider future or a
        // pending write confirmation. Convert only checkpoints that were
        // explicitly marked safe into user-visible recoverable turns; unsafe
        // boundaries remain interrupted and require a fresh request.
        let Ok(turns) = self.workspace.read_agent_turns() else {
            return;
        };
        for mut turn in turns {
            if matches!(
                turn.status.as_str(),
                "completed" | "failed" | "cancelled" | "resumed"
            ) {
                continue;
            }
            turn.status = if turn.resume_safe {
                "recoverable".into()
            } else {
                "interrupted".into()
            };
            turn.checkpoint = "recovery".into();
            turn.error = Some("The previous process stopped before this turn finished.".into());
            turn.updated_at = self
                .clock
                .now()
                .to_rfc3339_opts(SecondsFormat::Millis, true);
            if let Err(error) = self.workspace.write_agent_turn(&turn) {
                tracing::warn!(
                    turn_id = %turn.id,
                    error = %error,
                    "failed to persist recovery status for Agent turn",
                );
            }
        }
    }

    pub fn with_clock(
        workspace: Workspace,
        model: Arc<dyn ModelClient>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        // The registry and state are created once per runtime so a run cannot
        // accidentally use a different tool catalog halfway through a loop.
        let runtime = Arc::new(Self {
            workspace,
            tools: ToolRegistry::default(),
            model,
            clock,
            capabilities: CapabilityRegistry::built_in(),
            state: Mutex::new(RuntimeState {
                active_run: None,
                runs: HashMap::new(),
            }),
        });
        runtime.recover_persisted_turns();
        runtime
    }

    pub fn start_agent(
        self: &Arc<Self>,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<ConversationMessage>,
    ) -> Result<String, AgentError> {
        // Chat is the compatibility entry point; richer modes opt into the
        // same lifecycle through `start_agent_with_mode`.
        self.start_agent_with_mode(AgentMode::Chat, goal, source_paths, history)
    }

    pub fn start_agent_with_mode(
        self: &Arc<Self>,
        mode: AgentMode,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<ConversationMessage>,
    ) -> Result<String, AgentError> {
        self.start_agent_with_mode_and_policy(
            mode,
            goal,
            source_paths,
            history,
            Vec::new(),
            RunOptions {
                metadata: TurnMetadata::default(),
                tool_policy: ToolPolicy {
                    allow_tools: true,
                    read_only: false,
                },
            },
        )
    }

    /// Start a feature caller with an explicit source boundary. Empty source
    /// selection means no Workspace tools at all; non-empty selection exposes
    /// only read tools scoped by the normal Workspace path checks.
    pub fn start_feature_with_mode(
        self: &Arc<Self>,
        mode: AgentMode,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<ConversationMessage>,
        attachments: Vec<ModelImageAttachment>,
    ) -> Result<String, AgentError> {
        let allow_tools = !source_paths.is_empty();
        self.start_agent_with_mode_and_policy(
            mode,
            goal,
            source_paths,
            history,
            attachments,
            RunOptions {
                metadata: TurnMetadata::default(),
                tool_policy: ToolPolicy {
                    allow_tools,
                    read_only: true,
                },
            },
        )
    }

    pub fn start_turn(self: &Arc<Self>, request: TurnRequest) -> Result<String, AgentError> {
        let TurnRequest {
            mode,
            goal,
            source_paths,
            history,
            notebook_id,
            config_json,
        } = request;
        if config_json
            .as_ref()
            .is_some_and(|value| value.len() > 64 * 1024)
        {
            return Err(AgentError::Runtime("turn config exceeds 64 KiB".into()));
        }
        self.start_agent_with_mode_and_policy(
            mode,
            goal,
            source_paths,
            history,
            Vec::new(),
            RunOptions {
                metadata: TurnMetadata {
                    notebook_id,
                    config_json,
                },
                tool_policy: ToolPolicy {
                    allow_tools: true,
                    read_only: false,
                },
            },
        )
    }

    pub fn list_turns(&self) -> Result<Vec<AgentTurn>, AgentError> {
        self.workspace
            .read_agent_turns()
            .map_err(|error| AgentError::Runtime(error.to_string()))
    }

    pub fn resume_turn(self: &Arc<Self>, turn_id: &str) -> Result<String, AgentError> {
        let turn = self
            .workspace
            .read_agent_turn(turn_id)
            .map_err(|error| AgentError::Runtime(error.to_string()))?;
        if matches!(turn.status.as_str(), "completed" | "cancelled" | "resumed") {
            return Err(AgentError::Runtime(
                "only an unfinished recoverable Agent turn can be resumed".into(),
            ));
        }
        if !turn.resume_safe {
            return Err(AgentError::Runtime(
                "this Agent turn stopped during an unsafe tool boundary and cannot be resumed automatically".into(),
            ));
        }
        let mode = serde_json::from_str::<AgentMode>(&format!("\"{}\"", turn.mode))
            .map_err(|_| AgentError::Runtime("stored Agent mode is invalid".into()))?;
        let messages =
            serde_json::from_str::<Vec<ChatMessage>>(&turn.history_json).map_err(|error| {
                AgentError::Runtime(format!("stored Agent transcript is invalid: {error}"))
            })?;
        if messages.is_empty() {
            return Err(AgentError::Runtime(
                "stored Agent transcript is empty".into(),
            ));
        }
        let notebook_id = turn.notebook_id.clone();
        let config_json = turn.config_json.clone();
        let run_id = self.start_agent_from_messages(
            mode,
            turn.goal.clone(),
            turn.source_paths.clone(),
            messages,
            Vec::new(),
            RunOptions {
                metadata: TurnMetadata {
                    notebook_id,
                    config_json,
                },
                tool_policy: ToolPolicy {
                    allow_tools: turn.allow_tools,
                    read_only: turn.read_only,
                },
            },
        )?;

        // The resumed run has its own durable checkpoint. Marking the source
        // checkpoint as consumed prevents it from being offered repeatedly
        // after the new run completes, while cloning the original turn keeps
        // its Notebook identity intact for audit/recovery.
        let mut resumed_turn = turn;
        resumed_turn.status = "resumed".into();
        resumed_turn.checkpoint = "resumed".into();
        resumed_turn.resume_safe = false;
        resumed_turn.updated_at = self
            .clock
            .now()
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        if let Err(error) = self.workspace.write_agent_turn(&resumed_turn) {
            // The resumed run is already in flight, so this failure cannot be
            // propagated; log it because an unconsumed source checkpoint may
            // let the UI offer the same turn for recovery again.
            tracing::warn!(
                turn_id = %resumed_turn.id,
                error = %error,
                "failed to mark source Agent turn as resumed",
            );
        }
        Ok(run_id)
    }

    /// Return a live result when the run is in this process, otherwise replay
    /// the durable Result event. This keeps completed turns inspectable after
    /// an application restart without restoring non-serializable wait state.
    pub fn turn_result_for_run(&self, run_id: &str) -> Result<TurnResult, AgentError> {
        if let Ok(control) = self.control(run_id) {
            return Ok(self.turn_result(&control));
        }
        let turn = self
            .workspace
            .read_agent_turn(run_id)
            .map_err(|_| AgentError::RunNotFound)?;
        if let Ok(lines) = self.workspace.read_agent_run_log(run_id) {
            for line in lines.iter().rev() {
                if let Ok(event) = serde_json::from_str::<AgentEvent>(line)
                    && event.kind == AgentEventKind::Result
                    && let Some(payload) = event.payload_json
                    && let Ok(result) = serde_json::from_str::<TurnResult>(&payload)
                {
                    return Ok(result);
                }
            }
        }
        let mode = serde_json::from_str::<AgentMode>(&format!("\"{}\"", turn.mode))
            .unwrap_or(AgentMode::Chat);
        let result_kind = self
            .capabilities
            .manifest(mode)
            .map(|manifest| manifest.result_kind)
            .unwrap_or_else(|| "markdown".into());
        Ok(TurnResult {
            schema_version: TURN_RESULT_SCHEMA_VERSION,
            mode,
            result_kind,
            text: turn.result_json.clone().unwrap_or_default(),
            output_json: turn.result_json,
            sources: Vec::new(),
            artifacts: Vec::new(),
            usage: UsageSummary::default(),
        })
    }

    fn start_agent_with_mode_and_policy(
        self: &Arc<Self>,
        mode: AgentMode,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<ConversationMessage>,
        attachments: Vec<ModelImageAttachment>,
        options: RunOptions,
    ) -> Result<String, AgentError> {
        let RunOptions {
            metadata,
            tool_policy,
        } = options;
        // Input validation occurs before the run id is reserved.  This is
        // important for callers that retry a rejected request: an invalid goal
        // must not consume the single active-run slot or leave a phantom run
        // in the lookup map.
        // Validate user text and resolve selected sources before reserving the
        // active slot.  This prevents an invalid request from leaving a busy
        // runtime or from handing unvalidated paths to a model/tool.
        if goal.trim().is_empty() {
            return Err(AgentError::Runtime("goal must not be empty".into()));
        }
        if history
            .iter()
            .any(|message| message.content.trim().is_empty())
        {
            return Err(AgentError::Runtime(
                "conversation history must not contain empty messages".into(),
            ));
        }
        let source_paths = if tool_policy.allow_tools {
            self.workspace
                .list_selected_library_files(&source_paths)
                .map_err(|error| AgentError::Runtime(error.to_string()))?
                .into_iter()
                .map(|entry| entry.relative_path)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        // Source resolution returns selected relative paths from Workspace.  The
        // Agent carries this list into every read invocation instead of trusting
        // a path proposed by the model during a later loop.
        let mut initial_messages = history
            .into_iter()
            .map(|message| match message.role {
                ConversationRole::User => ChatMessage::User {
                    content: message.content,
                },
                ConversationRole::Assistant => ChatMessage::Assistant {
                    content: message.content,
                },
            })
            .collect::<Vec<_>>();
        initial_messages.push(ChatMessage::User {
            content: goal.clone(),
        });
        let run_id = Uuid::new_v4().to_string();
        let now = self
            .clock
            .now()
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let turn = AgentTurn {
            id: run_id.clone(),
            mode: serde_json::to_string(&mode)
                .unwrap_or_else(|_| "\"chat\"".into())
                .trim_matches('"')
                .to_owned(),
            notebook_id: metadata.notebook_id,
            config_json: metadata.config_json,
            goal: goal.clone(),
            source_paths: source_paths.clone(),
            allow_tools: tool_policy.allow_tools,
            read_only: tool_policy.read_only,
            history_json: serde_json::to_string(&initial_messages).unwrap_or_else(|_| "[]".into()),
            status: "started".into(),
            stage: None,
            loop_index: 0,
            last_sequence: 0,
            result_json: None,
            error: None,
            checkpoint: "safe".into(),
            resume_safe: true,
            created_at: now.clone(),
            updated_at: now,
        };
        let control = Arc::new(RunControl::new(run_id.clone(), turn));
        {
            // The lock makes the check-and-reserve operation atomic across UI
            // calls racing to start two runs at the same time.
            let mut state = self.state.lock();
            if state.active_run.is_some() {
                return Err(AgentError::Busy);
            }
            state.active_run = Some(run_id.clone());
            state.runs.insert(run_id.clone(), Arc::clone(&control));
        }
        if let Err(error) = self.persist_turn(&control) {
            // Roll back the reserved slot so a failed start does not wedge the
            // single-active-run guard behind a run that never began.
            let mut state = self.state.lock();
            if state.active_run.as_deref() == Some(&run_id) {
                state.active_run = None;
            }
            state.runs.remove(&run_id);
            return Err(AgentError::Runtime(error));
        }
        let runtime = Arc::clone(self);
        // Keep the synchronous public API responsive while the current-thread
        // Tokio executor owns async model I/O and can select cancellation first.
        thread::Builder::new()
            .name(format!("studypulse-agent-{run_id}"))
            .spawn(move || {
                let executor = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match executor {
                    Ok(executor) => executor.block_on(runtime.run_agent(
                        control,
                        mode,
                        goal,
                        source_paths,
                        initial_messages,
                        attachments,
                        tool_policy,
                    )),
                    Err(error) => {
                        runtime.finish_with_error(&control, error.to_string());
                    }
                }
            })
            .map_err(|error| AgentError::Runtime(error.to_string()))?;
        Ok(run_id)
    }

    fn start_agent_from_messages(
        self: &Arc<Self>,
        mode: AgentMode,
        goal: String,
        requested_source_paths: Vec<String>,
        messages: Vec<ChatMessage>,
        attachments: Vec<ModelImageAttachment>,
        options: RunOptions,
    ) -> Result<String, AgentError> {
        let RunOptions {
            metadata,
            tool_policy,
        } = options;
        if goal.trim().is_empty() || messages.is_empty() {
            return Err(AgentError::Runtime(
                "resumed Agent turn is incomplete".into(),
            ));
        }
        let source_paths = if tool_policy.allow_tools {
            self.workspace
                .list_selected_library_files(&requested_source_paths)
                .map_err(|error| AgentError::Runtime(error.to_string()))?
                .into_iter()
                .map(|entry| entry.relative_path)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let run_id = Uuid::new_v4().to_string();
        let now = self
            .clock
            .now()
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let turn = AgentTurn {
            id: run_id.clone(),
            mode: serde_json::to_string(&mode)
                .unwrap_or_else(|_| "\"chat\"".into())
                .trim_matches('"')
                .to_owned(),
            notebook_id: metadata.notebook_id,
            config_json: metadata.config_json,
            goal: goal.clone(),
            source_paths: source_paths.clone(),
            allow_tools: tool_policy.allow_tools,
            read_only: tool_policy.read_only,
            history_json: serde_json::to_string(&messages).unwrap_or_else(|_| "[]".into()),
            status: "started".into(),
            stage: None,
            loop_index: 0,
            last_sequence: 0,
            result_json: None,
            error: None,
            checkpoint: "safe".into(),
            resume_safe: true,
            created_at: now.clone(),
            updated_at: now,
        };
        let control = Arc::new(RunControl::new(run_id.clone(), turn));
        let mut state = self.state.lock();
        if state.active_run.is_some() {
            return Err(AgentError::Busy);
        }
        state.active_run = Some(run_id.clone());
        state.runs.insert(run_id.clone(), Arc::clone(&control));
        drop(state);
        if let Err(error) = self.persist_turn(&control) {
            // Same fail-fast contract as the chat entry point: without the
            // initial checkpoint there is nothing to recover, so the start
            // fails and releases the reserved slot instead of spawning a run
            // that reports success while unpersisted.
            let mut state = self.state.lock();
            if state.active_run.as_deref() == Some(&run_id) {
                state.active_run = None;
            }
            state.runs.remove(&run_id);
            return Err(AgentError::Runtime(error));
        }
        let runtime = Arc::clone(self);
        thread::Builder::new()
            .name(format!("studypulse-agent-{run_id}"))
            .spawn(move || {
                let executor = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match executor {
                    Ok(executor) => executor.block_on(runtime.run_agent(
                        control,
                        mode,
                        goal,
                        source_paths,
                        messages,
                        attachments,
                        tool_policy,
                    )),
                    Err(error) => runtime.finish_with_error(&control, error.to_string()),
                }
            })
            .map_err(|error| AgentError::Runtime(error.to_string()))?;
        Ok(run_id)
    }

    pub fn cancel_agent(&self, run_id: &str) -> Result<(), AgentError> {
        // Cancellation is cooperative but reaches every blocking surface.  A
        // model future observes Notify, while confirmation/input/event waiters
        // observe their Condvars; one flag provides the shared terminal cause.
        // Cancellation is broadcast to every possible waiter.  Waking only
        // the model would leave a confirmation/input waiter blocked forever.
        let control = self.control(run_id)?;
        if control.status.lock().is_terminal() {
            return Ok(());
        }
        control.cancelled.store(true, Ordering::Release);
        *control.status.lock() = RunStatus::Cancelling;
        self.emit(
            &control,
            AgentEventKind::StatusChanged,
            EventFields {
                status: Some(RunStatus::Cancelling),
                ..EventFields::default()
            },
        );
        control.confirmation_changed.notify_all();
        control.input_changed.notify_all();
        control.events_changed.notify_all();
        control.cancellation_changed.notify_one();
        Ok(())
    }

    pub fn submit_confirmation(
        &self,
        run_id: &str,
        confirmation_id: &str,
        decision: ConfirmationDecision,
    ) -> Result<(), AgentError> {
        // The id is intentionally supplied by the UI event and checked against
        // the pending state.  Treating a decision as a bare allow/deny boolean
        // would make delayed or duplicated UI responses unsafe.
        // Match both run-local id and one-shot state so a stale or replayed UI
        // response cannot authorize a later tool call.
        let control = self.control(run_id)?;
        let mut confirmation = control.confirmation.lock();
        let Some(pending) = confirmation.as_mut() else {
            return Err(AgentError::ConfirmationNotFound);
        };
        if pending.id != confirmation_id || pending.decision.is_some() {
            return Err(AgentError::ConfirmationNotFound);
        }
        pending.decision = Some(decision);
        control.confirmation_changed.notify_all();
        Ok(())
    }

    pub fn submit_input(
        &self,
        run_id: &str,
        input_id: &str,
        answer_json: String,
    ) -> Result<(), AgentError> {
        // Input is JSON text because the model protocol can carry structured
        // answers, but the bounded size keeps a paused run from becoming an
        // unbounded memory or prompt-injection channel.
        if answer_json.len() > 8_000 || answer_json.trim().is_empty() {
            return Err(AgentError::Runtime(
                "Agent input is empty or too large".into(),
            ));
        }
        let control = self.control(run_id)?;
        let mut input = control.input.lock();
        let Some(pending) = input.as_mut() else {
            return Err(AgentError::InputNotFound);
        };
        if pending.id != input_id || pending.answer.is_some() {
            return Err(AgentError::InputNotFound);
        }
        pending.answer = Some(answer_json);
        control.input_changed.notify_all();
        Ok(())
    }

    pub fn run_status(&self, run_id: &str) -> Result<RunStatus, AgentError> {
        let control = self.control(run_id)?;
        let status = *control.status.lock();
        Ok(status)
    }

    pub fn wait_for_events(
        &self,
        run_id: &str,
        after_sequence: u64,
        timeout_ms: u32,
    ) -> Result<Vec<AgentEvent>, AgentError> {
        // Polling uses an exclusive sequence cursor rather than vector length.
        // Events may be persisted, filtered, or drained independently, while
        // sequence remains the only stable ordering contract for clients.
        // `after_sequence` is an exclusive cursor.  A bounded Condvar wait
        // avoids busy polling, and the terminal check lets callers return even
        // when no new event arrives after the final transition.
        let control = match self.control(run_id) {
            Ok(control) => control,
            Err(AgentError::RunNotFound) => {
                let turn = self
                    .workspace
                    .read_agent_turn(run_id)
                    .map_err(|_| AgentError::RunNotFound)?;
                let _ = turn;
                let events = self
                    .workspace
                    .read_agent_run_log(run_id)
                    .map_err(|error| AgentError::Runtime(error.to_string()))?
                    .into_iter()
                    .filter_map(|line| serde_json::from_str::<AgentEvent>(&line).ok())
                    .filter(|event| event.sequence > after_sequence)
                    .collect();
                return Ok(events);
            }
            Err(error) => return Err(error),
        };
        let mut buffer = control.events.lock();
        if !buffer
            .events
            .iter()
            .any(|event| event.sequence > after_sequence)
            && !control.status.lock().is_terminal()
        {
            control.events_changed.wait_for(
                &mut buffer,
                StdDuration::from_millis(u64::from(timeout_ms.min(30_000))),
            );
        }
        Ok(buffer
            .events
            .iter()
            .filter(|event| event.sequence > after_sequence)
            .cloned()
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_agent(
        self: Arc<Self>,
        control: Arc<RunControl>,
        mode: AgentMode,
        goal: String,
        source_paths: Vec<String>,
        mut messages: Vec<ChatMessage>,
        attachments: Vec<ModelImageAttachment>,
        tool_policy: ToolPolicy,
    ) {
        // This loop is the only place where a model response becomes a new
        // runtime action.  Keeping the ordering explicit makes the safety
        // review straightforward: build a request, stream its response,
        // prepare each proposed tool, obtain consent when required, execute,
        // and append the normalized result before the next model turn.
        // The run starts with a visible event and stage, then converts history
        // into the provider-neutral ChatMessage protocol.  Tool definitions are
        // descriptive input to the model; they do not grant execution rights.
        self.emit(
            &control,
            AgentEventKind::Started,
            EventFields {
                status: Some(RunStatus::Started),
                text: Some(goal.clone()),
                mode: Some(mode),
                stage: Some(
                    self.capabilities
                        .manifest(mode)
                        .and_then(|manifest| manifest.stages.first().cloned())
                        .unwrap_or_else(|| "responding".into()),
                ),
                ..EventFields::default()
            },
        );
        self.set_status(&control, RunStatus::Running);
        self.emit(
            &control,
            AgentEventKind::StageStarted,
            EventFields {
                mode: Some(mode),
                stage: Some(stage_for(mode, 0)),
                progress: Some(0.0),
                ..EventFields::default()
            },
        );
        if let Err(error) = self.checkpoint_messages(&control, &messages, 0, "safe", true) {
            self.finish_with_error(&control, error);
            return;
        }
        // Permission is serialized as a hint for weaker models, not as an
        // authorization grant.  The host repeats the decision at prepare time.
        let definitions = if tool_policy.allow_tools {
            self.tools
                .definitions()
                .into_iter()
                .filter(|definition| tool_is_enabled(mode, &definition.name))
                .filter(|definition| {
                    !tool_policy.read_only || definition.permission == PermissionLevel::Read
                })
                .map(|definition| ModelToolDefinition {
                    name: definition.name,
                    description: definition.description,
                    parameters: definition.parameters,
                    permission: Some(
                        serde_json::to_string(&definition.permission)
                            .unwrap_or_else(|_| "\"read\"".into())
                            .trim_matches('"')
                            .to_owned(),
                    ),
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let manifest = self.capabilities.manifest(mode);
        let max_loops = manifest
            .as_ref()
            .map_or(MAX_AGENT_LOOPS, |manifest| manifest.max_loops as usize);
        // Each loop represents one model turn.  The cap prevents a faulty or
        // adversarial provider from creating an endless tool-call conversation.
        // A loop budget is a liveness guard as well as a cost guard.  A model
        // that keeps proposing harmless reads must still eventually produce a
        // terminal event, otherwise the desktop poller would wait forever.
        for loop_index in 0..max_loops {
            if self.finish_if_cancelled(&control) {
                return;
            }
            // The request snapshot includes current history, definitions, mode,
            // and stage labels.  It is cloned so provider implementations cannot
            // mutate runtime-owned conversation state.
            let request = ModelRequest {
                messages: messages.clone(),
                tools: definitions.clone(),
                attachments: attachments.clone(),
                mode: Some(
                    serde_json::to_string(&mode)
                        .unwrap_or_else(|_| "chat".into())
                        .trim_matches('"')
                        .to_owned(),
                ),
                stages: manifest
                    .as_ref()
                    .map(|manifest| manifest.stages.clone())
                    .unwrap_or_default(),
            };
            let prompt_characters = serde_json::to_string(&request.messages)
                .map(|value| value.len())
                .unwrap_or_default();
            self.emit(
                &control,
                AgentEventKind::StageProgress,
                EventFields {
                    mode: Some(mode),
                    stage: Some(stage_for(mode, loop_index)),
                    progress: Some(loop_index as f64 / max_loops as f64),
                    ..EventFields::default()
                },
            );
            let streamed_text = Arc::new(Mutex::new(String::new()));
            let streamed_text_for_handler = Arc::clone(&streamed_text);
            let runtime_for_handler = Arc::clone(&self);
            let control_for_handler = Arc::clone(&control);
            let on_text_delta: ModelTextDeltaHandler = Arc::new(move |delta| {
                // Providers may stream the same logical answer that is returned
                // again in `ModelResponse`; retain it for de-duplication while
                // emitting only non-empty deltas and ignoring post-cancel text.
                if delta.is_empty() || control_for_handler.cancelled.load(Ordering::SeqCst) {
                    return;
                }
                streamed_text_for_handler.lock().push_str(&delta);
                runtime_for_handler.emit(
                    &control_for_handler,
                    AgentEventKind::TextDelta,
                    EventFields {
                        text: Some(delta),
                        ..EventFields::default()
                    },
                );
            });
            // Provider cancellation and provider completion race here.  The
            // biased select makes an already-requested user cancellation win
            // deterministically, preventing late text from reopening a run
            // that the UI has already considered stopped.
            let response = match tokio::select! {
                biased;
                // Cancellation is deliberately biased so a finished user
                // action wins over a provider that is slow to resolve.
                _ = control.cancellation_changed.notified() => {
                    self.finish(
                        &control,
                        RunStatus::Cancelled,
                        AgentEventKind::Cancelled,
                        None,
                    );
                    return;
                }
                response = self.model.complete(request, on_text_delta) => response
            } {
                Ok(response) => response,
                Err(error) => {
                    // Provider errors enter the same terminal cleanup path as
                    // loop-limit failures, ensuring polling never hangs on an
                    // unreported rejected future.
                    self.finish_with_error(&control, error.to_string());
                    return;
                }
            };

            if (!tool_policy.allow_tools || tool_policy.read_only)
                && response.tool_calls.iter().any(|call| {
                    !definitions
                        .iter()
                        .any(|definition| definition.name == call.name)
                })
            {
                self.finish_with_error(
                    &control,
                    "the requested tool is outside this AI feature's read-only source scope".into(),
                );
                return;
            }

            let streamed_text = streamed_text.lock().clone();
            self.record_usage(
                &control,
                response.usage.as_ref(),
                prompt_characters,
                streamed_text.len() + response.text_deltas.iter().map(String::len).sum::<usize>(),
            );
            // A provider can deliver text incrementally and also return the
            // complete text.  Emit only the suffix not already sent to the UI.
            let returned_text = response.text_deltas.concat();
            let remaining_deltas = if streamed_text.is_empty() {
                response.text_deltas
            } else if let Some(remaining) = returned_text.strip_prefix(&streamed_text) {
                if remaining.is_empty() {
                    Vec::new()
                } else {
                    vec![remaining.to_owned()]
                }
            } else {
                Vec::new()
            };
            for delta in remaining_deltas {
                if self.finish_if_cancelled(&control) {
                    return;
                }
                self.emit(
                    &control,
                    AgentEventKind::TextDelta,
                    EventFields {
                        text: Some(delta.clone()),
                        ..EventFields::default()
                    },
                );
            }
            let assistant_text = if returned_text.is_empty() {
                streamed_text
            } else {
                returned_text
            };
            if !assistant_text.is_empty() {
                messages.push(ChatMessage::Assistant {
                    content: assistant_text,
                });
            }
            if let Err(error) =
                self.checkpoint_messages(&control, &messages, loop_index, "safe", true)
            {
                self.finish_with_error(&control, error);
                return;
            }

            if response.tool_calls.is_empty() {
                // No tool call means the model turn is the final answer.  The
                // terminal event is emitted only after the final stage marker.
                if let Err(error) = self
                    .capabilities
                    .validate_result(mode, &control.final_text.lock())
                {
                    self.finish_with_error(&control, error);
                    return;
                }
                self.emit(
                    &control,
                    AgentEventKind::StageCompleted,
                    EventFields {
                        mode: Some(mode),
                        stage: Some(stage_for(mode, loop_index)),
                        progress: Some(1.0),
                        ..EventFields::default()
                    },
                );
                self.finish(
                    &control,
                    RunStatus::Completed,
                    AgentEventKind::Completed,
                    None,
                );
                return;
            }

            // Read-only calls can use a bounded worker batch. Writes, deletes,
            // execution, and interactive input stay sequential so no side
            // effect can pass an unapproved or stale confirmation boundary.
            if response.tool_calls.len() > 1
                && self.dispatch_read_batch(
                    &control,
                    mode,
                    loop_index,
                    &response.tool_calls,
                    &source_paths,
                    &mut messages,
                )
            {
                // The batch helper ends with the same "safe" checkpoint the
                // sequential path takes; it lives at the call site so a failed
                // write can end the run instead of falling through to another
                // loop iteration.
                if let Err(error) =
                    self.checkpoint_messages(&control, &messages, loop_index, "safe", true)
                {
                    self.finish_with_error(&control, error);
                    return;
                }
                continue;
            }
            for call in response.tool_calls {
                // Prepare is intentionally before every permission check: it
                // validates arguments and creates a preview without touching
                // Workspace, so consent can be shown before any side effect.
                if self.finish_if_cancelled(&control) {
                    return;
                }
                let prepared =
                    match self
                        .tools
                        .prepare(call.id.clone(), &call.name, call.arguments.clone())
                    {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            let result = tool_error_result("invalid_arguments", &error.to_string());
                            self.emit(
                                &control,
                                AgentEventKind::ToolCompleted,
                                EventFields {
                                    tool_call_id: Some(call.id.clone()),
                                    tool_name: Some(call.name.clone()),
                                    payload_json: Some(result.clone()),
                                    ..EventFields::default()
                                },
                            );
                            messages.push(ChatMessage::Tool {
                                call_id: call.id,
                                name: call.name,
                                content: result,
                            });
                            continue;
                        }
                    };
                self.emit(
                    &control,
                    AgentEventKind::ToolRequested,
                    EventFields {
                        tool_call_id: Some(prepared.call_id.clone()),
                        tool_name: Some(prepared.name.clone()),
                        permission: Some(prepared.permission),
                        preview: Some(prepared.preview.clone()),
                        payload_json: Some(call.arguments.to_string()),
                        ..EventFields::default()
                    },
                );

                if prepared.permission != PermissionLevel::Read {
                    // Write, destructive, and execute tools all pause here.
                    // Denial is fed back as structured JSON so the model can
                    // recover without the host pretending the operation ran.
                    let confirmation_id = Uuid::new_v4().to_string();
                    {
                        *control.confirmation.lock() = Some(ConfirmationState {
                            id: confirmation_id.clone(),
                            decision: None,
                        });
                    }
                    self.set_status(&control, RunStatus::WaitingForConfirmation);
                    self.emit(
                        &control,
                        AgentEventKind::ConfirmationRequired,
                        EventFields {
                            tool_call_id: Some(prepared.call_id.clone()),
                            tool_name: Some(prepared.name.clone()),
                            permission: Some(prepared.permission),
                            preview: Some(prepared.preview.clone()),
                            confirmation_id: Some(confirmation_id),
                            payload_json: Some(call.arguments.to_string()),
                            ..EventFields::default()
                        },
                    );
                    if let Err(error) = self.checkpoint_messages(
                        &control,
                        &messages,
                        loop_index,
                        "waiting_confirmation",
                        true,
                    ) {
                        self.finish_with_error(&control, error);
                        return;
                    }
                    let decision = self.wait_for_confirmation(&control);
                    if self.finish_if_cancelled(&control) {
                        return;
                    }
                    self.set_status(&control, RunStatus::Running);
                    if decision != Some(ConfirmationDecision::Allow) {
                        let result = json!({
                            "ok": false,
                            "error": {"code": "user_denied", "message": "User denied the requested operation"}
                        })
                        .to_string();
                        self.emit(
                            &control,
                            AgentEventKind::ToolCompleted,
                            EventFields {
                                tool_call_id: Some(prepared.call_id.clone()),
                                tool_name: Some(prepared.name.clone()),
                                permission: Some(prepared.permission),
                                payload_json: Some(result.clone()),
                                ..EventFields::default()
                            },
                        );
                        messages.push(ChatMessage::Tool {
                            call_id: prepared.call_id,
                            name: prepared.name,
                            content: result,
                        });
                        if let Err(error) =
                            self.checkpoint_messages(&control, &messages, loop_index, "safe", true)
                        {
                            self.finish_with_error(&control, error);
                            return;
                        }
                        continue;
                    }
                }

                if prepared.name == "ask_user" {
                    // `ask_user` is a read-level pause for clarification, not a
                    // Workspace operation.  Its answer re-enters the model as
                    // a Tool message after the one-shot input is consumed.
                    let input_id = Uuid::new_v4().to_string();
                    {
                        *control.input.lock() = Some(InputState {
                            id: input_id.clone(),
                            answer: None,
                        });
                    }
                    self.set_status(&control, RunStatus::WaitingForConfirmation);
                    self.emit(
                        &control,
                        AgentEventKind::InputRequired,
                        EventFields {
                            mode: Some(mode),
                            stage: Some(stage_for(mode, loop_index)),
                            confirmation_id: Some(input_id),
                            preview: Some(call.arguments.to_string()),
                            payload_json: Some(call.arguments.to_string()),
                            ..EventFields::default()
                        },
                    );
                    let answer = self.wait_for_input(&control);
                    if self.finish_if_cancelled(&control) {
                        return;
                    }
                    self.set_status(&control, RunStatus::Running);
                    let result = answer
                        .map(|answer| json!({"ok": true, "answer": answer}).to_string())
                        .unwrap_or_else(|| {
                            tool_error_result("input_missing", "user did not answer")
                        });
                    self.emit(
                        &control,
                        AgentEventKind::ToolCompleted,
                        EventFields {
                            tool_call_id: Some(prepared.call_id.clone()),
                            tool_name: Some(prepared.name.clone()),
                            payload_json: Some(result.clone()),
                            ..EventFields::default()
                        },
                    );
                    messages.push(ChatMessage::Tool {
                        call_id: prepared.call_id,
                        name: prepared.name,
                        content: result,
                    });
                    if let Err(error) =
                        self.checkpoint_messages(&control, &messages, loop_index, "safe", true)
                    {
                        self.finish_with_error(&control, error);
                        return;
                    }
                    continue;
                }

                // The tool result is always converted to a Tool message.  The
                // model therefore sees success and failure through one
                // protocol, instead of through host-only side channels that
                // would make retries or explanations inconsistent.
                // The unsafe-boundary checkpoint must land before execution:
                // without it a crash during a side-effecting tool would leave
                // an unresumable turn, so a failed write ends the run here.
                if let Err(error) = self.checkpoint_messages(
                    &control,
                    &messages,
                    loop_index,
                    "executing_tool",
                    false,
                ) {
                    self.finish_with_error(&control, error);
                    return;
                }
                let result = self
                    // Only this execute path is allowed to perform the prepared
                    // invocation.  The selected Notebook sources are passed
                    // again so read tools cannot escape the UI selection.
                    .tools
                    .execute_for_sources(
                        prepared.clone(),
                        &self.workspace,
                        self.clock.now(),
                        &source_paths,
                    )
                    .map(|value| value.to_string())
                    .unwrap_or_else(|error| tool_error_result("tool_error", &error.to_string()));
                self.collect_sources(&control, &prepared, &result);
                self.emit(
                    &control,
                    AgentEventKind::ToolCompleted,
                    EventFields {
                        tool_call_id: Some(prepared.call_id.clone()),
                        tool_name: Some(prepared.name.clone()),
                        permission: Some(prepared.permission),
                        payload_json: Some(result.clone()),
                        ..EventFields::default()
                    },
                );
                if prepared.name == "save_artifact" && result.contains("\"ok\":true") {
                    self.collect_artifact(&control, &result);
                    self.emit(
                        &control,
                        AgentEventKind::ArtifactCreated,
                        EventFields {
                            mode: Some(mode),
                            stage: Some(stage_for(mode, loop_index)),
                            payload_json: Some(result.clone()),
                            ..EventFields::default()
                        },
                    );
                }
                messages.push(ChatMessage::Tool {
                    call_id: prepared.call_id,
                    name: prepared.name,
                    content: result,
                });
                if let Err(error) =
                    self.checkpoint_messages(&control, &messages, loop_index, "safe", true)
                {
                    self.finish_with_error(&control, error);
                    return;
                }
            }
        }
        self.finish_with_error(
            &control,
            format!("Agent exceeded the maximum of {max_loops} model loops"),
        );
    }

    fn dispatch_read_batch(
        &self,
        control: &Arc<RunControl>,
        mode: AgentMode,
        loop_index: usize,
        calls: &[ModelToolCall],
        source_paths: &[String],
        messages: &mut Vec<ChatMessage>,
    ) -> bool {
        // Only a batch made entirely of known read tools can enter this path.
        // `ask_user` remains sequential because it changes the run state, and
        // every write/execute operation remains behind the normal consent gate.
        if calls.len() > MAX_PARALLEL_READ_TOOLS {
            return false;
        }
        let definitions = self.tools.definitions();
        if calls.iter().any(|call| {
            call.name == "ask_user"
                || definitions
                    .iter()
                    .find(|definition| definition.name == call.name)
                    .is_none_or(|definition| definition.permission != PermissionLevel::Read)
        }) {
            return false;
        }
        let prepared = calls
            .iter()
            .map(|call| {
                self.tools
                    .prepare(call.id.clone(), &call.name, call.arguments.clone())
            })
            .collect::<Result<Vec<_>, _>>();
        let Ok(prepared) = prepared else {
            return false;
        };
        for (call, tool) in calls.iter().zip(&prepared) {
            self.emit(
                control,
                AgentEventKind::ToolRequested,
                EventFields {
                    tool_call_id: Some(tool.call_id.clone()),
                    tool_name: Some(tool.name.clone()),
                    permission: Some(tool.permission),
                    preview: Some(tool.preview.clone()),
                    payload_json: Some(call.arguments.to_string()),
                    ..EventFields::default()
                },
            );
        }

        let registry = self.tools.clone();
        let workspace = self.workspace.clone();
        let selected_sources = source_paths.to_vec();
        let now = self.clock.now();
        let handles = prepared
            .iter()
            .cloned()
            .map(|tool| {
                let registry = registry.clone();
                let workspace = workspace.clone();
                let selected_sources = selected_sources.clone();
                std::thread::spawn(move || {
                    registry
                        .execute_for_sources(tool, &workspace, now, &selected_sources)
                        .map(|value| value.to_string())
                        .unwrap_or_else(|error| tool_error_result("tool_error", &error.to_string()))
                })
            })
            .collect::<Vec<_>>();

        for (tool, handle) in prepared.into_iter().zip(handles) {
            let result = handle
                .join()
                .unwrap_or_else(|_| tool_error_result("worker_panic", "read tool worker panicked"));
            self.collect_sources(control, &tool, &result);
            self.emit(
                control,
                AgentEventKind::ToolCompleted,
                EventFields {
                    tool_call_id: Some(tool.call_id.clone()),
                    tool_name: Some(tool.name.clone()),
                    permission: Some(tool.permission),
                    payload_json: Some(result.clone()),
                    ..EventFields::default()
                },
            );
            messages.push(ChatMessage::Tool {
                call_id: tool.call_id,
                name: tool.name,
                content: result,
            });
        }
        self.emit(
            control,
            AgentEventKind::Observation,
            EventFields {
                mode: Some(mode),
                stage: Some(stage_for(mode, loop_index)),
                text: Some(format!(
                    "Completed {} read-only tool calls in parallel",
                    calls.len()
                )),
                ..EventFields::default()
            },
        );
        true
    }

    fn collect_sources(
        &self,
        control: &Arc<RunControl>,
        tool: &studypulse_tools::PreparedTool,
        result: &str,
    ) {
        let Ok(value) = serde_json::from_str::<Value>(result) else {
            return;
        };
        let Some(results) = value.get("results").and_then(Value::as_array) else {
            if tool.name == "read_source" {
                let locator = value
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if !locator.is_empty() {
                    control.sources.lock().push(SourceRef {
                        source_type: "workspace".into(),
                        locator,
                        title: None,
                        excerpt: value
                            .get("content")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        tool_call_id: Some(tool.call_id.clone()),
                    });
                }
            }
            return;
        };
        let mut sources = control.sources.lock();
        for item in results {
            let locator = item
                .get("url")
                .or_else(|| item.get("path"))
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if locator.is_empty() {
                continue;
            }
            sources.push(SourceRef {
                source_type: item
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or(if tool.name == "paper_search" {
                        "paper"
                    } else {
                        "web"
                    })
                    .into(),
                locator: locator.into(),
                title: item.get("title").and_then(Value::as_str).map(str::to_owned),
                excerpt: item
                    .get("snippet")
                    .or_else(|| item.get("abstract"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                tool_call_id: Some(tool.call_id.clone()),
            });
        }
        if let Ok(payload) = serde_json::to_string(&*sources) {
            drop(sources);
            self.emit(
                control,
                AgentEventKind::Sources,
                EventFields {
                    tool_call_id: Some(tool.call_id.clone()),
                    tool_name: Some(tool.name.clone()),
                    payload_json: Some(payload),
                    ..EventFields::default()
                },
            );
        }
    }

    fn collect_artifact(&self, control: &Arc<RunControl>, result: &str) {
        let Ok(value) = serde_json::from_str::<Value>(result) else {
            return;
        };
        let Some(path) = value.get("relative_path").and_then(Value::as_str) else {
            return;
        };
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let Some((artifact_id, extension)) = file_name.rsplit_once('.') else {
            return;
        };
        control.artifacts.lock().push(ArtifactRef {
            artifact_id: value
                .get("artifact_id")
                .and_then(Value::as_str)
                .unwrap_or(artifact_id)
                .into(),
            path: path.into(),
            extension: extension.into(),
            render_type: match extension {
                "svg" => Some("svg".into()),
                "html" => Some("html".into()),
                "json" => Some("structured".into()),
                "md" | "markdown" => Some("markdown".into()),
                _ => None,
            },
        });
    }

    fn wait_for_confirmation(&self, control: &RunControl) -> Option<ConfirmationDecision> {
        // Confirmation is a one-shot handoff between the event stream and a
        // UI command.  The lock guards both the request identity and its
        // decision, so a response from an older render cannot authorize a new
        // invocation that happens to use the same screen.
        // Condvar waits are rechecked in a loop because wakeups are not proof
        // of a decision.  Cancellation clears the pending request so a later
        // stale response cannot apply after the run has ended.
        let mut confirmation = control.confirmation.lock();
        loop {
            if control.cancelled.load(Ordering::Acquire) {
                *confirmation = None;
                return None;
            }
            if let Some(decision) = confirmation.as_ref().and_then(|value| value.decision) {
                *confirmation = None;
                return Some(decision);
            }
            control
                .confirmation_changed
                .wait_for(&mut confirmation, StdDuration::from_millis(100));
        }
    }

    fn wait_for_input(&self, control: &RunControl) -> Option<String> {
        // User input uses the same ownership rule as confirmation: consume the
        // answer while holding the state lock, then clear it before returning.
        // This prevents duplicate submissions from becoming two tool messages.
        // Input follows the same one-shot consume rule as confirmation, but it
        // returns the raw bounded JSON string for the model tool message.
        let mut input = control.input.lock();
        loop {
            if control.cancelled.load(Ordering::Acquire) {
                *input = None;
                return None;
            }
            if let Some(answer) = input.as_ref().and_then(|value| value.answer.clone()) {
                *input = None;
                return Some(answer);
            }
            control
                .input_changed
                .wait_for(&mut input, StdDuration::from_millis(100));
        }
    }

    fn finish_if_cancelled(&self, control: &Arc<RunControl>) -> bool {
        // Check cancellation at loop, callback, and tool boundaries so a run
        // cannot begin another side effect after the user has cancelled it.
        if control.cancelled.load(Ordering::Acquire) {
            self.finish(
                control,
                RunStatus::Cancelled,
                AgentEventKind::Cancelled,
                None,
            );
            true
        } else {
            false
        }
    }

    fn finish_with_error(&self, control: &Arc<RunControl>, message: String) {
        // Provider and runtime failures share the normal terminal path, which
        // keeps active-run cleanup and event wakeups consistent.
        self.finish(
            control,
            RunStatus::Failed,
            AgentEventKind::Failed,
            Some(message),
        );
    }

    fn finish(
        &self,
        control: &Arc<RunControl>,
        status: RunStatus,
        kind: AgentEventKind,
        text: Option<String>,
    ) {
        // All terminal exits converge here, including cancellation, provider
        // failure, and the loop cap.  Centralization prevents one exit path
        // from forgetting to clear the active-run guard or wake pollers.
        // Terminal cleanup is centralized: update status, release the
        // single-active slot, append the terminal event, and wake any
        // event/consent waiter that may still be observing the run.
        *control.status.lock() = status;
        {
            // The slot is released before the terminal transition becomes
            // observable: once run_status reports a terminal status or the
            // terminal event is delivered, a caller that immediately starts
            // the next run must not race into AgentError::Busy.
            let mut state = self.state.lock();
            if state.active_run.as_deref() == Some(&control.run_id) {
                state.active_run = None;
            }
        }
        let usage_json = serde_json::to_string(&*control.usage.lock()).ok();
        self.emit(
            control,
            AgentEventKind::Usage,
            EventFields {
                payload_json: usage_json,
                ..EventFields::default()
            },
        );
        if status == RunStatus::Completed {
            let text = control.final_text.lock().clone();
            let output_json = serde_json::from_str::<Value>(text.trim())
                .ok()
                .map(|value| value.to_string());
            {
                let mut turn = control.turn.lock();
                turn.result_json = output_json;
                turn.checkpoint = "completed".into();
                turn.resume_safe = false;
                turn.updated_at = self
                    .clock
                    .now()
                    .to_rfc3339_opts(SecondsFormat::Millis, true);
                let snapshot = turn.clone();
                drop(turn);
                if let Err(error) = self.workspace.write_agent_turn(&snapshot) {
                    // The run is already terminal and the live UI received the
                    // Result event; demoting a completed run over the
                    // post-terminal write would discard finished work, so the
                    // failure stays a logged warning.
                    tracing::warn!(
                        run_id = %control.run_id,
                        error = %error,
                        "failed to persist completed Agent turn",
                    );
                }
            }
            if let Ok(result) = serde_json::to_string(&self.turn_result(control)) {
                self.emit(
                    control,
                    AgentEventKind::Result,
                    EventFields {
                        payload_json: Some(result),
                        ..EventFields::default()
                    },
                );
            }
        } else {
            let mut turn = control.turn.lock();
            turn.error = text.clone();
            turn.checkpoint = "terminal".into();
            turn.resume_safe = false;
            turn.updated_at = self
                .clock
                .now()
                .to_rfc3339_opts(SecondsFormat::Millis, true);
            let snapshot = turn.clone();
            drop(turn);
            if let Err(error) = self.workspace.write_agent_turn(&snapshot) {
                tracing::warn!(
                    run_id = %control.run_id,
                    error = %error,
                    "failed to persist terminal Agent turn",
                );
            }
        }
        self.emit(
            control,
            kind,
            EventFields {
                status: Some(status),
                text,
                ..EventFields::default()
            },
        );
        control.events_changed.notify_all();
        control.confirmation_changed.notify_all();
    }

    fn set_status(&self, control: &Arc<RunControl>, status: RunStatus) {
        // Status is written before its event is emitted, so a poller that wakes
        // on the event observes the same state through run_status immediately.
        // Status changes are events as well as in-memory state, so UI clients
        // never need to infer transitions from tool or text events.
        *control.status.lock() = status;
        self.emit(
            control,
            AgentEventKind::StatusChanged,
            EventFields {
                status: Some(status),
                ..EventFields::default()
            },
        );
    }

    fn emit(&self, control: &Arc<RunControl>, kind: AgentEventKind, fields: EventFields) {
        // Constructing the event before locking the buffer gives every sink the
        // same timestamp, cursor, and optional payload.  That is what allows a
        // UI to reconcile live events with a replayed JSONL run log.
        // Allocate the sequence exactly once, then write the same event to the
        // durable JSONL timeline and the condition-variable-backed buffer.  The
        // two sinks intentionally share the wire object for replay consistency.
        let event = AgentEvent {
            run_id: control.run_id.clone(),
            sequence: control.next_sequence.fetch_add(1, Ordering::AcqRel),
            timestamp: self
                .clock
                .now()
                .to_rfc3339_opts(SecondsFormat::Millis, true),
            kind,
            status: fields.status,
            text: fields.text,
            tool_call_id: fields.tool_call_id,
            tool_name: fields.tool_name,
            permission: fields.permission,
            preview: fields.preview,
            confirmation_id: fields.confirmation_id,
            payload_json: fields.payload_json,
            mode: fields.mode,
            stage: fields.stage,
            progress: fields.progress,
        };
        {
            let mut turn = control.turn.lock();
            turn.last_sequence = event.sequence;
            if let Some(status) = event.status {
                turn.status = serde_json::to_string(&status)
                    .unwrap_or_else(|_| "\"running\"".into())
                    .trim_matches('"')
                    .to_owned();
            }
            if event.stage.is_some() {
                turn.stage = event.stage.clone();
            }
            turn.updated_at = event.timestamp.clone();
            let snapshot = turn.clone();
            drop(turn);
            if let Err(error) = self.workspace.write_agent_turn(&snapshot) {
                // The per-event snapshot mirrors in-memory state; the live run
                // stays driven by memory, but a failed mirror must at least be
                // observable instead of silently losing restart recovery.
                tracing::warn!(
                    run_id = %control.run_id,
                    error = %error,
                    "failed to persist Agent turn snapshot",
                );
            }
        }
        if event.kind == AgentEventKind::TextDelta
            && let Some(text) = &event.text
        {
            control.final_text.lock().push_str(text);
        }
        self.persist_event(&event);
        control.events.lock().events.push(event);
        control.events_changed.notify_all();
    }

    fn persist_event(&self, event: &AgentEvent) {
        // The log is an audit/replay aid rather than the live synchronization
        // source.  If it cannot be opened, the in-memory event still reaches
        // the current UI and the run can complete normally.
        // Persistence is append-only and best-effort here; the in-memory event
        // stream remains the synchronization source while JSONL enables later
        // inspection of a run without changing its live state machine.  A
        // failed append is logged so timeline loss is visible in diagnostics
        // even though the run itself keeps going.
        if let Ok(line) = serde_json::to_string(event)
            && let Err(error) = self.workspace.append_agent_run_log(&event.run_id, &line)
        {
            tracing::warn!(
                run_id = %event.run_id,
                error = %error,
                "failed to append Agent run log",
            );
        }
    }

    fn persist_turn(&self, control: &Arc<RunControl>) -> Result<(), String> {
        // The initial checkpoint anchors crash recovery for the whole turn, so
        // callers treat a failed write as a failed start instead of reporting
        // a run whose recovery anchor silently never reached the disk.
        let turn = control.turn.lock().clone();
        self.workspace
            .write_agent_turn(&turn)
            .map_err(|error| format!("failed to persist Agent checkpoint: {error}"))
    }

    fn record_usage(
        &self,
        control: &Arc<RunControl>,
        usage: Option<&ModelUsage>,
        prompt_characters: usize,
        completion_characters: usize,
    ) {
        let mut summary = control.usage.lock();
        let value = usage.cloned().unwrap_or_else(|| {
            let prompt_tokens = (prompt_characters as u64).div_ceil(4);
            let completion_tokens = (completion_characters as u64).div_ceil(4);
            ModelUsage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens.saturating_add(completion_tokens),
                estimated: true,
            }
        });
        summary.prompt_tokens = summary.prompt_tokens.saturating_add(value.prompt_tokens);
        summary.completion_tokens = summary
            .completion_tokens
            .saturating_add(value.completion_tokens);
        summary.total_tokens = summary.total_tokens.saturating_add(value.total_tokens);
        summary.model_calls = summary.model_calls.saturating_add(1);
        summary.estimated |= value.estimated;
    }

    fn turn_result(&self, control: &Arc<RunControl>) -> TurnResult {
        let turn = control.turn.lock().clone();
        let mode = serde_json::from_str::<AgentMode>(&format!("\"{}\"", turn.mode))
            .unwrap_or(AgentMode::Chat);
        let result_kind = self
            .capabilities
            .manifest(mode)
            .map(|manifest| manifest.result_kind)
            .unwrap_or_else(|| "markdown".into());
        TurnResult {
            schema_version: TURN_RESULT_SCHEMA_VERSION,
            mode,
            result_kind,
            text: control.final_text.lock().clone(),
            output_json: turn.result_json.clone(),
            sources: control.sources.lock().clone(),
            artifacts: control.artifacts.lock().clone(),
            usage: control.usage.lock().clone(),
        }
    }

    fn checkpoint_messages(
        &self,
        control: &Arc<RunControl>,
        messages: &[ChatMessage],
        loop_index: usize,
        checkpoint: &str,
        resume_safe: bool,
    ) -> Result<(), String> {
        // Labeled checkpoints are the explicit-recovery contract: if one cannot
        // be written the caller must end the run rather than keep reporting
        // success on a turn that would not survive a restart.
        let mut turn = control.turn.lock();
        turn.history_json = serde_json::to_string(messages).unwrap_or_else(|_| "[]".into());
        turn.loop_index = loop_index as u32;
        turn.checkpoint = checkpoint.into();
        turn.resume_safe = resume_safe;
        turn.updated_at = self
            .clock
            .now()
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let snapshot = turn.clone();
        drop(turn);
        self.workspace
            .write_agent_turn(&snapshot)
            .map_err(|error| format!("failed to persist Agent checkpoint '{checkpoint}': {error}"))
    }

    fn control(&self, run_id: &str) -> Result<Arc<RunControl>, AgentError> {
        // Returning the Arc keeps the control alive for the duration of a public
        // operation even if a terminal path removes the active-run marker.
        // Controls are looked up under the runtime lock so every public command
        // observes the same run identity and receives a stable not-found error.
        self.state
            .lock()
            .runs
            .get(run_id)
            .cloned()
            .ok_or(AgentError::RunNotFound)
    }
}

fn json_payload(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    let candidate = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(candidate).ok().or_else(|| {
        let start = candidate.find('{')?;
        let end = candidate.rfind('}')?;
        serde_json::from_str(&candidate[start..=end]).ok()
    })
}

fn tool_error_result(code: &str, message: &str) -> String {
    json!({
        "ok": false,
        "error": {"code": code, "message": message}
    })
    .to_string()
}

fn validate_capability_result(mode: AgentMode, text: &str) -> Result<(), String> {
    match mode {
        AgentMode::QuestionLab => {
            let value = json_payload(text).ok_or_else(|| {
                "Question Lab must return a JSON object with a questions array".to_owned()
            })?;
            let questions = value
                .get("questions")
                .and_then(Value::as_array)
                .ok_or_else(|| "Question Lab result is missing questions".to_owned())?;
            if questions.is_empty() || questions.len() > 100 {
                return Err("Question Lab must contain between 1 and 100 questions".into());
            }
            for question in questions {
                let object = question
                    .as_object()
                    .ok_or_else(|| "each Question Lab item must be an object".to_owned())?;
                let prompt = object
                    .get("prompt")
                    .or_else(|| object.get("question"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "each question needs a non-empty prompt".to_owned())?;
                if prompt.len() > 20_000 {
                    return Err("question prompt exceeds the 20,000 character limit".into());
                }
                let options = object
                    .get("options")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "each question needs an options array".to_owned())?;
                if !(2..=8).contains(&options.len()) {
                    return Err("question options must contain between 2 and 8 items".into());
                }
                if object.get("answer").is_none() && object.get("correctAnswer").is_none() {
                    return Err("each question needs an answer".into());
                }
            }
            Ok(())
        }
        AgentMode::Visualize => {
            if let Some(value) = json_payload(text) {
                let render_type = value
                    .get("renderType")
                    .or_else(|| value.get("render_type"))
                    .and_then(Value::as_str)
                    .unwrap_or("markdown")
                    .to_ascii_lowercase();
                if !matches!(
                    render_type.as_str(),
                    "mermaid" | "svg" | "chart" | "chartjs" | "html" | "markdown"
                ) {
                    return Err("Visualize returned an unsupported render type".into());
                }
                let content = value
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                reject_active_visual_content(content)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn reject_active_visual_content(content: &str) -> Result<(), String> {
    let lowered = content.to_ascii_lowercase();
    let compact = lowered
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if ["<script", "javascript:", "onload=", "onclick=", "onerror="]
        .iter()
        .any(|needle| compact.contains(needle))
    {
        return Err("Visualize content contains executable markup".into());
    }
    Ok(())
}

// EventFields keeps construction sites readable while preserving one stable
// AgentEvent wire shape.  `None` means the field is not applicable to that
// event kind; it does not mean an empty string or an unknown event.
#[derive(Default)]
struct EventFields {
    status: Option<RunStatus>,
    text: Option<String>,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    permission: Option<PermissionLevel>,
    preview: Option<String>,
    confirmation_id: Option<String>,
    payload_json: Option<String>,
    mode: Option<AgentMode>,
    stage: Option<String>,
    progress: Option<f64>,
}

fn stage_for(mode: AgentMode, loop_index: usize) -> String {
    // Stage lookup is derived from the manifest on every request so the label
    // cannot drift from the capability list sent to the model or shown in UI.
    // Stage labels are presentation metadata.  Clamping the index makes a
    // longer mode budget safe even if its manifest has fewer named stages.
    let stages = capability_manifests()
        .into_iter()
        .find(|manifest| manifest.mode == mode)
        .map(|manifest| manifest.stages)
        .unwrap_or_else(|| vec!["responding".into()]);
    stages
        .get(loop_index.min(stages.len().saturating_sub(1)))
        .cloned()
        .unwrap_or_else(|| "responding".into())
}

fn tool_is_enabled(_mode: AgentMode, _name: &str) -> bool {
    // Visibility is broad by design; authorization remains narrow at execution.
    // This keeps discovery and permission separate in every Agent mode.
    // The complete catalog is intentionally visible to the model.  Permission
    // is enforced by `prepare` and the confirmation path, not by hiding tools
    // per mode, which would make valid workflows impossible to discover.
    // OpenCode exposes one coherent tool catalog to the model and applies
    // permission at execution time. Mode-specific allow-lists made the
    // default Chat mode unable to see the Runner, search, and task tools at
    // all, so the model could not choose them even when they were appropriate.
    // The host still validates every call and fails closed inside each tool.
    true
}

// Runtime tests use the mock provider to drive real confirmation, cancellation,
// cursor, and terminal transitions.  This keeps lifecycle guarantees testable
// without network access or a UI process, while Workspace writes still happen
// only through the production tool dispatcher.
#[cfg(test)]
mod tests {
    // Tests exercise protocol guarantees rather than implementation details:
    // exclusive cursors, consent-before-write, structured denial, and prompt
    // cancellation are the contracts that clients rely on across platforms.
    use super::*;
    use async_trait::async_trait;
    use studypulse_model_client::{
        MockModelClient, ModelClient, ModelError, ModelResponse, ModelToolCall,
    };

    #[derive(Debug)]
    struct FixedClock;

    impl Clock for FixedClock {
        // Deterministic timestamps make event ordering assertions independent of
        // wall-clock scheduling while leaving production Clock behavior intact.
        fn now(&self) -> DateTime<Utc> {
            DateTime::parse_from_rfc3339("2026-07-30T09:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        }
    }

    fn runtime() -> (tempfile::TempDir, Workspace, Arc<AgentRuntime>) {
        // Each test owns a fresh Workspace and MockModelClient, so assertions on
        // writes and persisted run logs cannot depend on a previous test.
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runtime = AgentRuntime::with_clock(
            workspace.clone(),
            Arc::new(MockModelClient),
            Arc::new(FixedClock),
        );
        (temp, workspace, runtime)
    }

    fn wait_for_terminal(runtime: &AgentRuntime, run_id: &str) -> Vec<AgentEvent> {
        // The helper advances from the last event's sequence, not from the
        // number of events returned in a batch.  This mirrors the production
        // UI poller and protects against dropped/duplicated batch sizes.
        let mut cursor = 0;
        let mut all = Vec::new();
        loop {
            let events = runtime.wait_for_events(run_id, cursor, 500).unwrap();
            if let Some(last) = events.last() {
                cursor = last.sequence;
            }
            let terminal = events.iter().any(|event| {
                matches!(
                    event.kind,
                    AgentEventKind::Failed | AgentEventKind::Cancelled | AgentEventKind::Completed
                )
            });
            all.extend(events);
            if terminal {
                return all;
            }
        }
    }

    #[derive(Debug)]
    struct PlainAnswerModel;

    #[async_trait]
    impl ModelClient for PlainAnswerModel {
        async fn complete(
            &self,
            _request: ModelRequest,
            _on_text_delta: ModelTextDeltaHandler,
        ) -> Result<ModelResponse, ModelError> {
            Ok(ModelResponse {
                text_deltas: vec!["All done".into()],
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }

    #[derive(Debug, Default)]
    struct AskUserModel {
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl ModelClient for AskUserModel {
        async fn complete(
            &self,
            _request: ModelRequest,
            _on_text_delta: ModelTextDeltaHandler,
        ) -> Result<ModelResponse, ModelError> {
            let mut calls = self.calls.lock();
            if *calls == 0 {
                *calls += 1;
                Ok(ModelResponse {
                    text_deltas: Vec::new(),
                    tool_calls: vec![ModelToolCall {
                        id: "ask-1".into(),
                        name: "ask_user".into(),
                        arguments: json!({"prompt": "Continue with the plan?"}),
                    }],
                    usage: None,
                })
            } else {
                Ok(ModelResponse {
                    text_deltas: vec!["Resumed answer".into()],
                    tool_calls: Vec::new(),
                    usage: None,
                })
            }
        }
    }

    // Replacing the turns directory with a regular file makes every turn
    // write fail with an IO error, which simulates disk-full and broken-path
    // conditions without reaching into Workspace internals.
    fn break_turn_storage(workspace: &Workspace) -> std::path::PathBuf {
        let turns = workspace.root().join("Agent/turns");
        std::fs::remove_dir_all(&turns).unwrap();
        std::fs::write(&turns, b"not a directory").unwrap();
        turns
    }

    fn restore_turn_storage(turns: &std::path::Path) {
        std::fs::remove_file(turns).unwrap();
        std::fs::create_dir(turns).unwrap();
    }

    #[test]
    fn start_agent_fails_when_initial_checkpoint_cannot_be_persisted() {
        // A start whose initial checkpoint never reaches the disk must fail
        // the start outright instead of reporting a run that cannot be
        // recovered after a crash.
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runtime = AgentRuntime::with_clock(
            workspace.clone(),
            Arc::new(PlainAnswerModel),
            Arc::new(FixedClock),
        );
        let turns = break_turn_storage(&workspace);
        let error = runtime
            .start_agent("Recover algebra".into(), Vec::new(), Vec::new())
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to persist Agent checkpoint")
        );
        // The failed start must not wedge the single-active-run guard.
        restore_turn_storage(&turns);
        let run_id = runtime
            .start_agent("Recover algebra".into(), Vec::new(), Vec::new())
            .unwrap();
        wait_for_terminal(&runtime, &run_id);
    }

    #[test]
    fn checkpoint_failure_fails_the_run_instead_of_reporting_success() {
        // While the run is parked on ask_user the test breaks turn storage, so
        // the next checkpoint write fails: the run must surface a Failed event
        // rather than continue as if the recovery state had been saved.
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runtime = AgentRuntime::with_clock(
            workspace.clone(),
            Arc::new(AskUserModel::default()),
            Arc::new(FixedClock),
        );
        let run_id = runtime
            .start_agent("Plan the session".into(), Vec::new(), Vec::new())
            .unwrap();
        let mut cursor = 0;
        let input_id = loop {
            let events = runtime.wait_for_events(&run_id, cursor, 500).unwrap();
            if let Some(last) = events.last() {
                cursor = last.sequence;
            }
            if let Some(event) = events
                .iter()
                .find(|event| event.kind == AgentEventKind::InputRequired)
            {
                break event.confirmation_id.clone().unwrap();
            }
        };
        let turns = break_turn_storage(&workspace);
        runtime
            .submit_input(&run_id, &input_id, json!("yes").to_string())
            .unwrap();
        let events = wait_for_terminal(&runtime, &run_id);
        let failed = events
            .iter()
            .find(|event| event.kind == AgentEventKind::Failed)
            .expect("checkpoint failure must produce a Failed event");
        assert!(
            failed
                .text
                .as_deref()
                .unwrap_or_default()
                .contains("failed to persist Agent checkpoint")
        );
        assert_eq!(runtime.run_status(&run_id).unwrap(), RunStatus::Failed);
        // The terminal path released the slot, so a later start still works.
        restore_turn_storage(&turns);
        let run_id = runtime
            .start_agent("Plan the session".into(), Vec::new(), Vec::new())
            .unwrap();
        wait_for_terminal(&runtime, &run_id);
    }

    #[test]
    fn run_log_failure_stays_best_effort_and_completes_the_run() {
        // The JSONL run log is an audit/replay aid while the in-memory event
        // stream drives the run, so a broken log must not fail an otherwise
        // healthy run; the failure is only logged.
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runs = workspace.root().join("Agent/runs");
        std::fs::remove_dir_all(&runs).unwrap();
        std::fs::write(&runs, b"not a directory").unwrap();
        let runtime =
            AgentRuntime::with_clock(workspace, Arc::new(PlainAnswerModel), Arc::new(FixedClock));
        let run_id = runtime
            .start_agent("Summarize progress".into(), Vec::new(), Vec::new())
            .unwrap();
        let events = wait_for_terminal(&runtime, &run_id);
        assert!(
            events
                .iter()
                .any(|event| event.kind == AgentEventKind::Completed)
        );
    }

    #[test]
    fn mock_agent_waits_for_confirmation_before_writing() {
        // A write tool must be visible as a confirmation before the Workspace
        // changes.  The test observes the pause, checks storage, then allows it.
        let (_temp, workspace, runtime) = runtime();
        let run_id = runtime
            .start_agent("Review algebra".into(), Vec::new(), Vec::new())
            .unwrap();
        let mut cursor = 0;
        let confirmation_id = loop {
            let events = runtime.wait_for_events(&run_id, cursor, 500).unwrap();
            if let Some(last) = events.last() {
                cursor = last.sequence;
            }
            if let Some(id) = events
                .iter()
                .find_map(|event| event.confirmation_id.clone())
            {
                break id;
            }
        };
        assert!(workspace.read_tasks().unwrap().is_empty());
        runtime
            .submit_confirmation(&run_id, &confirmation_id, ConfirmationDecision::Allow)
            .unwrap();
        loop {
            let events = runtime.wait_for_events(&run_id, cursor, 500).unwrap();
            if let Some(last) = events.last() {
                cursor = last.sequence;
            }
            if events
                .iter()
                .any(|event| event.kind == AgentEventKind::Completed)
            {
                break;
            }
        }
        let tasks = workspace.read_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Review algebra");
        assert!(
            workspace
                .root()
                .join(format!("Agent/runs/{run_id}.jsonl"))
                .is_file()
        );
    }

    #[test]
    fn capability_manifests_cover_all_modes() {
        // Manifest coverage protects the public mode list and the stage/loop
        // metadata consumed by both Agent requests and UI progress views.
        let manifests = capability_manifests();
        assert_eq!(manifests.len(), 9);
        assert_eq!(manifests[1].mode, AgentMode::DeepSolve);
        assert_eq!(manifests[5].mode, AgentMode::Visualize);
        assert_eq!(manifests[6].mode, AgentMode::Coach);
        assert_eq!(manifests[8].mode, AgentMode::ReversePlanner);
        assert!(manifests.iter().all(|manifest| !manifest.stages.is_empty()));
        assert_eq!(manifests[3].result_kind, "research_report");
        assert!(manifests[4].tools_used.contains(&"save_artifact".into()));
        assert!(serde_json::from_str::<Value>(&manifests[3].request_schema_json).is_ok());
    }

    #[derive(Debug, Default)]
    struct BatchReadModel {
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl ModelClient for BatchReadModel {
        async fn complete(
            &self,
            _request: ModelRequest,
            _on_text_delta: ModelTextDeltaHandler,
        ) -> Result<ModelResponse, ModelError> {
            let mut calls = self.calls.lock();
            if *calls == 0 {
                *calls += 1;
                Ok(ModelResponse {
                    text_deltas: Vec::new(),
                    tool_calls: vec![
                        ModelToolCall {
                            id: "read-files".into(),
                            name: "list_workspace_files".into(),
                            arguments: json!({}),
                        },
                        ModelToolCall {
                            id: "read-tasks".into(),
                            name: "get_tasks".into(),
                            arguments: json!({}),
                        },
                    ],
                    usage: None,
                })
            } else {
                Ok(ModelResponse {
                    text_deltas: vec!["Parallel reads are complete".into()],
                    tool_calls: Vec::new(),
                    usage: None,
                })
            }
        }
    }

    #[test]
    fn parallel_read_batch_returns_observation_and_preserves_tool_results() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runtime = AgentRuntime::with_clock(
            workspace,
            Arc::new(BatchReadModel::default()),
            Arc::new(FixedClock),
        );
        let run_id = runtime
            .start_agent("Inspect the workspace".into(), Vec::new(), Vec::new())
            .unwrap();
        let events = wait_for_terminal(&runtime, &run_id);
        assert!(
            events
                .iter()
                .any(|event| event.kind == AgentEventKind::Observation)
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == AgentEventKind::ToolCompleted)
                .count(),
            2
        );
        assert!(
            events
                .iter()
                .any(|event| event.kind == AgentEventKind::Result)
        );
    }

    #[derive(Debug)]
    struct UsageModel;

    #[async_trait]
    impl ModelClient for UsageModel {
        async fn complete(
            &self,
            _request: ModelRequest,
            _on_text_delta: ModelTextDeltaHandler,
        ) -> Result<ModelResponse, ModelError> {
            Ok(ModelResponse {
                text_deltas: vec!["Measured answer".into()],
                tool_calls: Vec::new(),
                usage: Some(ModelUsage {
                    prompt_tokens: 11,
                    completion_tokens: 7,
                    total_tokens: 18,
                    estimated: false,
                }),
            })
        }
    }

    #[test]
    fn usage_and_result_are_persisted_as_structured_events() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runtime = AgentRuntime::with_clock(
            workspace.clone(),
            Arc::new(UsageModel),
            Arc::new(FixedClock),
        );
        let run_id = runtime
            .start_turn(TurnRequest {
                mode: AgentMode::Visualize,
                goal: "Explain the cycle".into(),
                source_paths: Vec::new(),
                history: Vec::new(),
                notebook_id: None,
                config_json: None,
            })
            .unwrap();
        let events = wait_for_terminal(&runtime, &run_id);
        let usage = events
            .iter()
            .find(|event| event.kind == AgentEventKind::Usage)
            .and_then(|event| event.payload_json.as_deref())
            .and_then(|payload| serde_json::from_str::<UsageSummary>(payload).ok())
            .unwrap();
        assert_eq!(usage.total_tokens, 18);
        let result = runtime.turn_result_for_run(&run_id).unwrap();
        assert_eq!(result.result_kind, "visualization");
        assert_eq!(result.text, "Measured answer");
        assert_eq!(
            workspace.read_agent_turn(&run_id).unwrap().status,
            "completed"
        );
    }

    #[derive(Debug)]
    struct QuestionModel;

    #[async_trait]
    impl ModelClient for QuestionModel {
        async fn complete(
            &self,
            _request: ModelRequest,
            _on_text_delta: ModelTextDeltaHandler,
        ) -> Result<ModelResponse, ModelError> {
            Ok(ModelResponse {
                text_deltas: vec![
                    json!({
                        "questions": [{
                            "prompt": "2 + 2 = ?",
                            "options": ["3", "4"],
                            "answer": "4",
                            "explanation": "Addition"
                        }]
                    })
                    .to_string(),
                ],
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }

    #[test]
    fn question_lab_requires_and_persists_a_valid_question_set() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runtime =
            AgentRuntime::with_clock(workspace, Arc::new(QuestionModel), Arc::new(FixedClock));
        let run_id = runtime
            .start_agent_with_mode(
                AgentMode::QuestionLab,
                "Generate one arithmetic question".into(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
        let events = wait_for_terminal(&runtime, &run_id);
        assert!(
            events
                .iter()
                .any(|event| event.kind == AgentEventKind::Completed)
        );
        assert!(
            runtime
                .turn_result_for_run(&run_id)
                .unwrap()
                .output_json
                .is_some()
        );
    }

    #[test]
    fn persisted_events_are_replayable_after_runtime_recreation() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let first = AgentRuntime::with_clock(
            workspace.clone(),
            Arc::new(RecordingModel::default()),
            Arc::new(FixedClock),
        );
        let run_id = first
            .start_agent("Persist this run".into(), Vec::new(), Vec::new())
            .unwrap();
        let original = wait_for_terminal(&first, &run_id);
        drop(first);
        let reopened = AgentRuntime::with_clock(
            workspace,
            Arc::new(RecordingModel::default()),
            Arc::new(FixedClock),
        );
        let replayed = reopened.wait_for_events(&run_id, 0, 0).unwrap();
        assert_eq!(replayed.len(), original.len());
        assert!(
            replayed
                .iter()
                .any(|event| event.kind == AgentEventKind::Completed)
        );
    }

    #[test]
    fn cancellation_interrupts_confirmation_wait() {
        // Cancellation must wake a synchronous consent waiter and leave no
        // pending write behind; a terminal Cancelled event is the observable
        // completion signal for the client.
        let (_temp, workspace, runtime) = runtime();
        let run_id = runtime
            .start_agent("Cancel me".into(), Vec::new(), Vec::new())
            .unwrap();
        let mut cursor = 0;
        loop {
            let events = runtime.wait_for_events(&run_id, cursor, 500).unwrap();
            if let Some(last) = events.last() {
                cursor = last.sequence;
            }
            if events
                .iter()
                .any(|event| event.kind == AgentEventKind::ConfirmationRequired)
            {
                break;
            }
        }
        runtime.cancel_agent(&run_id).unwrap();
        loop {
            let events = runtime.wait_for_events(&run_id, cursor, 500).unwrap();
            if let Some(last) = events.last() {
                cursor = last.sequence;
            }
            if events
                .iter()
                .any(|event| event.kind == AgentEventKind::Cancelled)
            {
                break;
            }
        }
        assert!(workspace.read_tasks().unwrap().is_empty());
    }

    #[test]
    fn denial_returns_structured_result_without_writing() {
        // Denial is not a runtime crash and not an empty result.  The model gets
        // the explicit user_denied code while Workspace remains unchanged.
        let (_temp, workspace, runtime) = runtime();
        let run_id = runtime
            .start_agent("Review geometry".into(), Vec::new(), Vec::new())
            .unwrap();
        let mut cursor = 0;
        let confirmation_id = loop {
            let events = runtime.wait_for_events(&run_id, cursor, 500).unwrap();
            if let Some(last) = events.last() {
                cursor = last.sequence;
            }
            if let Some(id) = events
                .iter()
                .find_map(|event| event.confirmation_id.clone())
            {
                break id;
            }
        };
        runtime
            .submit_confirmation(&run_id, &confirmation_id, ConfirmationDecision::Deny)
            .unwrap();
        let events = wait_for_terminal(&runtime, &run_id);
        assert!(workspace.read_tasks().unwrap().is_empty());
        assert!(events.iter().any(|event| {
            event
                .payload_json
                .as_deref()
                .is_some_and(|payload| payload.contains("\"code\":\"user_denied\""))
        }));
    }

    #[derive(Debug, Default)]
    struct RecordingModel {
        // Recording the first request makes history ordering and permission
        // metadata observable without performing a network request.
        requests: Mutex<Vec<ModelRequest>>,
    }

    #[async_trait]
    impl ModelClient for RecordingModel {
        // This model returns a final answer immediately, isolating request-shape
        // assertions from tool-loop behavior.
        async fn complete(
            &self,
            request: ModelRequest,
            _on_text_delta: ModelTextDeltaHandler,
        ) -> Result<ModelResponse, ModelError> {
            self.requests.lock().push(request);
            Ok(ModelResponse {
                text_deltas: vec!["Next answer".into()],
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }

    #[test]
    fn sends_prior_conversation_before_the_new_goal() {
        // Conversation history is converted before the new goal and the model
        // sees the complete tool catalog, including the execute permission hint.
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let model = Arc::new(RecordingModel::default());
        let runtime = AgentRuntime::with_clock(workspace, model.clone(), Arc::new(FixedClock));
        let history = vec![
            ConversationMessage {
                role: ConversationRole::User,
                content: "First question".into(),
            },
            ConversationMessage {
                role: ConversationRole::Assistant,
                content: "First answer".into(),
            },
        ];

        let run_id = runtime
            .start_agent("Follow up".into(), Vec::new(), history)
            .unwrap();
        wait_for_terminal(&runtime, &run_id);

        assert_eq!(
            model.requests.lock()[0].messages,
            vec![
                ChatMessage::User {
                    content: "First question".into()
                },
                ChatMessage::Assistant {
                    content: "First answer".into()
                },
                ChatMessage::User {
                    content: "Follow up".into()
                },
            ]
        );
        assert!(
            model.requests.lock()[0]
                .tools
                .iter()
                .any(|tool| tool.name == "code_execution")
        );
        assert_eq!(
            model.requests.lock()[0]
                .tools
                .iter()
                .find(|tool| tool.name == "code_execution")
                .and_then(|tool| tool.permission.as_deref()),
            Some("execute")
        );
    }

    #[test]
    fn feature_without_explicit_sources_exposes_no_workspace_tools() {
        // Specialized callers receive only their structured input by default;
        // an empty source selection must not inherit the general Agent meaning
        // of “search the whole Workspace”.
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let model = Arc::new(RecordingModel::default());
        let runtime = AgentRuntime::with_clock(workspace, model.clone(), Arc::new(FixedClock));

        let run_id = runtime
            .start_feature_with_mode(
                AgentMode::Coach,
                "Feature input".into(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
        wait_for_terminal(&runtime, &run_id);

        assert!(model.requests.lock()[0].tools.is_empty());
    }

    #[derive(Debug)]
    struct LoopingModel;

    // A deterministic repeated call exercises the runtime loop budget and its
    // terminal error event without creating a Workspace write.
    #[async_trait]
    impl ModelClient for LoopingModel {
        async fn complete(
            &self,
            _request: ModelRequest,
            _on_text_delta: ModelTextDeltaHandler,
        ) -> Result<ModelResponse, ModelError> {
            Ok(ModelResponse {
                text_deltas: Vec::new(),
                tool_calls: vec![ModelToolCall {
                    id: Uuid::new_v4().to_string(),
                    name: "get_tasks".into(),
                    arguments: json!({}),
                }],
                usage: None,
            })
        }
    }

    #[test]
    fn fails_after_maximum_model_loops() {
        // A model that always requests a read tool must still terminate at the
        // mode's loop budget rather than keeping the desktop run busy forever.
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runtime =
            AgentRuntime::with_clock(workspace, Arc::new(LoopingModel), Arc::new(FixedClock));
        let run_id = runtime
            .start_agent("Loop".into(), Vec::new(), Vec::new())
            .unwrap();
        let events = wait_for_terminal(&runtime, &run_id);
        let requested = events
            .iter()
            .filter(|event| event.kind == AgentEventKind::ToolRequested)
            .count();
        assert_eq!(requested, MAX_AGENT_LOOPS);
        assert!(events.iter().any(|event| {
            event.kind == AgentEventKind::Failed
                && event
                    .text
                    .as_deref()
                    .is_some_and(|text| text.contains("maximum of 8"))
        }));
    }

    #[derive(Debug)]
    struct BlockingModel;

    // A permanently pending future verifies that cancellation is a transport
    // concern handled by AgentRuntime rather than by provider cooperation.
    #[async_trait]
    impl ModelClient for BlockingModel {
        async fn complete(
            &self,
            _request: ModelRequest,
            _on_text_delta: ModelTextDeltaHandler,
        ) -> Result<ModelResponse, ModelError> {
            std::future::pending().await
        }
    }

    #[test]
    fn cancellation_interrupts_model_wait() {
        // The model future can remain pending indefinitely; biased select plus
        // Notify must still produce a terminal cancellation event promptly.
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let runtime =
            AgentRuntime::with_clock(workspace, Arc::new(BlockingModel), Arc::new(FixedClock));
        let run_id = runtime
            .start_agent("Wait forever".into(), Vec::new(), Vec::new())
            .unwrap();
        loop {
            let events = runtime.wait_for_events(&run_id, 0, 500).unwrap();
            if events
                .iter()
                .any(|event| event.kind == AgentEventKind::StatusChanged)
            {
                break;
            }
        }
        runtime.cancel_agent(&run_id).unwrap();
        let events = wait_for_terminal(&runtime, &run_id);
        assert!(
            events
                .iter()
                .any(|event| event.kind == AgentEventKind::Cancelled)
        );
    }
}
