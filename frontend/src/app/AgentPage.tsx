import { useEffect, useEffectEvent, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import rehypeSanitize from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import {
  parseAgentInputRequest,
  pythonCodeForCompletedEvent,
  pythonCodeForConfirmation,
} from "../lib/agentEvents";
import { core } from "../lib/core";
import { sanitizeMarkup, svgSandboxDocument } from "../lib/sanitizeMarkup";
import { localizeEnum, useI18n, type Translate } from "../i18n";
import type {
  AgentEvent,
  AgentEventKind,
  AgentMessage,
  AgentMode,
  AgentNotebook,
  AgentTurn,
  AppSnapshot,
  ArtifactRef,
  SourceRef,
  UsageSummary,
} from "../types";
import MathText from "../components/MathText";
import { AppIcon, formatDate } from "../components/UIComponents";
import { useToast } from "../components/Toast";

function modeLabel(t: Translate, value: string): string {
  const key =
    value === "DeepSolve"
      ? "deepSolve"
      : value === "DeepResearch"
      ? "deepResearch"
      : value === "QuestionLab"
      ? "questionLab"
      : value === "ExamSimulation"
      ? "examSimulation"
      : value === "ReversePlanner"
      ? "reversePlanner"
      : value.toLowerCase();
  return localizeEnum(t, "mode", key);
}

function parseUsage(value: unknown): UsageSummary | undefined {
  if (!value || typeof value !== "object") return undefined;
  const record = value as Record<string, unknown>;
  const numberValue = (snakeKey: string, camelKey: string) => {
    const candidate = record[snakeKey] ?? record[camelKey];
    return typeof candidate === "number" && Number.isFinite(candidate) ? candidate : 0;
  };
  return {
    prompt_tokens: numberValue("prompt_tokens", "promptTokens"),
    completion_tokens: numberValue("completion_tokens", "completionTokens"),
    total_tokens: numberValue("total_tokens", "totalTokens"),
    model_calls: numberValue("model_calls", "modelCalls"),
    estimated: Boolean(record.estimated),
  };
}

function eventLabel(t: Translate, value: AgentEventKind): string {
  const key = value.replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase().replaceAll("_", "");
  const keyMap: Record<string, string> = {
    started: "started",
    statuschanged: "statusChanged",
    textdelta: "textDelta",
    toolrequested: "toolRequested",
    toolcompleted: "toolCompleted",
    confirmationrequired: "confirmationRequired",
    stagestarted: "stageStarted",
    stageprogress: "stageProgress",
    stagecompleted: "stageCompleted",
    inputrequired: "inputRequired",
    artifactcreated: "artifactCreated",
    observation: "observation",
    sources: "sources",
    result: "result",
    usage: "usage",
    turnrecovered: "turnRecovered",
    failed: "failed",
    cancelled: "cancelled",
    completed: "completed",
  };
  return t(`event.${keyMap[key] ?? ""}`) === `event.${keyMap[key] ?? ""}`
    ? value.replaceAll("_", " ")
    : t(`event.${keyMap[key]}`);
}

function PythonCodeBlock({ source }: { source: string }) {
  const { t } = useI18n();
  const { showToast } = useToast();

  const handleCopy = () => {
    void navigator.clipboard.writeText(source);
    showToast(t("agent.codeCopied"), "success");
  };

  return (
    <div className="agent-code-wrapper">
      <div className="code-header">
        <span className="code-lang">python</span>
        <button className="code-copy-btn" onClick={handleCopy}>
          {t("agent.copyCode")}
        </button>
      </div>
      <p className="code-safety-warning" role="alert">
        {t("agent.localPythonWarning")}
      </p>
      <pre className="agent-python-code">
        <code>{source}</code>
      </pre>
    </div>
  );
}

function AgentTimelineEvent({
  event,
  events,
  language,
  t,
}: {
  event: AgentEvent;
  events: AgentEvent[];
  language: string;
  t: Translate;
}) {
  const pythonCode = pythonCodeForCompletedEvent(event, events);
  return (
    <div className="timeline-item">
      <span className={`timeline-dot kind-${event.kind.toLowerCase()}`} />
      <div className="timeline-content">
        <div className="timeline-header">
          <strong>{eventLabel(t, event.kind)}</strong>
          <span className="timeline-time">{formatDate(event.timestamp, language)}</span>
        </div>
        <span className="timeline-detail">
          {event.tool_name ?? event.stage ?? event.preview}
        </span>
        {pythonCode && <PythonCodeBlock source={pythonCode} />}
      </div>
    </div>
  );
}

interface ConsumedRun {
  text: string;
  sources: SourceRef[];
  artifacts: ArtifactRef[];
  usage?: UsageSummary;
  status: "completed" | "failed" | "cancelled";
  error?: string;
}

export function AgentPage({
  workspaceId,
  provider,
  initialGoal,
  onInitialGoalHandled,
}: {
  workspaceId: string;
  provider?: AppSnapshot["provider"];
  initialGoal?: string;
  onInitialGoalHandled?: () => void;
}) {
  const { language, t } = useI18n();
  const { showToast } = useToast();
  const queryClient = useQueryClient();

  const notebooksQuery = useQuery({ queryKey: ["notebooks"], queryFn: core.notebooks });
  const filesQuery = useQuery({ queryKey: ["library"], queryFn: core.library });
  const capabilitiesQuery = useQuery({ queryKey: ["capabilities"], queryFn: core.capabilities });
  const turnsQuery = useQuery({ queryKey: ["agent-turns"], queryFn: core.agentTurns });

  const [selectedId, setSelectedId] = useState<string>();
  const [mode, setMode] = useState<AgentMode>("Chat");
  const [goal, setGoal] = useState(initialGoal ?? "");
  const [sourcePaths, setSourcePaths] = useState<string[]>([]);
  const [runId, setRunId] = useState<string>();
  const [running, setRunning] = useState(false);
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [answer, setAnswer] = useState("");
  const [pending, setPending] = useState<AgentEvent>();
  const [pendingInput, setPendingInput] = useState("");
  const [submittingInput, setSubmittingInput] = useState(false);
  const [activity, setActivity] = useState<string>();
  const [sources, setSources] = useState<SourceRef[]>([]);
  const [artifacts, setArtifacts] = useState<ArtifactRef[]>([]);
  const [usage, setUsage] = useState<UsageSummary>();
  const [structuredOutput, setStructuredOutput] = useState<Record<string, unknown> | null>(null);
  const [errorMessage, setErrorMessage] = useState<string>();
  const [notebookSaving, setNotebookSaving] = useState(false);

  const [showContext, setShowContext] = useState(true);
  const [showThreads, setShowThreads] = useState(true);
  const initialGoalConsumedRef = useRef<string | undefined>(undefined);

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const composerTextareaRef = useRef<HTMLTextAreaElement>(null);

  const notebooks = notebooksQuery.data ?? [];
  const selected = notebooks.find((n) => n.id === selectedId) ?? notebooks[0];
  const effectiveSourcePaths = selectedId ? sourcePaths : (selected?.source_paths ?? sourcePaths);
  const selectedMessages: AgentMessage[] = useMemo(() => selected?.messages ?? [], [selected?.messages]);
  const files = (filesQuery.data ?? []).filter((f) => !f.is_directory);
  const canRun = Boolean(
    provider?.cloud_account || provider?.byok_config || provider?.has_saved_byok
  );
  const recoverableTurn = (turnsQuery.data ?? []).find(
    (turn) => turn.status === "recoverable" && turn.resume_safe
  );

  function reportError(error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    setErrorMessage(message);
    setActivity(message);
    showToast(message, "error");
  }

  // Auto-scroll messages
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [selectedMessages, answer]);

  async function persist(next: AgentNotebook[]) {
    await core.saveNotebooks(workspaceId, next);
    await queryClient.invalidateQueries({ queryKey: ["notebooks"] });
  }

  async function createNotebook(): Promise<AgentNotebook> {
    const notebook: AgentNotebook = {
      id: crypto.randomUUID(),
      title: t("agent.untitledNotebook", { count: notebooks.length + 1 }),
      source_paths: [],
      messages: [],
      last_goal: "",
      last_answer: "",
      updated_at: new Date().toISOString(),
    };
    await persist([notebook, ...notebooks]);
    setSelectedId(notebook.id);
    setSourcePaths([]);
    return notebook;
  }

  async function handleCreateNotebook() {
    if (running || notebookSaving) return;
    setErrorMessage(undefined);
    setNotebookSaving(true);
    try {
      await createNotebook();
    } catch (error) {
      reportError(error);
    } finally {
      setNotebookSaving(false);
    }
  }

  async function toggleSource(path: string) {
    if (running) return;
    setErrorMessage(undefined);
    const previousPaths = effectiveSourcePaths;
    const nextPaths = effectiveSourcePaths.includes(path)
      ? effectiveSourcePaths.filter((v) => v !== path)
      : [...effectiveSourcePaths, path].sort();
    setSourcePaths(nextPaths);
    try {
      if (selected) {
        await persist(
          notebooks.map((n) =>
            n.id === selected.id
              ? { ...n, source_paths: nextPaths, updated_at: new Date().toISOString() }
              : n
          )
        );
      }
    } catch (error) {
      setSourcePaths(previousPaths);
      reportError(error);
    }
  }

  async function consumeRun(id: string): Promise<ConsumedRun> {
    let cursor = 0;
    let assembled = "";
    let collectedSources: SourceRef[] = [];
    let collectedArtifacts: ArtifactRef[] = [];
    let collectedUsage: UsageSummary | undefined;
    let terminalStatus: ConsumedRun["status"] = "completed";
    let terminalError: string | undefined;
    let finished = false;

    while (!finished) {
      const batch = await core.waitAgentEvents(id, cursor, 1000);
      for (const event of batch) {
        cursor = Math.max(cursor, event.sequence);
        setEvents((curr) => [...curr, event]);
        if (event.stage) setActivity(event.stage);
        if (event.kind === "TextDelta") {
          assembled += event.text ?? "";
          setAnswer(assembled);
        }
        if (event.kind === "Sources" && event.payload_json) {
          try {
            collectedSources = JSON.parse(event.payload_json) as SourceRef[];
            setSources(collectedSources);
          } catch {
            // keep last valid
          }
        }
        if (event.kind === "Usage" && event.payload_json) {
          try {
            collectedUsage = parseUsage(JSON.parse(event.payload_json));
            if (collectedUsage) setUsage(collectedUsage);
          } catch {
            // keep last valid
          }
        }
        if (event.kind === "ArtifactCreated" && event.payload_json) {
          try {
            const value = JSON.parse(event.payload_json) as {
              artifact_id?: string;
              relative_path?: string;
            };
            if (value.artifact_id && value.relative_path) {
              const extension = value.relative_path.split(".").at(-1) ?? "";
              const artifact: ArtifactRef = {
                artifact_id: value.artifact_id,
                path: value.relative_path,
                extension,
                render_type: extension === "svg" || extension === "html" ? extension : null,
              };
              collectedArtifacts = collectedArtifacts.some((item) => item.path === artifact.path)
                ? collectedArtifacts
                : [...collectedArtifacts, artifact];
              setArtifacts(collectedArtifacts);
            }
          } catch {
            // fallback
          }
        }
        if (event.kind === "Result" && event.payload_json) {
          try {
            const result = JSON.parse(event.payload_json) as {
              text?: string;
              output_json?: string | null;
              outputJson?: string | null;
              sources?: SourceRef[];
              artifacts?: ArtifactRef[];
              usage?: unknown;
            };
            if (result.text) {
              assembled = result.text;
              setAnswer(result.text);
            }
            const outputJson = result.output_json ?? result.outputJson;
            if (outputJson) {
              try {
                const parsed = JSON.parse(outputJson) as unknown;
                if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
                  setStructuredOutput(parsed as Record<string, unknown>);
                }
              } catch {
                // fallback
              }
            }
            if (result.sources) {
              collectedSources = result.sources;
              setSources(collectedSources);
            }
            if (result.artifacts) {
              collectedArtifacts = result.artifacts;
              setArtifacts(collectedArtifacts);
            }
            if (result.usage) {
              collectedUsage = parseUsage(result.usage);
              if (collectedUsage) setUsage(collectedUsage);
            }
          } catch {
            // fallback
          }
        }
        if ((event.kind === "ConfirmationRequired" || event.kind === "ToolRequested") && event.confirmation_id) {
          setPending(event);
        }
        if (event.kind === "InputRequired") {
          setPending(event);
          setPendingInput("");
          setSubmittingInput(false);
        }
        if (event.kind === "Failed") {
          terminalStatus = "failed";
          terminalError = event.preview ?? event.text ?? event.payload_json ?? undefined;
          finished = true;
        }
        if (event.kind === "Cancelled") {
          terminalStatus = "cancelled";
          finished = true;
        }
        if (event.kind === "Completed") {
          terminalStatus = "completed";
          finished = true;
        }
        // A terminal event is authoritative. Do not let a malformed or
        // reordered tail of the same batch turn a Failed/Cancelled run into a
        // successful Assistant message.
        if (finished) break;
      }
    }
    return {
      text: assembled,
      sources: collectedSources,
      artifacts: collectedArtifacts,
      usage: collectedUsage,
      status: terminalStatus,
      error: terminalError,
    };
  }

  async function runAgent(goalOverride?: string) {
    const trimmed = (goalOverride ?? goal).trim();
    if (!trimmed || running) return;
    setErrorMessage(undefined);
    setRunning(true);
    setActivity(t("agent.starting"));

    try {
      if (!canRun) {
        const restored = await core.restoreAi();
        if (!restored.cloud_account && !restored.byok_config && !restored.has_saved_byok) {
          reportError(t("agent.noProvider"));
          return;
        }
      }

      let notebook = selected;
      let notebookList = notebooks;
      if (!notebook) {
        notebook = await createNotebook();
        notebookList = [notebook, ...notebooks];
      }

      const history = notebook.messages;
      const userMessage: AgentMessage = {
        id: crypto.randomUUID(),
        role: "User",
        content: trimmed,
        created_at: new Date().toISOString(),
      };

      const optimisticNotebook = {
        ...notebook,
        source_paths: effectiveSourcePaths,
        messages: [...history, userMessage],
        last_goal: trimmed,
        last_answer: "",
        updated_at: new Date().toISOString(),
      };

      await persist(
        notebookList.map((n) => (n.id === notebook!.id ? optimisticNotebook : n))
      );

      setGoal("");
      setAnswer("");
      setEvents([]);
      setSources([]);
      setArtifacts([]);
      setUsage(undefined);
      setStructuredOutput(null);
      setPending(undefined);
      setPendingInput("");
      setSubmittingInput(false);

      const id = await core.startTurn({
        mode,
        goal: trimmed,
        sourcePaths: effectiveSourcePaths,
        history,
        notebookId: notebook.id,
      });
      setRunId(id);
      const completed = await consumeRun(id);

      if (completed.status !== "completed") {
        const message = completed.error ?? t(`event.${completed.status}`);
        setActivity(message);
        if (completed.status === "failed") {
          reportError(message);
        }
        return;
      }

      const assistant: AgentMessage = {
        id: crypto.randomUUID(),
        role: "Assistant",
        content: completed.text,
        created_at: new Date().toISOString(),
        turn_id: id,
        source_refs_json: JSON.stringify(completed.sources),
        artifact_refs_json: JSON.stringify(completed.artifacts),
      };

      await persist(
        notebookList.map((n) =>
          n.id === notebook!.id
            ? {
                ...optimisticNotebook,
                messages: [...optimisticNotebook.messages, assistant],
                last_answer: completed.text,
                updated_at: new Date().toISOString(),
              }
            : n
        )
      );

      await queryClient.invalidateQueries({ queryKey: ["agent-turns"] });
      setAnswer("");
      setActivity(t("agent.completed"));
    } catch (error) {
      reportError(error);
    } finally {
      setRunning(false);
      setPending(undefined);
      setRunId(undefined);
    }
  }

  const runInitialGoal = useEffectEvent((value: string) => {
    void runAgent(value);
  });

  useEffect(() => {
    if (
      !initialGoal ||
      notebooksQuery.isPending ||
      running ||
      initialGoalConsumedRef.current === initialGoal
    ) {
      return;
    }
    initialGoalConsumedRef.current = initialGoal;
    onInitialGoalHandled?.();
    setGoal(initialGoal);
    runInitialGoal(initialGoal);
  }, [initialGoal, notebooksQuery.isPending, onInitialGoalHandled, running]);

  async function resolveConfirmation(decision: "Allow" | "Deny") {
    if (!pending?.confirmation_id || !runId) return;
    setErrorMessage(undefined);
    try {
      await core.submitConfirmation(runId, pending.confirmation_id, decision);
      setPending(undefined);
    } catch (error) {
      reportError(error);
    }
  }

  async function resolveInput() {
    if (!pending?.confirmation_id || !runId || !pendingInput.trim() || submittingInput) return;
    setErrorMessage(undefined);
    setSubmittingInput(true);
    const answerJson = JSON.stringify({ answer: pendingInput });
    try {
      await core.submitAgentInput(runId, pending.confirmation_id, answerJson);
      setPending(undefined);
      setPendingInput("");
    } catch (error) {
      reportError(error);
    } finally {
      setSubmittingInput(false);
    }
  }

  async function cancel() {
    if (!runId) return;
    try {
      await core.cancelAgent(runId);
    } catch (error) {
      reportError(error);
    }
  }

  async function resumeTurn(turn: AgentTurn) {
    if (running) return;
    setErrorMessage(undefined);
    const storedMode: Record<string, AgentMode> = {
      chat: "Chat",
      deep_solve: "DeepSolve",
      mastery: "Mastery",
      deep_research: "DeepResearch",
      question_lab: "QuestionLab",
      visualize: "Visualize",
      coach: "Coach",
      exam_simulation: "ExamSimulation",
      reverse_planner: "ReversePlanner",
    };
    const resumeNotebook =
      (turn.notebook_id && notebooks.find((notebook) => notebook.id === turn.notebook_id)) ||
      notebooks.find((notebook) => notebook.last_goal === turn.goal) ||
      selected;
    if (resumeNotebook) {
      setSelectedId(resumeNotebook.id);
      setSourcePaths(resumeNotebook.source_paths);
    }
    setMode(storedMode[turn.mode] ?? mode);
    setRunning(true);
    setEvents([]);
    setSources([]);
    setArtifacts([]);
    setUsage(undefined);
    setStructuredOutput(null);
    setAnswer("");
    setPendingInput("");
    setSubmittingInput(false);
    setActivity(t("agent.starting"));

    try {
      const id = await core.resumeAgentTurn(turn.id);
      setRunId(id);
      const completed = await consumeRun(id);
      if (completed.status !== "completed") {
        const message = completed.error ?? t(`event.${completed.status}`);
        setActivity(message);
        if (completed.status === "failed") {
          reportError(message);
        }
        return;
      }

      const assembled = completed.text;
      if (resumeNotebook) {
        const assistant: AgentMessage = {
          id: crypto.randomUUID(),
          role: "Assistant",
          content: assembled,
          created_at: new Date().toISOString(),
          turn_id: id,
          source_refs_json: JSON.stringify(completed.sources),
          artifact_refs_json: JSON.stringify(completed.artifacts),
        };
        await persist(
          notebooks.map((n) =>
            n.id === resumeNotebook.id
              ? {
                  ...n,
                  messages: [...n.messages, assistant],
                  last_answer: assembled,
                  updated_at: new Date().toISOString(),
                }
              : n
          )
        );
      }
      await queryClient.invalidateQueries({ queryKey: ["agent-turns"] });
      setAnswer("");
      setActivity(t("agent.completed"));
    } catch (error) {
      reportError(error);
    } finally {
      setRunning(false);
      setRunId(undefined);
      setPending(undefined);
    }
  }

  const pendingPythonCode = pythonCodeForConfirmation(pending);
  const pendingInputRequest = pending?.kind === "InputRequired" ? parseAgentInputRequest(pending) : null;
  const contextSourceLabels = Array.from(
    new Set([...effectiveSourcePaths, ...sources.map((s) => s.title ?? s.locator)])
  ).slice(0, 15);

  return (
    <div className={`agent-page ${!showThreads ? "hide-threads" : ""} ${!showContext ? "hide-context" : ""}`}>
      <div className="agent-workbench-grid">
        {/* Left: Threads / Notebooks */}
        <aside className="agent-threads-panel">
          <div className="threads-header">
            <h3>{t("agent.thread")}</h3>
            <button
              className="icon-button"
              onClick={() => void handleCreateNotebook()}
              title={t("agent.newThread")}
              aria-label={t("agent.newThread")}
              disabled={running || notebookSaving}
            >
              <AppIcon name="plus" className="btn-icon" />
            </button>
          </div>

          {recoverableTurn && (
            <div className="agent-recovery-banner">
              <span className="recovery-spark">✦</span>
              <div className="recovery-text">
                <strong>{t("agent.recoverable")}</strong>
                <button
                  className="button primary small"
                  onClick={() => void resumeTurn(recoverableTurn)}
                  disabled={running}
                >
                  {t("agent.resume")}
                </button>
              </div>
            </div>
          )}

          <div className="threads-list">
            {notebooks.map((n) => (
              <button
                key={n.id}
                className={`thread-item ${selected?.id === n.id ? "active" : ""}`}
                onClick={() => {
                  setSelectedId(n.id);
                  setSourcePaths(n.source_paths);
                }}
                disabled={running}
              >
                <div className="thread-title">{n.title}</div>
                <div className="thread-meta">
                  <span>{t("agent.sources", { count: n.source_paths.length })}</span>
                  <span>·</span>
                  <span>{t("agent.messages", { count: n.messages.length })}</span>
                </div>
              </button>
            ))}
            {!notebooks.length && <p className="muted small-copy">{t("agent.noNotebook")}</p>}
          </div>
        </aside>

        {/* Center: Conversation & Composer */}
        <main className="agent-chat-panel">
          <header className="chat-topbar">
            <div className="chat-topbar-left">
              <button
                className="toggle-panel-btn"
                onClick={() => setShowThreads((v) => !v)}
                title={t("agent.toggleThreads")}
                aria-label={t("agent.toggleThreads")}
              >
                <AppIcon name="library" className="btn-icon" />
              </button>
              <h2 className="active-thread-title">
                {selected?.title ?? t("agent.newNotebook")}
              </h2>
            </div>

            <div className="chat-topbar-right">
              <select
                className="mode-select"
                value={mode}
                onChange={(e) => setMode(e.target.value as AgentMode)}
                disabled={running}
                aria-label={t("agent.mode")}
              >
                {(capabilitiesQuery.data ?? []).map((c) => (
                  <option key={c.mode} value={c.mode}>
                    {modeLabel(t, c.mode)}
                  </option>
                ))}
              </select>

              <button
                className={`toggle-context-btn ${showContext ? "active" : ""}`}
                onClick={() => setShowContext((v) => !v)}
                title={showContext ? t("agent.collapseContext") : t("agent.showContext")}
              >
                <AppIcon name="insights" className="btn-icon" />
              </button>
            </div>
          </header>

          {errorMessage && (
            <div className="panel error-card" role="alert">
              {errorMessage}
            </div>
          )}

          <div className="chat-viewport">
            {selectedMessages.map((msg) => (
              <div
                key={msg.id}
                className={`chat-message ${msg.role === "User" ? "user-msg" : "assistant-msg"}`}
              >
                <div className="message-avatar">
                  {msg.role === "User" ? "U" : <AppIcon name="agent" className="avatar-agent-icon" />}
                </div>
                <div className="message-bubble">
                  {msg.role === "Assistant" ? (
                    <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>
                      {msg.content}
                    </ReactMarkdown>
                  ) : (
                    <p>{msg.content}</p>
                  )}
                </div>
              </div>
            ))}

            {answer && !(structuredOutput && (mode === "QuestionLab" || mode === "Visualize")) && (
              <div className="chat-message assistant-msg streaming">
                <div className="message-avatar">
                  <AppIcon name="agent" className="avatar-agent-icon" />
                </div>
                <div className="message-bubble">
                  <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>
                    {answer}
                  </ReactMarkdown>
                </div>
              </div>
            )}

            {structuredOutput && mode === "QuestionLab" && (
              <QuestionSetView value={structuredOutput} t={t} />
            )}

            {structuredOutput && mode === "Visualize" && (
              <VisualizationView value={structuredOutput} t={t} />
            )}

            {!selectedMessages.length && !answer && !structuredOutput && (
              <div className="chat-empty-state">
                <div className="empty-spark-icon">✦</div>
                <h3>{t("agent.emptyTitle")}</h3>
                <p>{t("agent.emptyCopy")}</p>
              </div>
            )}

            {/* Inline Permission Confirmation Panel */}
            {pending && (pending.kind === "ConfirmationRequired" || pending.kind === "ToolRequested") && (
              <div className={`permission-card ${pendingPythonCode ? "has-python" : ""}`}>
                <div className="permission-head">
                  <span className="permission-badge">!</span>
                  <div>
                    <strong>{t("agent.permission")}</strong>
                    <p>{pending.preview ?? pending.tool_name ?? t("agent.toolFallback")}</p>
                  </div>
                </div>
                {pendingPythonCode && <PythonCodeBlock source={pendingPythonCode} />}
                <div className="permission-actions">
                  <button className="button subtle small" onClick={() => void resolveConfirmation("Deny")}>
                    {t("agent.deny")}
                  </button>
                  <button className="button primary small" onClick={() => void resolveConfirmation("Allow")}>
                    {t("agent.allowOnce")}
                  </button>
                </div>
              </div>
            )}

            {/* Inline Input Required Panel */}
            {pending?.kind === "InputRequired" && (
              <div className="permission-card input-card">
                <span className="permission-badge">?</span>
                <div className="input-body">
                  <strong>{t("agent.inputRequired")}</strong>
                  <div className="input-prompt">
                    <MathText
                      content={pendingInputRequest?.prompt || t("agent.inputFallback")}
                    />
                  </div>
                  {pendingInputRequest?.options.length ? (
                    <div className="input-options" role="group" aria-label={t("agent.inputRequired")}>
                      {pendingInputRequest.options.map((option) => (
                        <button
                          key={option}
                          type="button"
                          className={`input-option ${pendingInput === option ? "selected" : ""}`}
                          onClick={() => setPendingInput(option)}
                          disabled={submittingInput}
                        >
                          {option}
                        </button>
                      ))}
                    </div>
                  ) : null}
                  <textarea
                    className="input-answer"
                    value={pendingInput}
                    onChange={(e) => setPendingInput(e.target.value)}
                    placeholder={t("agent.answerPlaceholder")}
                    rows={2}
                    disabled={submittingInput}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && !e.shiftKey) {
                        e.preventDefault();
                        void resolveInput();
                      }
                    }}
                  />
                </div>
                <button
                  className="button primary small"
                  onClick={() => void resolveInput()}
                  disabled={!pendingInput.trim() || submittingInput}
                  aria-busy={submittingInput}
                >
                  {t("agent.send")}
                </button>
              </div>
            )}

            <div ref={messagesEndRef} />
          </div>

          {/* Bottom Composer */}
          <div className="chat-composer-container">
            <div className="chat-composer">
              <textarea
                ref={composerTextareaRef}
                value={goal}
                onChange={(e) => setGoal(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void runAgent();
                  }
                }}
                placeholder={t("agent.askPlaceholder")}
                disabled={running}
                rows={2}
              />

              <div className="composer-footer">
                <div className="composer-sources">
                  {files.slice(0, 4).map((file) => (
                    <button
                      key={file.relative_path}
                      className={`source-pill ${
                        effectiveSourcePaths.includes(file.relative_path) ? "selected" : ""
                      }`}
                      onClick={() => void toggleSource(file.relative_path)}
                      disabled={running}
                    >
                      <span>＋</span> {file.relative_path.split("/").at(-1)}
                    </button>
                  ))}
                  {files.length > 4 && (
                    <span className="muted small-copy">
                      {t("agent.moreLibrary", { count: files.length - 4 })}
                    </span>
                  )}
                </div>

                <div className="composer-controls">
                  <span className="activity-status">{activity ?? t("agent.shortcut")}</span>
                  {running ? (
                    <button
                      className="button danger small"
                      onClick={() => void cancel()}
                      disabled={!runId}
                    >
                      {t("agent.cancel")}
                    </button>
                  ) : (
                    <button
                      className="button primary small send-btn"
                      onClick={() => void runAgent()}
                      disabled={!goal.trim()}
                    >
                      {t("agent.run")} <span>↗</span>
                    </button>
                  )}
                </div>
              </div>
            </div>
          </div>
        </main>

        {/* Right: Context Inspector */}
        <aside className="agent-context-panel">
          <div className="context-header">
            <h3>{t("agent.context")}</h3>
            <span className="muted small-copy">{t("agent.timelineDescription")}</span>
          </div>

          <div className="context-sections">
            <section className="context-block">
              <div className="context-block-title">
                <strong>{t("agent.sourcesPanel")}</strong>
                <span className="count-badge">{contextSourceLabels.length}</span>
              </div>
              <div className="context-items-list">
                {contextSourceLabels.map((lbl) => (
                  <div className="context-item" key={lbl} title={lbl}>
                    <AppIcon name="library" className="item-icon" />
                    <span>{lbl}</span>
                  </div>
                ))}
              </div>
            </section>

            {artifacts.length > 0 && (
              <section className="context-block">
                <div className="context-block-title">
                  <strong>{t("agent.artifacts")}</strong>
                  <span className="count-badge">{artifacts.length}</span>
                </div>
                <div className="context-items-list">
                  {artifacts.map((a) => (
                    <div className="context-item" key={a.path} title={a.path}>
                      <AppIcon name="tasks" className="item-icon" />
                      <span>{a.path}</span>
                    </div>
                  ))}
                </div>
              </section>
            )}

            {usage && (
              <section className="context-block">
                <div className="context-block-title">
                  <strong>{t("agent.usage")}</strong>
                </div>
                <p className="context-usage">
                  {t("agent.tokens", { count: usage.total_tokens })}
                  {usage.estimated ? ` · ${t("agent.estimated")}` : ""}
                </p>
              </section>
            )}

            <section className="context-block timeline-block">
              <div className="context-block-title">
                <strong>{t("agent.timeline")}</strong>
                <span className="count-badge">{events.length}</span>
              </div>
              <div className="timeline-events-list">
                {events.length ? (
                  events
                    .slice(-20)
                    .map((ev) => (
                      <AgentTimelineEvent
                        key={`${ev.run_id}-${ev.sequence}`}
                        event={ev}
                        events={events}
                        language={language}
                        t={t}
                      />
                    ))
                ) : (
                  <p className="muted small-copy">{t("agent.noEvents")}</p>
                )}
              </div>
            </section>
          </div>
        </aside>
      </div>
    </div>
  );
}

