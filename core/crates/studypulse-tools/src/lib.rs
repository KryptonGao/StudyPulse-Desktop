use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use parking_lot::Mutex;
use quick_xml::de::from_str as from_xml;
use reqwest::blocking::Client;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use studypulse_workspace::{TaskItem, TaskType, Workspace};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    Read,
    Write,
    Destructive,
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub permission: PermissionLevel,
}

#[derive(Debug, Clone)]
pub struct PreparedTool {
    pub call_id: String,
    pub name: String,
    pub permission: PermissionLevel,
    pub preview: String,
    invocation: Invocation,
}

#[derive(Debug, Clone)]
enum Invocation {
    ListWorkspaceFiles,
    SearchWorkspace(SearchWorkspaceArgs),
    ReadSource(ReadSourceArgs),
    ReadMemory(ReadMemoryArgs),
    WriteMemory(WriteMemoryArgs),
    WebSearch(WebSearchArgs),
    PaperSearch(PaperSearchArgs),
    CodeExecution(CodeExecutionArgs),
    SaveArtifact(SaveArtifactArgs),
    AskUser(AskUserArgs),
    GetTasks,
    CreateTask(CreateTaskArgs),
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchWorkspaceArgs {
    query: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadSourceArgs {
    path: String,
    #[serde(default = "default_read_chars")]
    max_chars: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadMemoryArgs {
    #[serde(default = "default_memory_scope")]
    scope: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WriteMemoryArgs {
    #[serde(default = "default_memory_scope")]
    scope: String,
    key: String,
    value: Value,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WebSearchArgs {
    query: String,
    #[serde(default = "default_search_results")]
    max_results: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PaperSearchArgs {
    query: String,
    #[serde(default = "default_paper_results")]
    max_results: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeExecutionArgs {
    language: String,
    code: String,
    #[serde(default)]
    stdin: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SaveArtifactArgs {
    artifact_id: String,
    extension: String,
    content: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AskUserArgs {
    prompt: String,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateTaskArgs {
    title: String,
    #[serde(default)]
    task_type: Option<TaskTypeArg>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    reminder_date: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    importance: Option<u8>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum TaskTypeArg {
    Homework,
    Reading,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("invalid arguments for {tool}: {detail}")]
    InvalidArguments { tool: String, detail: String },
    #[error(transparent)]
    Workspace(#[from] studypulse_workspace::WorkspaceError),
    #[error("tool execution failed: {0}")]
    Execution(String),
}

const DEFAULT_RUNNER_URL: &str = "http://127.0.0.1:45891";
const RUNNER_IMAGE: &str = "studypulse-runner";
const RUNNER_HEALTH_TIMEOUT: StdDuration = StdDuration::from_secs(15);
const MAX_LOCAL_EXECUTION_TIMEOUT_SECONDS: u64 = 30;
const MAX_LOCAL_EXECUTION_STDIN_BYTES: usize = 64 * 1024;
const MAX_LOCAL_EXECUTION_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Default)]
pub struct ToolRegistry {
    runner: RunnerManager,
}

#[derive(Clone)]
struct RunnerManager {
    inner: Arc<RunnerManagerInner>,
}

struct RunnerManagerInner {
    state: Mutex<RunnerState>,
}

#[derive(Default)]
struct RunnerState {
    managed: Option<ManagedRunner>,
}

struct ManagedRunner {
    child: Child,
    token: String,
    base_url: String,
}

struct RunnerConnection {
    base_url: String,
    token: String,
    health: RunnerHealth,
}

impl Default for RunnerManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(RunnerManagerInner {
                state: Mutex::new(RunnerState::default()),
            }),
        }
    }
}

impl RunnerManager {
    fn ensure_connection(&self) -> Result<RunnerConnection, ToolError> {
        let base_url = std::env::var("STUDYPULSE_RUNNER_URL")
            .unwrap_or_else(|_| DEFAULT_RUNNER_URL.into())
            .trim_end_matches('/')
            .to_owned();
        let configured_token = std::env::var("STUDYPULSE_RUNNER_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty());

        if let Some(token) = configured_token {
            let health = wait_for_runner_health(&base_url, &token)?;
            return Ok(RunnerConnection {
                base_url,
                token,
                health,
            });
        }

        if base_url != DEFAULT_RUNNER_URL {
            return Err(ToolError::Execution(
                "STUDYPULSE_RUNNER_TOKEN is required when STUDYPULSE_RUNNER_URL is customized"
                    .into(),
            ));
        }

        let mut state = self.inner.state.lock();
        if let Some(managed) = state.managed.as_mut() {
            match managed.child.try_wait() {
                Ok(None) => {
                    let base_url = managed.base_url.clone();
                    let token = managed.token.clone();
                    return match wait_for_runner_health(&base_url, &token) {
                        Ok(health) => Ok(RunnerConnection {
                            base_url,
                            token,
                            health,
                        }),
                        Err(error) => {
                            stop_managed_runner(&mut state);
                            Err(error)
                        }
                    };
                }
                Ok(Some(_)) => state.managed = None,
                Err(error) => {
                    return Err(ToolError::Execution(format!(
                        "could not inspect managed Runner process: {error}"
                    )));
                }
            }
        }

        ensure_runner_image()?;
        let token = format!("studypulse-{}", Uuid::new_v4().simple());
        let container_name = format!("studypulse-runner-{}", Uuid::new_v4().simple());
        let child = Command::new(docker_program())
            .args([
                "run",
                "--rm",
                "--publish",
                "127.0.0.1:45891:45891",
                "--read-only",
                "--tmpfs",
                "/tmp:rw,noexec,nosuid,size=64m",
                "--network",
                "none",
                "--cap-drop",
                "ALL",
                "--pids-limit",
                "64",
                "--memory",
                "512m",
                "--cpus",
                "1",
                "--security-opt",
                "no-new-privileges",
                "--env",
                "STUDYPULSE_RUNNER_TOKEN",
            ])
            .arg("--name")
            .arg(&container_name)
            .arg(RUNNER_IMAGE)
            .env("STUDYPULSE_RUNNER_TOKEN", &token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ToolError::Execution(
                        "Docker is not installed or is not available on PATH; cannot start the isolated Runner".into(),
                    )
                } else {
                    ToolError::Execution(format!("could not start the isolated Runner: {error}"))
                }
            })?;

        state.managed = Some(ManagedRunner {
            child,
            token: token.clone(),
            base_url: DEFAULT_RUNNER_URL.into(),
        });

        match wait_for_runner_health(DEFAULT_RUNNER_URL, &token) {
            Ok(health) => Ok(RunnerConnection {
                base_url: DEFAULT_RUNNER_URL.into(),
                token,
                health,
            }),
            Err(error) => {
                stop_managed_runner(&mut state);
                Err(error)
            }
        }
    }
}

impl Drop for RunnerManagerInner {
    fn drop(&mut self) {
        stop_managed_runner(&mut self.state.lock());
    }
}

fn stop_managed_runner(state: &mut RunnerState) {
    if let Some(mut managed) = state.managed.take() {
        let _ = managed.child.kill();
        let _ = managed.child.wait();
    }
}

fn ensure_runner_image() -> Result<(), ToolError> {
    let status = Command::new(docker_program())
        .args(["image", "inspect", RUNNER_IMAGE])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ToolError::Execution(
                    "Docker is not installed or is not available on PATH; cannot start the isolated Runner".into(),
                )
            } else {
                ToolError::Execution(format!("could not inspect the Runner image: {error}"))
            }
        })?;
    if !status.success() {
        return Err(ToolError::Execution(
            "Runner image 'studypulse-runner' is missing; build it from core/ with 'docker build -f crates/studypulse-runner/Dockerfile -t studypulse-runner .'".into(),
        ));
    }
    Ok(())
}

fn docker_program() -> &'static str {
    [
        "/opt/homebrew/bin/docker",
        "/usr/local/bin/docker",
        "/Applications/Docker.app/Contents/Resources/bin/docker",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).is_file())
    .unwrap_or("docker")
}

