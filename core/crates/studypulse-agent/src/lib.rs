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

pub const MAX_AGENT_LOOPS: usize = 8;

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

struct RuntimeState {
    active_run: Option<String>,
    runs: HashMap<String, Arc<RunControl>>,
}

pub struct AgentRuntime {
    workspace: Workspace,
    tools: ToolRegistry,
    model: Arc<dyn ModelClient>,
    clock: Arc<dyn Clock>,
    state: Mutex<RuntimeState>,
}

impl AgentRuntime {
    pub fn new(workspace: Workspace, model: Arc<dyn ModelClient>) -> Arc<Self> {
        Self::with_clock(workspace, model, Arc::new(SystemClock))
    }

    pub fn with_clock(
        workspace: Workspace,
        model: Arc<dyn ModelClient>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
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
        self.start_agent_with_mode(AgentMode::Chat, goal, source_paths, history)
    }

    pub fn start_agent_with_mode(
        self: &Arc<Self>,
        mode: AgentMode,
        goal: String,
        source_paths: Vec<String>,
        history: Vec<ConversationMessage>,
    ) -> Result<String, AgentError> {
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
        let run_id = Uuid::new_v4().to_string();
        let control = Arc::new(RunControl::new(run_id.clone()));
        {
            let mut state = self.state.lock();
            if state.active_run.is_some() {
                return Err(AgentError::Busy);
            }
            state.active_run = Some(run_id.clone());
            state.runs.insert(run_id.clone(), Arc::clone(&control));
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
        messages.push(ChatMessage::User { content: goal });
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
        for loop_index in 0..max_loops {
            if self.finish_if_cancelled(&control) {
                return;
            }
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
            let response = match tokio::select! {
                biased;
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
                    self.finish_with_error(&control, error.to_string());
                    return;
                }
            };

            let streamed_text = streamed_text.lock().clone();
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

            for call in response.tool_calls {
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

                let result = self
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
        self.state
            .lock()
            .runs
            .get(run_id)
            .cloned()
            .ok_or(AgentError::RunNotFound)
    }
}

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
    // OpenCode exposes one coherent tool catalog to the model and applies
    // permission at execution time. Mode-specific allow-lists made the
    // default Chat mode unable to see the Runner, search, and task tools at
    // all, so the model could not choose them even when they were appropriate.
    // The host still validates every call and fails closed inside each tool.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use studypulse_model_client::{
        MockModelClient, ModelClient, ModelError, ModelResponse, ModelToolCall,
    };

    #[derive(Debug)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            DateTime::parse_from_rfc3339("2026-07-30T09:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        }
    }

    fn runtime() -> (tempfile::TempDir, Workspace, Arc<AgentRuntime>) {
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
        requests: Mutex<Vec<ModelRequest>>,
    }

    #[async_trait]
    impl ModelClient for RecordingModel {
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