function QuestionSetView({ value, t }: { value: Record<string, unknown>; t: Translate }) {
  const questions = Array.isArray(value.questions)
    ? value.questions.filter(
        (q): q is Record<string, unknown> =>
          Boolean(q && typeof q === "object" && !Array.isArray(q))
      )
    : [];
  const [answers, setAnswers] = useState<Record<number, string>>({});
  const [checked, setChecked] = useState(false);

  const score = questions.reduce((total, q, idx) => {
    const answer = q.answer ?? q.correctAnswer;
    return (
      total +
      (answers[idx] && answer !== undefined && String(answers[idx]) === String(answer) ? 1 : 0)
    );
  }, 0);

  if (!questions.length) return null;

  return (
    <section className="agent-structured-result panel">
      <h3>{t("agent.questionSet")}</h3>
      {questions.map((q, idx) => {
        const prompt = String(q.prompt ?? q.question ?? "");
        const options = Array.isArray(q.options) ? q.options.map(String) : [];
        return (
          <div className="question-card" key={`${idx}-${prompt}`}>
            <strong>
              {idx + 1}. {prompt}
            </strong>
            <div className="question-options">
              {options.map((opt) => (
                <button
                  key={opt}
                  className={`button subtle small ${answers[idx] === opt ? "active" : ""}`}
                  onClick={() => {
                    setAnswers((curr) => ({ ...curr, [idx]: opt }));
                    setChecked(false);
                  }}
                >
                  {opt}
                </button>
              ))}
            </div>
            {checked && (
              <p className="small-copy">
                {t("agent.answer")}: {String(q.answer ?? q.correctAnswer ?? "—")}
                {q.explanation ? ` · ${String(q.explanation)}` : ""}
              </p>
            )}
          </div>
        );
      })}
      <div className="result-footer">
        <button className="button primary small" onClick={() => setChecked(true)}>
          {t("agent.checkAnswers")}
        </button>
        {checked && (
          <p className="score-summary">
            {t("agent.score")}: {score} / {questions.length}
          </p>
        )}
      </div>
    </section>
  );
}