fn wait_for_runner_health(base_url: &str, token: &str) -> Result<RunnerHealth, ToolError> {
    let client = Client::builder()
        .timeout(StdDuration::from_secs(2))
        .user_agent("StudyPulse-Desktop/1.0")
        .build()
        .map_err(|error| ToolError::Execution(error.to_string()))?;
    let health_url = format!("{base_url}/health");
    let deadline = Instant::now() + RUNNER_HEALTH_TIMEOUT;
    loop {
        let last_error = match client.get(&health_url).bearer_auth(token).send() {
            Ok(response) => {
                let status = response.status();
                match response.json::<RunnerHealth>() {
                    Ok(health) if status.is_success() && health.ok => {
                        if health.isolation != "container" {
                            return Err(ToolError::Execution(
                                "Runner is reachable but did not confirm container isolation"
                                    .into(),
                            ));
                        }
                        return Ok(health);
                    }
                    Ok(health) => {
                        format!(
                            "Runner returned {status} (ok={}, isolation={})",
                            health.ok, health.isolation
                        )
                    }
                    Err(error) => format!("invalid Runner health response: {error}"),
                }
            }
            Err(error) => error.to_string(),
        };

        if Instant::now() >= deadline {
            return Err(ToolError::Execution(format!(
                "Runner health check timed out after {} seconds: {}",
                RUNNER_HEALTH_TIMEOUT.as_secs(),
                last_error
            )));
        }
        std::thread::sleep(StdDuration::from_millis(250));
    }
}

