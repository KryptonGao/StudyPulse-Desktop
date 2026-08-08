use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
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
use serde_json::json;
use studypulse_model_client::{
    ChatMessage, ModelClient, ModelRequest, ModelTextDeltaHandler, ModelToolDefinition,
};
use studypulse_tools::{PermissionLevel, ToolRegistry};
use studypulse_workspace::Workspace;
use thiserror::Error;
use uuid::Uuid;

// The Agent runtime is deliberately kept above the model client and tool
// registry: it owns the run lifecycle, while providers only return text and
// proposed calls, and tools only execute after the host has validated them.
// This separation lets every front end observe the same event protocol and
// keeps confirmation, cancellation, and Workspace writes in one place.

pub const MAX_AGENT_LOOPS: usize = 8;

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
    CapabilityManifest {
        mode,
        title: title.into(),
        description: description.into(),
        stages: stages.iter().map(|stage| (*stage).into()).collect(),
        max_loops,
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
}

// Sequence numbers start at one so zero is an unambiguous initial cursor for a
// newly opened UI.  Every event is assigned by this control object before it
// is made visible to either the in-memory buffer or the persisted JSONL log.
impl RunControl {
    fn new(run_id: String) -> Self {
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
    state: Mutex<RuntimeState>,
}

impl AgentRuntime {
    // Production construction uses the real clock; tests use `with_clock` to
    // make timestamps stable without adding a second runtime implementation.
    pub fn new(workspace: Workspace, model: Arc<dyn ModelClient>) -> Arc<Self> {
        Self::with_clock(workspace, model, Arc::new(SystemClock))
    }

    pub fn with_clock(
        workspace: Workspace,
        model: Arc<dyn ModelClient>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        // The registry and state are created once per runtime so a run cannot
        // accidentally use a different tool catalog halfway through a loop.
        Arc::new(Self {
            workspace,
            tools: ToolRegistry::default(),
            model,
            clock,
            state: Mutex::new(RuntimeState {
                active_run: None,
                runs: HashMap::new(),
            }),
        })
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
        let source_paths = self
            .workspace
            .list_selected_library_files(&source_paths)
            .map_err(|error| AgentError::Runtime(error.to_string()))?
            .into_iter()
            .map(|entry| entry.relative_path)
            .collect::<Vec<_>>();
        // Source resolution returns selected relative paths from Workspace.  The
        // Agent carries this list into every read invocation instead of trusting
        // a path proposed by the model during a later loop.
        let run_id = Uuid::new_v4().to_string();
        let control = Arc::new(RunControl::new(run_id.clone()));
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
                        history,
                    )),
                    Err(error) => {
                        runtime.finish_with_error(&control, error.to_string());
                    }
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
        let control = self.control(run_id)?;
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

    async fn run_agent(
        self: Arc<Self>,
        control: Arc<RunControl>,
        mode: AgentMode,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<ConversationMessage>,
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
                    capability_manifests()
                        .into_iter()
                        .find(|manifest| manifest.mode == mode)
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
        let mut messages = history
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
        // Tool messages are appended only after prepare/consent/execute has
        // produced a result, keeping the transcript truthful for the next turn.
        messages.push(ChatMessage::User { content: goal });
        // Permission is serialized as a hint for weaker models, not as an
        // authorization grant.  The host repeats the decision at prepare time.
        let definitions = self
            .tools
            .definitions()
            .into_iter()
            .filter(|definition| tool_is_enabled(mode, &definition.name))
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
            .collect::<Vec<_>>();

        let max_loops = capability_manifests()
            .into_iter()
            .find(|manifest| manifest.mode == mode)
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
                mode: Some(
                    serde_json::to_string(&mode)
                        .unwrap_or_else(|_| "chat".into())
                        .trim_matches('"')
                        .to_owned(),
                ),
                stages: capability_manifests()
                    .into_iter()
                    .find(|manifest| manifest.mode == mode)
                    .map(|manifest| manifest.stages)
                    .unwrap_or_default(),
            };
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

            let streamed_text = streamed_text.lock().clone();
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

            if response.tool_calls.is_empty() {
                // No tool call means the model turn is the final answer.  The
                // terminal event is emitted only after the final stage marker.
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

            // Calls are handled sequentially even when the model returns a
            // batch.  This preserves event order and ensures a later call in
            // the batch cannot observe a write that the user has not yet
            // approved or a cancellation that has not yet been checked.
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
                            let result =
                                json!({"ok": false, "error": error.to_string()}).to_string();
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
                            json!({"ok": false, "error": "user did not answer"}).to_string()
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
                    continue;
                }

                // The tool result is always converted to a Tool message.  The
                // model therefore sees success and failure through one
                // protocol, instead of through host-only side channels that
                // would make retries or explanations inconsistent.
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
                    .unwrap_or_else(|error| {
                        json!({"ok": false, "error": error.to_string()}).to_string()
                    });
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
            }
        }
        self.finish_with_error(
            &control,
            format!("Agent exceeded the maximum of {max_loops} model loops"),
        );
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
        // Terminal cleanup is centralized: update status, append the terminal
        // event, release the single-active slot, and wake any event/consent
        // waiter that may still be observing the run.
        *control.status.lock() = status;
        self.emit(
            control,
            kind,
            EventFields {
                status: Some(status),
                text,
                ..EventFields::default()
            },
        );
        let mut state = self.state.lock();
        if state.active_run.as_deref() == Some(&control.run_id) {
            state.active_run = None;
        }
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
        // inspection of a run without changing its live state machine.
        let path = self
            .workspace
            .root()
            .join("Agent/runs")
            .join(format!("{}.jsonl", event.run_id));
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
            && serde_json::to_writer(&mut file, event).is_ok()
        {
            let _ = file.write_all(b"\n");
            let _ = file.flush();
        }
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