function VisualizationView({ value, t }: { value: Record<string, unknown>; t: Translate }) {
  const renderType = String(value.renderType ?? value.render_type ?? "markdown").toLowerCase();
  const content = String(value.content ?? "");
  const safeSvg = renderType === "svg" ? sanitizeMarkup(content, true) : null;
  const safeHtml = renderType === "html" ? sanitizeMarkup(content, false) : null;

  if (renderType === "svg" && safeSvg) {
    return (
      <section className="agent-structured-result panel">
        <h3>{t("agent.visualization")}</h3>
        <iframe title={t("agent.visualization")} sandbox="" srcDoc={svgSandboxDocument(safeSvg)} />
      </section>
    );
  }
  if (renderType === "html" && safeHtml) {
    return (
      <section className="agent-structured-result panel">
        <h3>{t("agent.visualization")}</h3>
        <iframe title={t("agent.visualization")} sandbox="" srcDoc={safeHtml} />
      </section>
    );
  }
  if (renderType === "markdown") {
    return (
      <section className="agent-structured-result panel">
        <h3>{t("agent.visualization")}</h3>
        <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>
          {content}
        </ReactMarkdown>
      </section>
    );
  }
  return (
    <section className="agent-structured-result panel">
      <h3>{t("agent.visualization")}</h3>
      <pre className="agent-visualization-source">
        <code>{content}</code>
      </pre>
    </section>
  );
}