impl ToolRegistry {
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            definition::<EmptyArgs>(
                "list_workspace_files",
                "List the selected Notebook files in Documents and Notes. Use this before answering questions about local files or when you need to discover a source path.",
                PermissionLevel::Read,
            ),
            definition::<SearchWorkspaceArgs>(
                "search_workspace",
                "Search file names and UTF-8 text in the selected Notebook sources. Use this to locate evidence before reading a source.",
                PermissionLevel::Read,
            ),
            definition::<ReadSourceArgs>(
                "read_source",
                "Read bounded UTF-8 text from one selected Notebook source. Use this to ground a factual answer in the user's local notes.",
                PermissionLevel::Read,
            ),
            definition::<ReadMemoryArgs>(
                "read_memory",
                "Read the current Notebook or Workspace Agent memory snapshot. Use this when prior preferences or saved context affect the answer.",
                PermissionLevel::Read,
            ),
            definition::<WriteMemoryArgs>(
                "write_memory",
                "Propose a change to Agent memory. This always requires confirmation.",
                PermissionLevel::Write,
            ),
            definition::<WebSearchArgs>(
                "web_search",
                "Search the configured web provider and return bounded cited results. Use this for current or external facts, not for local Notebook content.",
                PermissionLevel::Read,
            ),
            definition::<PaperSearchArgs>(
                "paper_search",
                "Search arXiv and return bounded paper metadata and citations. Use this for research questions that need academic sources.",
                PermissionLevel::Read,
            ),
            definition::<CodeExecutionArgs>(
                "code_execution",
                "Run Python locally on this Mac after native user confirmation. Use this for calculations, code, data transformation, checking a result, or validating a visualization; never invent an execution result. The host process is time-limited and output-bounded, but it is not a security sandbox.",
                PermissionLevel::Execute,
            ),
            definition::<SaveArtifactArgs>(
                "save_artifact",
                "Save a generated report, question set, or visualization artifact in the current Notebook Agent archive.",
                PermissionLevel::Write,
            ),
            definition::<AskUserArgs>(
                "ask_user",
                "Pause the Agent and ask the learner one bounded clarifying question.",
                PermissionLevel::Read,
            ),
            definition::<EmptyArgs>(
                "get_tasks",
                "Read StudyPulse homework and reading tasks.",
                PermissionLevel::Read,
            ),
            definition::<CreateTaskArgs>(
                "create_task",
                "Create a StudyPulse homework or reading task. This always requires confirmation.",
                PermissionLevel::Write,
            ),
        ]
    }

    pub fn prepare(
        &self,
        call_id: impl Into<String>,
        name: &str,
        arguments: Value,
    ) -> Result<PreparedTool, ToolError> {
        let call_id = call_id.into();
        match name {
            "list_workspace_files" => {
                parse::<EmptyArgs>(name, arguments)?;
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Read,
                    preview: "Browse Documents and Notes".into(),
                    invocation: Invocation::ListWorkspaceFiles,
                })
            }
            "search_workspace" => {
                let args = parse::<SearchWorkspaceArgs>(name, arguments)?;
                if args.query.trim().is_empty() {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "query must not be empty".into(),
                    });
                }
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Read,
                    preview: format!("Search Workspace for “{}”", args.query),
                    invocation: Invocation::SearchWorkspace(args),
                })
            }
            "read_source" => {
                let args = parse::<ReadSourceArgs>(name, arguments)?;
                if args.path.trim().is_empty() {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "path must not be empty".into(),
                    });
                }
                if args.max_chars == 0 || args.max_chars > 32_000 {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "max_chars must be between 1 and 32000".into(),
                    });
                }
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Read,
                    preview: format!("Read source {}", args.path),
                    invocation: Invocation::ReadSource(args),
                })
            }
            "read_memory" => {
                let args = parse::<ReadMemoryArgs>(name, arguments)?;
                validate_memory_scope(name, &args.scope)?;
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Read,
                    preview: format!("Read {} Agent memory", args.scope),
                    invocation: Invocation::ReadMemory(args),
                })
            }
            "write_memory" => {
                let args = parse::<WriteMemoryArgs>(name, arguments)?;
                validate_memory_scope(name, &args.scope)?;
                if args.key.trim().is_empty()
                    || !args
                        .key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "key must contain only letters, numbers, '_' or '-'".into(),
                    });
                }
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Write,
                    preview: format!("Update {} Agent memory key {}", args.scope, args.key),
                    invocation: Invocation::WriteMemory(args),
                })
            }
            "web_search" => {
                let args = parse::<WebSearchArgs>(name, arguments)?;
                validate_search_args(name, &args.query, args.max_results, 8)?;
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Read,
                    preview: format!("Search the configured web provider for {}", args.query),
                    invocation: Invocation::WebSearch(args),
                })
            }
            "paper_search" => {
                let args = parse::<PaperSearchArgs>(name, arguments)?;
                validate_search_args(name, &args.query, args.max_results, 5)?;
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Read,
                    preview: format!("Search arXiv for {}", args.query),
                    invocation: Invocation::PaperSearch(args),
                })
            }
            "code_execution" => {
                let args = parse::<CodeExecutionArgs>(name, arguments)?;
                if args.code.trim().is_empty() || args.code.len() > 100_000 {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "code must be between 1 and 100000 bytes".into(),
                    });
                }
                if !matches!(
                    args.language.to_ascii_lowercase().as_str(),
                    "python" | "python3"
                ) {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "only Python is supported for local execution".into(),
                    });
                }
                if args.stdin.len() > MAX_LOCAL_EXECUTION_STDIN_BYTES {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "stdin must not exceed 65536 bytes".into(),
                    });
                }
                if args.timeout_seconds.is_some_and(|seconds| {
                    !(1..=MAX_LOCAL_EXECUTION_TIMEOUT_SECONDS).contains(&seconds)
                }) {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "timeout_seconds must be between 1 and 30".into(),
                    });
                }
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Execute,
                    preview: "Run Python locally on this Mac".into(),
                    invocation: Invocation::CodeExecution(args),
                })
            }
            "save_artifact" => {
                let args = parse::<SaveArtifactArgs>(name, arguments)?;
                if args.artifact_id.is_empty()
                    || !args
                        .artifact_id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-')
                    || args.extension.is_empty()
                    || !args.extension.chars().all(|c| c.is_ascii_alphanumeric())
                    || args.content.len() > 10 * 1024 * 1024
                {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "artifact id/extension are unsafe or content exceeds 10 MiB".into(),
                    });
                }
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Write,
                    preview: format!("Save Agent artifact .{}", args.extension),
                    invocation: Invocation::SaveArtifact(args),
                })
            }
            "ask_user" => {
                let args = parse::<AskUserArgs>(name, arguments)?;
                if args.prompt.trim().is_empty()
                    || args.prompt.chars().count() > 1_000
                    || args.options.len() > 6
                {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail:
                            "prompt must be non-empty and options must contain at most six items"
                                .into(),
                    });
                }
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Read,
                    preview: args.prompt.clone(),
                    invocation: Invocation::AskUser(args),
                })
            }
            "get_tasks" => {
                parse::<EmptyArgs>(name, arguments)?;
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Read,
                    preview: "Read StudyPulse tasks".into(),
                    invocation: Invocation::GetTasks,
                })
            }
            "create_task" => {
                let args = parse::<CreateTaskArgs>(name, arguments)?;
                if args.title.trim().is_empty() {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "title must not be empty".into(),
                    });
                }
                if args
                    .importance
                    .is_some_and(|value| !(1..=5).contains(&value))
                {
                    return Err(ToolError::InvalidArguments {
                        tool: name.into(),
                        detail: "importance must be between 1 and 5".into(),
                    });
                }
                Ok(PreparedTool {
                    call_id,
                    name: name.into(),
                    permission: PermissionLevel::Write,
                    preview: format!("Create task “{}”", args.title.trim()),
                    invocation: Invocation::CreateTask(args),
                })
            }
            other => Err(ToolError::UnknownTool(other.into())),
        }
    }

    pub fn execute(
        &self,
        prepared: PreparedTool,
        workspace: &Workspace,
        now: DateTime<Utc>,
    ) -> Result<Value, ToolError> {
        self.execute_with_sources(prepared, workspace, now, None)
    }

    pub fn execute_for_sources(
        &self,
        prepared: PreparedTool,
        workspace: &Workspace,
        now: DateTime<Utc>,
        source_paths: &[String],
    ) -> Result<Value, ToolError> {
        self.execute_with_sources(prepared, workspace, now, Some(source_paths))
    }

    fn execute_with_sources(
        &self,
        prepared: PreparedTool,
        workspace: &Workspace,
        now: DateTime<Utc>,
        source_paths: Option<&[String]>,
    ) -> Result<Value, ToolError> {
        let call_id = prepared.call_id.clone();
        match prepared.invocation {
            Invocation::ListWorkspaceFiles => {
                let files = match source_paths {
                    Some(paths) => workspace.list_selected_library_files(paths)?,
                    None => workspace.list_library_files()?,
                };
                Ok(serde_json::to_value(files)
                    .map_err(|error| ToolError::Execution(error.to_string()))?)
            }
            Invocation::SearchWorkspace(args) => {
                let matches = match source_paths {
                    Some(paths) => workspace.search_selected_library(&args.query, paths)?,
                    None => workspace.search_library(&args.query)?,
                };
                Ok(serde_json::to_value(matches)
                    .map_err(|error| ToolError::Execution(error.to_string()))?)
            }
            Invocation::ReadSource(args) => {
                let Some(paths) = source_paths else {
                    return Err(ToolError::Execution(
                        "read_source requires a Notebook source selection".into(),
                    ));
                };
                if !paths.iter().any(|path| path == &args.path) {
                    return Err(ToolError::Execution(
                        "source is not selected in the active Notebook".into(),
                    ));
                }
                let content = workspace.read_library_source(&args.path, args.max_chars)?;
                Ok(json!({"path": args.path, "content": content}))
            }
            Invocation::ReadMemory(args) => Ok(workspace.read_agent_memory(&args.scope)?),
            Invocation::WriteMemory(args) => {
                let mut memory = workspace.read_agent_memory(&args.scope)?;
                let object = memory.as_object_mut().ok_or_else(|| {
                    ToolError::Execution("Agent memory root must be an object".into())
                })?;
                object.insert(args.key, args.value);
                workspace.write_agent_memory(&args.scope, &memory)?;
                Ok(json!({"ok": true, "scope": args.scope}))
            }
            Invocation::WebSearch(args) => {
                let query = args.query;
                let max_results = args.max_results;
                Ok(std::thread::spawn(move || web_search(&query, max_results))
                    .join()
                    .map_err(|_| ToolError::Execution("web search worker panicked".into()))??)
            }
            Invocation::PaperSearch(args) => {
                let query = args.query;
                let max_results = args.max_results;
                Ok(
                    std::thread::spawn(move || paper_search(&query, max_results))
                        .join()
                        .map_err(|_| {
                            ToolError::Execution("paper search worker panicked".into())
                        })??,
                )
            }
            Invocation::CodeExecution(args) => {
                let runner = self.runner.clone();
                Ok(std::thread::spawn(move || execute_code(&args, &runner))
                    .join()
                    .map_err(|_| ToolError::Execution("code execution worker panicked".into()))??)
            }
            Invocation::SaveArtifact(args) => {
                let relative = workspace.write_agent_artifact(
                    &call_id,
                    &args.artifact_id,
                    &args.extension,
                    args.content.as_bytes(),
                )?;
                Ok(json!({"ok": true, "relative_path": relative, "artifact_id": args.artifact_id}))
            }
            Invocation::AskUser(args) => Ok(json!({
                "wait_for_input": true,
                "prompt": args.prompt,
                "options": args.options,
            })),
            Invocation::GetTasks => Ok(serde_json::to_value(workspace.read_tasks()?)
                .map_err(|error| ToolError::Execution(error.to_string()))?),
            Invocation::CreateTask(args) => {
                let created_at = timestamp(now);
                let due_date =
                    validate_or_default_date(args.due_date, now + Duration::days(1), "due_date")?;
                let reminder_date =
                    validate_or_default_date(args.reminder_date, now, "reminder_date")?;
                let task = TaskItem {
                    id: Uuid::new_v4(),
                    title: args.title.trim().into(),
                    task_type: match args.task_type.unwrap_or(TaskTypeArg::Homework) {
                        TaskTypeArg::Homework => TaskType::Homework,
                        TaskTypeArg::Reading => TaskType::Reading,
                    },
                    due_date,
                    reminder_date,
                    subject: args.subject.unwrap_or_default(),
                    importance: args.importance.unwrap_or(3),
                    notes: args.notes.unwrap_or_default(),
                    is_completed: false,
                    reminder_event_id: None,
                    reminder_calendar_id: None,
                    created_at,
                    phase_id: None,
                    coach_execution_data: None,
                    coach_goal_id: None,
                    coach_proposal_id: None,
                    extra: BTreeMap::new(),
                };
                workspace.append_task(task.clone())?;
                Ok(json!({ "ok": true, "task": task }))
            }
        }
    }
}

fn definition<T: JsonSchema>(
    name: &str,
    description: &str,
    permission: PermissionLevel,
) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        parameters: serde_json::to_value(schema_for!(T)).expect("JSON schema must be serializable"),
        permission,
    }
}

fn parse<T: for<'de> Deserialize<'de>>(name: &str, value: Value) -> Result<T, ToolError> {
    serde_json::from_value(value).map_err(|error| ToolError::InvalidArguments {
        tool: name.into(),
        detail: error.to_string(),
    })
}

fn validate_or_default_date(
    value: Option<String>,
    default: DateTime<Utc>,
    field: &str,
) -> Result<String, ToolError> {
    match value {
        Some(value) => {
            DateTime::parse_from_rfc3339(&value).map_err(|error| ToolError::InvalidArguments {
                tool: "create_task".into(),
                detail: format!("{field} must be ISO-8601: {error}"),
            })?;
            Ok(value)
        }
        None => Ok(timestamp(default)),
    }
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn default_read_chars() -> usize {
    12_000
}

fn default_memory_scope() -> String {
    "workspace".into()
}

fn default_search_results() -> usize {
    5
}

fn default_paper_results() -> usize {
    3
}

fn validate_search_args(
    tool: &str,
    query: &str,
    max_results: usize,
    max_allowed: usize,
) -> Result<(), ToolError> {
    if query.trim().is_empty() {
        return Err(ToolError::InvalidArguments {
            tool: tool.into(),
            detail: "query must not be empty".into(),
        });
    }
    if query.chars().count() > 200 || max_results == 0 || max_results > max_allowed {
        return Err(ToolError::InvalidArguments {
            tool: tool.into(),
            detail: format!("query must be <= 200 chars and max_results must be 1..={max_allowed}"),
        });
    }
    Ok(())
}

fn web_search(query: &str, max_results: usize) -> Result<Value, ToolError> {
    let base_url = std::env::var("STUDYPULSE_SEARXNG_URL").map_err(|_| {
        ToolError::Execution("SearXNG is not configured; set STUDYPULSE_SEARXNG_URL".into())
    })?;
    let base_url = base_url.trim_end_matches('/');
    let parsed = reqwest::Url::parse(base_url)
        .map_err(|error| ToolError::Execution(format!("invalid SearXNG URL: {error}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(ToolError::Execution(
            "SearXNG URL must use http or https".into(),
        ));
    }
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("StudyPulse-Desktop/1.0")
        .build()
        .map_err(|error| ToolError::Execution(error.to_string()))?;
    let response = client
        .get(format!("{base_url}/search"))
        .query(&[("q", query), ("format", "json")])
        .send()
        .map_err(|error| ToolError::Execution(format!("SearXNG request failed: {error}")))?;
    if !response.status().is_success() {
        return Err(ToolError::Execution(format!(
            "SearXNG returned {}",
            response.status()
        )));
    }
    let payload: SearxResponse = response
        .json()
        .map_err(|error| ToolError::Execution(format!("invalid SearXNG response: {error}")))?;
    let results = payload
        .results
        .into_iter()
        .take(max_results)
        .enumerate()
        .map(|(index, result)| {
            json!({
                "id": format!("W{}", index + 1),
                "title": result.title,
                "url": result.url,
                "snippet": result.content,
                "source": result.engine.unwrap_or_else(|| "SearXNG".into()),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"query": query, "provider": "searxng", "results": results}))
}

fn paper_search(query: &str, max_results: usize) -> Result<Value, ToolError> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("StudyPulse-Desktop/1.0 (learning research)")
        .build()
        .map_err(|error| ToolError::Execution(error.to_string()))?;
    let response = client
        .get("https://export.arxiv.org/api/query")
        .query(&[
            ("search_query", format!("all:{query}")),
            ("start", "0".into()),
            ("max_results", max_results.to_string()),
        ])
        .send()
        .map_err(|error| ToolError::Execution(format!("arXiv request failed: {error}")))?;
    if !response.status().is_success() {
        return Err(ToolError::Execution(format!(
            "arXiv returned {}",
            response.status()
        )));
    }
    let xml = response
        .text()
        .map_err(|error| ToolError::Execution(format!("invalid arXiv response: {error}")))?;
    let feed: ArxivFeed = from_xml(&xml)
        .map_err(|error| ToolError::Execution(format!("invalid arXiv XML: {error}")))?;
    let results = feed
        .entries
        .into_iter()
        .take(max_results)
        .enumerate()
        .map(|(index, entry)| {
            json!({
                "id": format!("P{}", index + 1),
                "title": compact_text(entry.title),
                "url": entry.id,
                "abstract": compact_text(entry.summary),
                "published": entry.published,
                "authors": entry.authors.into_iter().map(|author| author.name).collect::<Vec<_>>(),
                "source": "arXiv",
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"query": query, "provider": "arxiv", "results": results}))
}

fn compact_text(value: String) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Deserialize)]
struct SearxResponse {
    #[serde(default)]
    results: Vec<SearxResult>,
}

#[derive(Debug, Deserialize)]
struct SearxResult {
    title: String,
    url: String,
    #[serde(default)]
    content: String,
    engine: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArxivFeed {
    #[serde(rename = "entry", default)]
    entries: Vec<ArxivEntry>,
}

#[derive(Debug, Deserialize)]
struct ArxivEntry {
    id: String,
    title: String,
    summary: String,
    published: String,
    #[serde(rename = "author", default)]
    authors: Vec<ArxivAuthor>,
}

#[derive(Debug, Deserialize)]
struct ArxivAuthor {
    name: String,
}

fn validate_memory_scope(tool: &str, scope: &str) -> Result<(), ToolError> {
    if scope == "workspace"
        || (!scope.is_empty() && scope.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    {
        Ok(())
    } else {
        Err(ToolError::InvalidArguments {
            tool: tool.into(),
            detail: "scope must be 'workspace' or a Notebook UUID".into(),
        })
    }
}

fn execute_code(args: &CodeExecutionArgs, runner: &RunnerManager) -> Result<Value, ToolError> {
    let backend = std::env::var("STUDYPULSE_CODE_EXECUTION_BACKEND")
        .unwrap_or_else(|_| "local".into())
        .to_ascii_lowercase();
    match backend.as_str() {
        "local" => execute_python_locally(args),
        "docker" => execute_in_runner(args, runner),
        other => Err(ToolError::Execution(format!(
            "unknown code execution backend '{other}'; use 'local' or 'docker'"
        ))),
    }
}

fn execute_python_locally(args: &CodeExecutionArgs) -> Result<Value, ToolError> {
    let execution_directory = LocalExecutionDirectory::create()?;
    let timeout_seconds = args.timeout_seconds.unwrap_or(10);
    let started_at = Instant::now();
    let mut child = Command::new(local_python_program())
        .args(["-I", "-c", args.code.as_str()])
        .current_dir(execution_directory.path())
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ToolError::Execution(
                    "Python 3 was not found; install it or set STUDYPULSE_PYTHON to the executable path".into(),
                )
            } else {
                ToolError::Execution(format!("could not start local Python: {error}"))
            }
        })?;

    let stdin_thread = child.stdin.take().map(|mut stdin| {
        let input = args.stdin.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        })
    });
    let stdout_thread = child
        .stdout
        .take()
        .map(|stdout| std::thread::spawn(move || capture_output(stdout)));
    let stderr_thread = child
        .stderr
        .take()
        .map(|stderr| std::thread::spawn(move || capture_output(stderr)));

    let deadline = Instant::now() + StdDuration::from_secs(timeout_seconds);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().map_err(|error| {
                    ToolError::Execution(format!("could not stop timed-out local Python: {error}"))
                })?;
            }
            Ok(None) => std::thread::sleep(StdDuration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ToolError::Execution(format!(
                    "could not inspect local Python process: {error}"
                )));
            }
        }
    };

    if let Some(thread) = stdin_thread {
        let _ = thread.join();
    }
    let stdout = stdout_thread
        .map(join_captured_output)
        .transpose()?
        .unwrap_or_default();
    let stderr = stderr_thread
        .map(join_captured_output)
        .transpose()?
        .unwrap_or_default();
    let stdout_text = normalize_process_output(&stdout.bytes);
    let stderr_text = normalize_process_output(&stderr.bytes);

    Ok(json!({
        "ok": !timed_out && status.success(),
        "backend": "local-python",
        "language": "python",
        "exit_code": status.code(),
        "stdout": stdout_text,
        "stderr": stderr_text,
        "timed_out": timed_out,
        "output_truncated": stdout.truncated || stderr.truncated,
        "duration_ms": started_at.elapsed().as_millis(),
        "error": timed_out.then_some("execution timed out"),
    }))
}

fn normalize_process_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

fn local_python_program() -> String {
    if let Ok(program) = std::env::var("STUDYPULSE_PYTHON")
        && !program.trim().is_empty()
    {
        return program;
    }

    #[cfg(windows)]
    {
        let path = std::env::var_os("PATH").unwrap_or_default();
        for program in ["python.exe", "python3.exe", "py.exe"] {
            if std::env::split_paths(&path).any(|directory| directory.join(program).is_file()) {
                return program.into();
            }
        }
        "python.exe".into()
    }

    #[cfg(not(windows))]
    {
        [
            "/opt/homebrew/bin/python3",
            "/usr/local/bin/python3",
            "/Library/Frameworks/Python.framework/Versions/Current/bin/python3",
            "/Library/Frameworks/Python.framework/Versions/3.14/bin/python3",
            "/usr/bin/python3",
        ]
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file())
        .unwrap_or("python3")
        .into()
    }
}

struct LocalExecutionDirectory {
    path: PathBuf,
}

impl LocalExecutionDirectory {
    fn create() -> Result<Self, ToolError> {
        let path = std::env::temp_dir().join(format!("studypulse-python-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).map_err(|error| {
            ToolError::Execution(format!("could not create local Python directory: {error}"))
        })?;
        Ok(Self { path })
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for LocalExecutionDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Default)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture_output<R: Read>(mut reader: R) -> io::Result<CapturedOutput> {
    let mut bytes = Vec::with_capacity(MAX_LOCAL_EXECUTION_OUTPUT_BYTES + 1);
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(CapturedOutput {
                bytes,
                truncated: false,
            });
        }
        let remaining = MAX_LOCAL_EXECUTION_OUTPUT_BYTES + 1 - bytes.len();
        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        if bytes.len() > MAX_LOCAL_EXECUTION_OUTPUT_BYTES {
            bytes.truncate(MAX_LOCAL_EXECUTION_OUTPUT_BYTES);
            return Ok(CapturedOutput {
                bytes,
                truncated: true,
            });
        }
    }
}

fn join_captured_output(
    thread: std::thread::JoinHandle<io::Result<CapturedOutput>>,
) -> Result<CapturedOutput, ToolError> {
    thread
        .join()
        .map_err(|_| ToolError::Execution("local Python output reader panicked".into()))?
        .map_err(|error| {
            ToolError::Execution(format!("could not read local Python output: {error}"))
        })
}

fn execute_in_runner(args: &CodeExecutionArgs, runner: &RunnerManager) -> Result<Value, ToolError> {
    let connection = runner.ensure_connection()?;
    let client = Client::builder()
        .timeout(StdDuration::from_secs(35))
        .user_agent("StudyPulse-Desktop/1.0")
        .build()
        .map_err(|error| ToolError::Execution(error.to_string()))?;
    let requested_language = match args.language.to_ascii_lowercase().as_str() {
        "python" | "python3" => "python",
        "javascript" | "js" => "javascript",
        _ => return Err(ToolError::Execution("unsupported Runner language".into())),
    };
    if !connection
        .health
        .languages
        .iter()
        .any(|language| language == requested_language)
    {
        return Err(ToolError::Execution(format!(
            "Runner does not support {requested_language}"
        )));
    }
    let url = format!("{}/v1/execute", connection.base_url);
    let response = client
        .post(url)
        .bearer_auth(connection.token)
        .json(&json!({
            "language": args.language,
            "code": args.code,
            "stdin": args.stdin,
            "timeout_seconds": args.timeout_seconds,
        }))
        .send()
        .map_err(|error| ToolError::Execution(format!("Runner request failed: {error}")))?;
    let status = response.status();
    let value: Value = response
        .json()
        .map_err(|error| ToolError::Execution(format!("invalid Runner response: {error}")))?;
    if !status.is_success() {
        return Err(ToolError::Execution(format!(
            "Runner returned {status}: {value}"
        )));
    }
    Ok(value)
}

#[derive(Debug, Deserialize)]
struct RunnerHealth {
    ok: bool,
    isolation: String,
    #[serde(default)]
    languages: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_task_is_declared_write_and_does_not_write_during_prepare() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        let registry = ToolRegistry::default();
        let prepared = registry
            .prepare(
                "call-1",
                "create_task",
                json!({"title": "Read chapter 3", "importance": 4}),
            )
            .unwrap();
        assert_eq!(prepared.permission, PermissionLevel::Write);
        assert!(workspace.read_tasks().unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_importance() {
        let registry = ToolRegistry::default();
        assert!(
            registry
                .prepare(
                    "call-1",
                    "create_task",
                    json!({"title": "Bad", "importance": 8}),
                )
                .is_err()
        );
    }

    #[test]
    fn read_tools_are_limited_to_notebook_sources() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        std::fs::write(
            workspace.root().join("Documents/algebra.md"),
            "Algebra source",
        )
        .unwrap();
        std::fs::write(
            workspace.root().join("Notes/chemistry.md"),
            "Chemistry source",
        )
        .unwrap();
        let registry = ToolRegistry::default();
        let sources = vec!["Documents/algebra.md".into()];

        let listed = registry
            .execute_for_sources(
                registry
                    .prepare("call-list", "list_workspace_files", json!({}))
                    .unwrap(),
                &workspace,
                Utc::now(),
                &sources,
            )
            .unwrap();
        let searched = registry
            .execute_for_sources(
                registry
                    .prepare(
                        "call-search",
                        "search_workspace",
                        json!({"query": "source"}),
                    )
                    .unwrap(),
                &workspace,
                Utc::now(),
                &sources,
            )
            .unwrap();

        assert!(listed.to_string().contains("algebra.md"));
        assert!(!listed.to_string().contains("chemistry.md"));
        assert!(searched.to_string().contains("algebra.md"));
        assert!(!searched.to_string().contains("chemistry.md"));
    }

    #[test]
    fn memory_and_execution_tools_have_explicit_risk_levels() {
        let registry = ToolRegistry::default();
        let read_memory = registry
            .prepare("memory", "read_memory", json!({"scope": "workspace"}))
            .unwrap();
        assert_eq!(read_memory.permission, PermissionLevel::Read);
        let write_memory = registry
            .prepare(
                "memory-write",
                "write_memory",
                json!({"scope": "workspace", "key": "preference", "value": "zh"}),
            )
            .unwrap();
        assert_eq!(write_memory.permission, PermissionLevel::Write);
        let code = registry
            .prepare(
                "code",
                "code_execution",
                json!({"language": "python", "code": "print(1)"}),
            )
            .unwrap();
        assert_eq!(code.permission, PermissionLevel::Execute);
    }

    #[test]
    fn local_python_execution_returns_bounded_structured_output() {
        let program = local_python_program();
        if Command::new(&program).arg("--version").status().is_err() {
            return;
        }
        let result = execute_python_locally(&CodeExecutionArgs {
            language: "python".into(),
            code: "print(2 + 2)".into(),
            stdin: String::new(),
            timeout_seconds: Some(5),
        })
        .unwrap();

        assert_eq!(result["backend"], "local-python");
        assert_eq!(result["ok"], true);
        assert_eq!(result["stdout"], "4\n");
        assert_eq!(result["output_truncated"], false);
    }
}
