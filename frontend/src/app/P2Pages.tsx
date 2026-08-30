import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { core } from "../lib/core";
import { useI18n } from "../i18n";
import { useToast } from "../components/Toast";
import { useConfirm } from "../components/ConfirmDialog";
import type {
  AiFeatureCaller, AppSnapshot, CoachAnalysis, CoachChat, CoachConversationMessage, CoachGoal,
  CoachProposal, CoachProposalItem, DailyExamTask, ExamGoal, ExamPlan, ExamQuestion,
  ExamQuestionRecord, ExamRoleAnalysis, ExamSimulation, ExamSimulationEvent, LearningReport,
} from "../types";

type Provider = AppSnapshot["provider"] | undefined;

// AI feature pages receive only the redacted provider status. Secrets stay in
// the host credential store and are never modeled in these React components.
function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) return String(error.message);
  return "Something went wrong.";
}

function providerReady(provider: Provider): boolean {
  // Both Cloud AI and BYOK are valid providers; the UI does not privilege one
  // backend when deciding whether an Agent run can start.
  return Boolean(provider?.cloud_account || provider?.byok_config);
}

type CoachFeatureOutput = {
  conclusion: string;
  rationale: string;
  shouldContinue: boolean;
  decision: string;
  weightedPredicted: number;
  weightedLowerBound: number;
  weightedUpperBound: number;
  successProbability: number;
  predictions: CoachAnalysis["predictions"];
  risks: CoachAnalysis["risks"];
  evidence: CoachAnalysis["evidence"];
  items: CoachProposalItem[];
  alternative: string | null;
};

type PlannerFeatureOutput = Omit<ExamPlan, "id" | "examGoalId" | "createdAt">;
type ExamGradeFeatureOutput = { totalScore: number; analysis: ExamRoleAnalysis; questionResults: Array<{ questionId: string; isCorrect: boolean; score: number; feedback: string }> };

async function runFeature<T>(caller: AiFeatureCaller, context: unknown, provider: Provider): Promise<T> {
  // Core owns the prompt, schema, cache, stale-result, and event lifecycle.
  // React only provides bounded structured context and renders the result.
  if (!providerReady(provider)) throw new Error("Connect Cloud AI or BYOK in Settings before using this feature.");
  const result = await core.runAiFeature<T>({ caller, context });
  return result.output;
}

function saveMessage(goalId: string, chat: CoachChat | undefined, role: string, content: string): { chat: CoachChat; message: CoachConversationMessage } {
  // Chat and message records share one timestamp per append so the local
  // conversation remains ordered even when the model response is immediate.
  const now = new Date().toISOString();
  const nextChat = chat ?? { id: crypto.randomUUID(), goalId, title: "Coach chat", createdAt: now, updatedAt: now };
  return { chat: { ...nextChat, updatedAt: now }, message: { id: crypto.randomUUID(), chatId: nextChat.id, role, content, createdAt: now, todoSuggestions: [] } };
}

export function CoachPage({ provider }: { provider: Provider }) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["coach-data"], queryFn: core.coachData });
  const [title, setTitle] = useState("");
  const [subject, setSubject] = useState("");
  const [baseline, setBaseline] = useState(60);
  const [target, setTarget] = useState(80);
  const [fullScore, setFullScore] = useState(100);
  const [weight, setWeight] = useState(1);
  const [targetDate, setTargetDate] = useState("");
  const [minutes, setMinutes] = useState(60);
  const [purpose, setPurpose] = useState("");
  const [constraints, setConstraints] = useState("");
  const [busy, setBusy] = useState(false);
  const [chatInput, setChatInput] = useState("");
  const data = query.data;
  const goal = data?.goals[0];
  // These are projections of the latest local Coach records, not new model
  // calls; generating or resolving them invalidates `coach-data` below.
  const analysis = data?.analyses.slice().sort((a, b) => b.calculatedAt.localeCompare(a.calculatedAt))[0];
  const proposal = data?.proposals.slice().sort((a, b) => b.createdAt.localeCompare(a.createdAt))[0];
  const chat = goal ? data?.chats.find((value) => value.goalId === goal.id) : undefined;
  const messages = chat ? data?.messages.filter((value) => value.chatId === chat.id) ?? [] : [];

  async function createGoal() {
    if (!title.trim() || !subject.trim() || !targetDate) return;
    const now = new Date().toISOString();
    const value: CoachGoal = { id: crypto.randomUUID(), title: title.trim(), subjects: [{ id: crypto.randomUUID(), subject: subject.trim(), baselineScore: baseline, targetScore: target, fullScore, weight }], examId: null, comprehensiveExamId: null, startDate: now, targetDate: new Date(`${targetDate}T23:59:59`).toISOString(), dailyAvailableMinutes: minutes, purpose, constraints, status: "active", version: 1, createdAt: now, updatedAt: now };
    setBusy(true);
    try { await core.upsertCoachGoal(value); await queryClient.invalidateQueries({ queryKey: ["coach-data"] }); showToast(t("common.saved"), "success"); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function generate() {
    // Core returns a schema-validated preview. Persistence still happens as
    // two local records, while proposal approval remains the write boundary.
    if (!goal) return;
    setBusy(true);
    try {
      const value = await runFeature<CoachFeatureOutput>("Coach", { goal }, provider);
      const now = new Date().toISOString();
      const nextAnalysis: CoachAnalysis = { id: crypto.randomUUID(), goalId: goal.id, goalVersion: goal.version, calculatedAt: now, decision: value.decision, weightedPredicted: value.weightedPredicted, weightedLowerBound: value.weightedLowerBound, weightedUpperBound: value.weightedUpperBound, successProbability: value.successProbability, predictions: value.predictions, risks: value.risks, evidence: value.evidence, dataFingerprint: "core-ai-caller-v1" };
      const nextProposal: CoachProposal = { id: crypto.randomUUID(), goalId: goal.id, goalVersion: goal.version, analysisId: nextAnalysis.id, conclusion: value.conclusion, rationale: value.rationale, items: value.items, status: "pending", createdAt: now, expiresAt: new Date(Date.now() + 7 * 86400000).toISOString(), resolvedAt: null, failureReason: null, alternative: value.alternative };
      await core.upsertCoachAnalysis(nextAnalysis); await core.upsertCoachProposal(nextProposal); await queryClient.invalidateQueries({ queryKey: ["coach-data"] });
      showToast(t("common.saved"), "success");
    } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function resolve(decision: "approve" | "reject") {
    // Resolution carries the goal version so Core can reject stale proposals;
    // approved tasks are refreshed through the shared tasks query as well.
    if (!proposal || !goal) return;
    setBusy(true);
    try { await core.resolveCoachProposal(proposal.id, decision, goal.version); await queryClient.invalidateQueries({ queryKey: ["coach-data"] }); await queryClient.invalidateQueries({ queryKey: ["tasks"] }); showToast(t("common.saved"), "success"); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function sendChat() {
    if (!goal || !chatInput.trim()) return;
    const input = chatInput.trim(); setChatInput(""); setBusy(true);
    // Persist the user turn before Core composes the model context. A failed
    // reply is therefore recoverable and the next request sees the exact
    // locally saved conversation rather than an optimistic React-only turn.
    try { const user = saveMessage(goal.id, chat, "user", input); await core.upsertCoachChat(user.chat); await core.upsertCoachMessage(user.message); const answer = await runFeature<string>("Chat", { goal, message: input }, provider); const assistant = saveMessage(goal.id, user.chat, "assistant", answer); await core.upsertCoachMessage(assistant.message); await queryClient.invalidateQueries({ queryKey: ["coach-data"] }); } catch (error) { await queryClient.invalidateQueries({ queryKey: ["coach-data"] }); showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  if (query.isLoading) return <div className="page-content"><div className="skeleton-card" /><div className="skeleton-card short" /></div>;
  if (query.error) return <div className="page-content"><div className="panel error-card">{errorText(query.error)}</div></div>;
  if (!providerReady(provider)) return <div className="page-content"><section className="panel feature-empty"><h2>{t("coach.title")}</h2><p>{t("feature.providerRequired")}</p></section></div>;
  return <div className="page-content feature-page"><section className="panel"><div className="section-header"><div><h2>{t("coach.title")}</h2><p className="muted">{t("coach.description")}</p></div><span className="status-pill on">{goal ? t("feature.saved") : t("feature.draft")}</span></div><div className="form-grid feature-form"><input value={title} onChange={(event) => setTitle(event.target.value)} placeholder={t("coach.goalTitle")} /><input value={subject} onChange={(event) => setSubject(event.target.value)} placeholder={t("coach.subject")} /><input type="number" value={baseline} onChange={(event) => setBaseline(Number(event.target.value))} placeholder={t("coach.baseline")} /><input type="number" value={target} onChange={(event) => setTarget(Number(event.target.value))} placeholder={t("coach.target")} /><input type="number" value={fullScore} onChange={(event) => setFullScore(Number(event.target.value))} placeholder={t("coach.fullScore")} /><input type="number" min="0" step="0.1" value={weight} onChange={(event) => setWeight(Number(event.target.value))} placeholder={t("coach.weight")} /><input type="number" min="1" value={minutes} onChange={(event) => setMinutes(Number(event.target.value))} placeholder={t("coach.minutes")} /><input type="date" value={targetDate} onChange={(event) => setTargetDate(event.target.value)} /><input value={purpose} onChange={(event) => setPurpose(event.target.value)} placeholder={t("coach.purpose")} /><input value={constraints} onChange={(event) => setConstraints(event.target.value)} placeholder={t("coach.constraints")} /><button className="button primary" onClick={() => void createGoal()} disabled={busy}>{t("coach.saveGoal")}</button></div></section>{goal && <><section className="panel"><div className="section-header"><div><h3>{goal.title}</h3><p className="muted">{goal.subjects.map((value) => `${value.subject} ${value.baselineScore}→${value.targetScore}`).join(" · ")}</p></div><button className="button secondary" onClick={() => void generate()} disabled={busy}>{busy ? t("feature.working") : t("coach.generate")}</button></div>{analysis && <div className="stat-grid"><div className="stat-card accent-sage"><span className="stat-label">{t("coach.prediction")}</span><strong>{analysis.weightedPredicted.toFixed(1)}</strong></div><div className="stat-card accent-gold"><span className="stat-label">{t("coach.probability")}</span><strong>{Math.round(analysis.successProbability * 100)}%</strong></div><div className="stat-card accent-plum"><span className="stat-label">{t("coach.risks")}</span><strong>{analysis.risks.length}</strong></div></div>}{proposal && <div className="proposal-card"><span className="eyebrow">{proposal.status}</span><h3>{proposal.conclusion}</h3><p>{proposal.rationale}</p><div className="compact-list">{proposal.items.map((item) => <div className="compact-row" key={item.id}><div><strong>{item.title}</strong><span>{item.subject} · {item.objective || item.stopCondition}</span></div><span>{item.importance}/5</span></div>)}</div>{proposal.status === "pending" && <div className="button-row"><button className="button subtle" onClick={() => void resolve("reject")} disabled={busy}>{t("coach.reject")}</button><button className="button primary" onClick={() => void resolve("approve")} disabled={busy}>{t("coach.approve")}</button></div>}</div>}</section><section className="panel"><div className="section-header"><div><h3>{t("coach.chat")}</h3><p className="muted">{t("coach.chatDescription")}</p></div></div><div className="chat-history">{messages.map((message) => <div className={`message ${message.role === "user" ? "user" : "assistant"}`} key={message.id}>{message.content}</div>)}</div><div className="inline-form"><input value={chatInput} onChange={(event) => setChatInput(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void sendChat(); }} placeholder={t("coach.chatPlaceholder")} /><button className="button primary small" onClick={() => void sendChat()} disabled={busy}>{t("common.send")}</button></div></section></>}</div>;
}

export function ReversePlannerPage({ provider }: { provider: Provider }) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const confirm = useConfirm();
  const queryClient = useQueryClient();
  const goals = useQuery({ queryKey: ["exam-goals"], queryFn: core.examGoals }); const plans = useQuery({ queryKey: ["exam-plans"], queryFn: core.examPlans });
  const grades = useQuery({ queryKey: ["grades"], queryFn: core.grades }); const mistakes = useQuery({ queryKey: ["mistakes"], queryFn: core.mistakes }); const dueMistakes = useQuery({ queryKey: ["due-mistakes"], queryFn: core.dueMistakes }); const tasks = useQuery({ queryKey: ["tasks"], queryFn: core.tasks });
  const [examName, setExamName] = useState(""); const [subject, setSubject] = useState(""); const [examDate, setExamDate] = useState(""); const [current, setCurrent] = useState(60); const [target, setTarget] = useState(80); const [full, setFull] = useState(100); const [busy, setBusy] = useState(false); const [draftPlan, setDraftPlan] = useState<ExamPlan>();
  // The planner presents the first saved goal and its matching plan; all
  // supporting grades/mistakes/tasks are read-only context for generation.
  const goal = goals.data?.[0]; const plan = plans.data?.find((value) => value.examGoalId === goal?.id); const visiblePlan = draftPlan ?? plan;
  async function generate() {
    // Context is bounded before crossing the Core caller boundary. The result
    // remains a local preview until the explicit Save plan action.
    if (!goal) return; setBusy(true);
    try { const context = { goal, grades: (grades.data ?? []).slice(-15), mistakes: (mistakes.data ?? []).slice(-30).map((value) => ({ subject: value.subject, tags: value.tags, mastery: value.mastery_score })), srsQueue: { overdue: dueMistakes.data?.length ?? 0, total: mistakes.data?.length ?? 0 }, openTasks: (tasks.data ?? []).filter((value) => !value.is_completed).slice(0, 30) }; const value = await runFeature<PlannerFeatureOutput>("ReversePlanner", context, provider); const planValue: ExamPlan = { id: crypto.randomUUID(), examGoalId: goal.id, improvementTarget: value.improvementTarget, summary: value.summary, weakPoints: value.weakPoints, phases: value.phases, dailyTasks: value.dailyTasks, modelInfo: value.modelInfo, createdAt: new Date().toISOString() }; setDraftPlan(planValue); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }
  async function savePlan() { if (!draftPlan) return; setBusy(true); try { await core.upsertExamPlan(draftPlan); setDraftPlan(undefined); await queryClient.invalidateQueries({ queryKey: ["exam-plans"] }); showToast(t("common.saved"), "success"); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); } }
  async function saveGoal() { if (!examName.trim() || !subject.trim() || !examDate) return; setBusy(true); const value: ExamGoal = { id: crypto.randomUUID(), examName: examName.trim(), subject: subject.trim(), examDate: new Date(`${examDate}T09:00:00`).toISOString(), currentScore: current, targetScore: target, fullScore: full, phaseId: null, createdAt: new Date().toISOString() }; try { await core.upsertExamGoal(value); await queryClient.invalidateQueries({ queryKey: ["exam-goals"] }); showToast(t("common.saved"), "success"); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); } }
  async function removeGoal() {
    if (!goal || busy) return;
    setBusy(true);
    try {
      const ok = await confirm({ title: t("planner.delete"), message: t("planner.confirmDelete"), isDestructive: true });
      if (!ok) return;
      await core.deleteExamGoal(goal.id);
      await queryClient.invalidateQueries({ queryKey: ["exam-goals"] });
      showToast(t("common.saved"), "success");
    } catch (error) {
      showToast(errorText(error), "error");
    } finally {
      setBusy(false);
    }
  }
  if (goals.isLoading || plans.isLoading) return <div className="page-content"><div className="skeleton-card" /></div>;
  if (!providerReady(provider)) return <div className="page-content"><section className="panel feature-empty"><h2>{t("planner.title")}</h2><p>{t("feature.providerRequired")}</p></section></div>;
  return <div className="page-content feature-page"><section className="panel"><div className="section-header"><div><h2>{t("planner.title")}</h2><p className="muted">{t("planner.description")}</p></div></div><div className="form-grid feature-form"><input value={examName} onChange={(event) => setExamName(event.target.value)} placeholder={t("planner.examName")} /><input value={subject} onChange={(event) => setSubject(event.target.value)} placeholder={t("coach.subject")} /><input type="date" value={examDate} onChange={(event) => setExamDate(event.target.value)} /><input type="number" value={current} onChange={(event) => setCurrent(Number(event.target.value))} placeholder={t("planner.currentScore")} /><input type="number" value={target} onChange={(event) => setTarget(Number(event.target.value))} placeholder={t("planner.targetScore")} /><input type="number" value={full} onChange={(event) => setFull(Number(event.target.value))} placeholder={t("coach.fullScore")} /><button className="button primary" onClick={() => void saveGoal()} disabled={busy}>{t("planner.saveGoal")}</button></div></section>{goal ? <section className="panel"><div className="section-header"><div><h3>{goal.examName}</h3><p className="muted">{goal.subject} · {goal.currentScore}/{goal.fullScore} → {goal.targetScore}</p></div><div className="button-row"><button className="button subtle" onClick={() => void removeGoal()} disabled={busy}>{t("planner.delete")}</button><button className="button primary" onClick={() => void generate()} disabled={busy}>{busy ? t("feature.working") : t("planner.generate")}</button>{draftPlan && <button className="button secondary" onClick={() => void savePlan()} disabled={busy}>{t("planner.savePlan")}</button>}</div></div>{visiblePlan ? <><p>{visiblePlan.summary}</p><div className="two-column"><div><h4>{t("planner.weakPoints")}</h4><div className="compact-list">{visiblePlan.weakPoints.map((value) => <div className="compact-row" key={value.id}><strong>{value.topic}</strong><span>{Math.round(value.mastery <= 1 ? value.mastery * 100 : value.mastery)}% · +{value.possibleScoreGain}</span></div>)}</div></div><div><h4>{t("planner.dailyTasks")}</h4><div className="compact-list">{visiblePlan.dailyTasks.map((value: DailyExamTask) => <div className="compact-row" key={value.id}><div><strong>{value.taskTitle}</strong><span>{value.date} · {value.subject}</span></div><span>{value.durationMinutes}m</span></div>)}</div></div></div></> : <div className="feature-empty"><p>{t("planner.empty")}</p></div>}</section> : <div className="panel"><div className="feature-empty"><h3>{t("planner.noGoal")}</h3><p>{t("planner.noGoalCopy")}</p></div></div>}</div>;
}

function blankRecord(questionId: string): ExamQuestionRecord { return { questionId, firstViewedAt: null, lastViewedAt: null, totalViewSeconds: 0, visitCount: 0, skipCount: 0, answerChangeCount: 0, firstAnswer: null, finalAnswer: null, submittedAt: null, isCorrect: null, score: null }; }
function event(kind: ExamSimulationEvent["kind"], simulation: ExamSimulation, questionId: string | null, index: number | null, answer: string | null, remainingSeconds: number): ExamSimulationEvent { return { id: crypto.randomUUID(), kind, timestamp: new Date().toISOString(), questionId, questionIndex: index, previousAnswer: null, answer, remainingSeconds }; }

export function ExamSimulationPage({ provider }: { provider: Provider }) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["exam-simulations"], queryFn: core.examSimulations });
  const [subject, setSubject] = useState(""); const [selectedId, setSelectedId] = useState<string>(); const [index, setIndex] = useState(0); const [busy, setBusy] = useState(false); const [remaining, setRemaining] = useState(0);
  const simulation = query.data?.find((value) => value.id === selectedId) ?? query.data?.[0]; const question = simulation?.questions[index]; const record = simulation?.questionRecords.find((value) => value.questionId === question?.id);
  // The timer intentionally follows the persisted simulation snapshot; submit is an action, not timer state.
  // Autosave events update Core, while this effect only derives a display countdown
  // and invokes the existing submit path when the persisted duration reaches zero.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { if (!simulation || simulation.status !== "running" || !simulation.startedAt) return; const tick = () => { const left = Math.max(0, simulation.durationSeconds - Math.floor((Date.now() - Date.parse(simulation.startedAt!)) / 1000)); setRemaining(left); if (left === 0) void submit(true); }; tick(); const timer = window.setInterval(tick, 1000); return () => window.clearInterval(timer); }, [simulation]);
  async function generate() {
    // Core enforces the exact ten-question schema before any generated record
    // is saved to the local simulation draft.
    if (!subject.trim()) return; setBusy(true);
    try { const value = await runFeature<{ questions: ExamQuestion[] }>("ExamSimulation", { kind: "generate", subject: subject.trim() }, provider); const next = await core.newExamSimulation(subject.trim()); const created: ExamSimulation = { ...next, questions: value.questions, questionRecords: value.questions.map((item) => blankRecord(item.id)) }; await core.upsertExamSimulation(created); setSelectedId(created.id); await queryClient.invalidateQueries({ queryKey: ["exam-simulations"] }); showToast(t("common.saved"), "success"); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }
  async function start() { if (!simulation) return; const now = new Date().toISOString(); const next = { ...simulation, status: "running" as const, startedAt: simulation.startedAt ?? now, questionRecords: simulation.questionRecords.length ? simulation.questionRecords : simulation.questions.map((item) => blankRecord(item.id)), events: [...simulation.events, event("started", simulation, null, null, null, simulation.durationSeconds)] }; try { await core.upsertExamSimulation(next); setRemaining(next.durationSeconds); await queryClient.invalidateQueries({ queryKey: ["exam-simulations"] }); } catch (error) { showToast(errorText(error), "error"); } }
  async function answer(value: string) { if (!simulation || !question || simulation.status !== "running") return; const now = new Date().toISOString(); const records = simulation.questionRecords.map((item) => item.questionId === question.id ? { ...item, firstViewedAt: item.firstViewedAt ?? now, lastViewedAt: now, visitCount: Math.max(1, item.visitCount), firstAnswer: item.firstAnswer ?? value, finalAnswer: value, answerChangeCount: item.finalAnswer && item.finalAnswer !== value ? item.answerChangeCount + 1 : item.answerChangeCount } : item); const next = { ...simulation, questionRecords: records, events: [...simulation.events, event("answerChanged", simulation, question.id, index, value, remaining)] }; try { await core.upsertExamSimulation(next); await queryClient.invalidateQueries({ queryKey: ["exam-simulations"] }); } catch (error) { showToast(errorText(error), "error"); } }
  // Submission persists `grading` first, then records either the completed
  // analysis or an `analysisFailed` snapshot so a model failure is recoverable.
  async function submit(timedOut = false) { if (!simulation || busy) return; setBusy(true); const now = new Date().toISOString(); const prepared = { ...simulation, status: "grading" as const, endedAt: now, events: [...simulation.events, event(timedOut ? "timedOut" : "submitted", simulation, question?.id ?? null, question ? index : null, record?.finalAnswer ?? null, remaining)] }; try { await core.upsertExamSimulation(prepared); const value = await runFeature<ExamGradeFeatureOutput>("ExamSimulation", { kind: "grade", simulation: prepared }, provider); const result = { ...prepared, status: "completed" as const, totalScore: value.totalScore, analysis: value.analysis, questionRecords: prepared.questionRecords.map((item) => { const found = value.questionResults.find((row) => row.questionId === item.questionId); return found ? { ...item, isCorrect: found.isCorrect, score: found.score, submittedAt: now } : item; }) }; await core.upsertExamSimulation(result); await queryClient.invalidateQueries({ queryKey: ["exam-simulations"] }); showToast(t("common.saved"), "success"); } catch (error) { const failed = { ...prepared, status: "analysisFailed" as const, lastError: errorText(error) }; try { await core.upsertExamSimulation(failed); } catch { /* preserve the original error */ } showToast(errorText(error), "error"); } finally { setBusy(false); } }
  if (query.isLoading) return <div className="page-content"><div className="skeleton-card" /></div>;
  if (!providerReady(provider)) return <div className="page-content"><section className="panel feature-empty"><h2>{t("simulation.title")}</h2><p>{t("feature.providerRequired")}</p></section></div>;
  return <div className="page-content feature-page"><section className="panel"><div className="section-header"><div><h2>{t("simulation.title")}</h2><p className="muted">{t("simulation.description")}</p></div><div className="inline-form"><input value={subject} onChange={(event) => setSubject(event.target.value)} placeholder={t("coach.subject")} /><button className="button primary" onClick={() => void generate()} disabled={busy}>{busy ? t("feature.working") : t("simulation.generate")}</button></div></div>{query.data?.length ? <div className="compact-list">{query.data.map((value) => <button className={`simulation-row ${simulation?.id === value.id ? "active" : ""}`} key={value.id} onClick={() => { setSelectedId(value.id); setIndex(0); }}>{value.subject}<span>{value.status} · {value.questions.length} {t("simulation.questions")}</span></button>)}</div> : <div className="feature-empty"><p>{t("simulation.empty")}</p></div>}</section>{simulation && <section className="panel simulation-panel"><div className="section-header"><div><h3>{simulation.subject}</h3><p className="muted">{simulation.status} · {simulation.questions.length} {t("simulation.questions")}</p></div><div className="timer-badge">{Math.floor(remaining / 60)}:{String(remaining % 60).padStart(2, "0")}</div></div>{question && simulation.status === "running" ? <div className="question-card"><span className="eyebrow">{index + 1} / {simulation.questions.length} · {question.kind}</span><h3>{question.prompt}</h3>{question.kind === "multipleChoice" ? <div className="option-list">{question.options.map((option) => <button className={`option ${record?.finalAnswer === option ? "selected" : ""}`} key={option} onClick={() => void answer(option)}>{option}</button>)}</div> : <textarea value={record?.finalAnswer ?? ""} onChange={(event) => void answer(event.target.value)} placeholder={t("simulation.answerPlaceholder")} /> }<div className="button-row"><button className="button subtle" onClick={() => setIndex(Math.max(0, index - 1))} disabled={index === 0}>{t("simulation.previous")}</button><button className="button secondary" onClick={() => setIndex(Math.min(simulation.questions.length - 1, index + 1))} disabled={index === simulation.questions.length - 1}>{t("simulation.next")}</button><button className="button primary" onClick={() => void submit()} disabled={busy}>{busy ? t("feature.working") : t("simulation.submit")}</button></div></div> : <div className="feature-empty"><h3>{simulation.status === "completed" ? `${t("simulation.score")}: ${simulation.totalScore ?? "—"}` : t("simulation.resume")}</h3>{simulation.analysis && <p>{simulation.analysis.risk}</p>}<button className="button primary" onClick={() => void start()} disabled={simulation.status === "completed" || simulation.status === "grading" || busy}>{simulation.status === "preparing" ? t("simulation.start") : t("simulation.resume")}</button></div>}</section>}</div>;
}

// Report HTML is built locally, so date values are escaped before interpolation
// while numeric aggregates remain constrained by the typed report DTO.
function escapeHtml(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;"); }
function reportMarkdown(report: LearningReport): string { return `# StudyPulse Report\n\nPeriod: ${report.fromDate} → ${report.toDate}\n\n- Study time: ${report.totalStudyMinutes} minutes\n- Sessions: ${report.sessionCount}\n- Average score rate: ${report.averageScoreRate?.toFixed(1) ?? "—"}%\n- Mistakes: ${report.mistakeCount}\n- Exams: ${report.examCount}\n- Mood: ${report.averageMoodScore?.toFixed(1) ?? "—"}/5\n- Energy: ${report.averageEnergyScore?.toFixed(1) ?? "—"}/5\n\n## Daily trend\n\n| Date | Study minutes | Sessions | Mood | Energy |\n|---|---:|---:|---:|---:|\n${report.dailyStudyMinutes.map((value) => `| ${value.date} | ${value.studyMinutes} | ${value.sessionCount} | ${value.moodScore?.toFixed(1) ?? "—"} | ${value.energyScore?.toFixed(1) ?? "—"} |`).join("\n")}`; }
function reportHtml(report: LearningReport): string { return `<!doctype html><meta charset="utf-8"><title>StudyPulse Report</title><style>body{font:16px system-ui;max-width:900px;margin:40px auto;color:#24332f}h1{font-size:36px}li{margin:8px 0}table{border-collapse:collapse;width:100%}td,th{border-bottom:1px solid #ddd;padding:8px;text-align:left}</style><h1>StudyPulse Report</h1><p>${escapeHtml(report.fromDate)} → ${escapeHtml(report.toDate)}</p><ul><li>Study time: ${report.totalStudyMinutes} minutes</li><li>Sessions: ${report.sessionCount}</li><li>Average score rate: ${report.averageScoreRate?.toFixed(1) ?? "—"}%</li><li>Mistakes: ${report.mistakeCount}</li><li>Exams: ${report.examCount}</li><li>Mood: ${report.averageMoodScore?.toFixed(1) ?? "—"}/5</li><li>Energy: ${report.averageEnergyScore?.toFixed(1) ?? "—"}/5</li></ul><table><thead><tr><th>Date</th><th>Study minutes</th><th>Sessions</th><th>Mood</th><th>Energy</th></tr></thead><tbody>${report.dailyStudyMinutes.map((value) => `<tr><td>${escapeHtml(value.date)}</td><td>${value.studyMinutes}</td><td>${value.sessionCount}</td><td>${value.moodScore?.toFixed(1) ?? "—"}</td><td>${value.energyScore?.toFixed(1) ?? "—"}</td></tr>`).join("")}</tbody></table>`; }

export function ReportsPage() {
  // Report range is part of the query key; changing it fetches a new Core
  // projection without mutating the already-rendered report object.
  const { t } = useI18n();
  const { showToast } = useToast();
  const [range, setRange] = useState(7); const [lastPath, setLastPath] = useState<string>(); const query = useQuery({ queryKey: ["learning-report", range], queryFn: () => core.learningReport(range) }); const report = query.data;
  async function exportText(extension: "md" | "html") {
    if (!report) return;
    try {
      // The host opens the save dialog and owns the destination; a null result
      // means the user cancelled and nothing was written.
      const savedPath = await core.exportReport(extension, `StudyPulse-${range}d`, extension === "md" ? reportMarkdown(report) : reportHtml(report));
      if (!savedPath) return;
      setLastPath(savedPath);
      showToast(t("backup.exported"), "success");
    } catch (error) {
      showToast(errorText(error), "error");
    }
  }
  // PNG export follows a browser-only SVG -> canvas conversion; the file name
  // and destination are decided by the host save dialog, not the renderer.
  async function exportPng() {
    if (!report) return;
    try {
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="650"><rect width="100%" height="100%" fill="#f7f4ec"/><text x="60" y="80" font-family="system-ui" font-size="38" fill="#24332f">StudyPulse ${range}-day report</text><text x="60" y="135" font-family="system-ui" font-size="22" fill="#55736b">${report.fromDate} → ${report.toDate}</text><text x="60" y="210" font-family="system-ui" font-size="28" fill="#24332f">Study ${report.totalStudyMinutes} min · ${report.sessionCount} sessions</text>${report.dailyStudyMinutes.map((value, index) => `<rect x="60" y="${280 + index * 22}" width="${Math.min(800, value.studyMinutes * 4)}" height="14" rx="7" fill="#86a99c"/><text x="875" y="${292 + index * 22}" font-family="system-ui" font-size="12" fill="#55736b">${value.date.slice(5)}</text>`).join("")}</svg>`;
      const dataBase64 = await new Promise<string>((resolve, reject) => {
        const image = new Image();
        image.onerror = () => reject(new Error("Could not render the report image."));
        image.onload = () => {
          try {
            const canvas = document.createElement("canvas");
            canvas.width = 1000;
            canvas.height = 650;
            const context = canvas.getContext("2d");
            if (!context) throw new Error("Could not create a report canvas.");
            context.drawImage(image, 0, 0);
            const encoded = canvas.toDataURL("image/png").split(",")[1];
            if (!encoded) throw new Error("Could not encode the report image.");
            resolve(encoded);
          } catch (error) {
            reject(error);
          }
        };
        image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
      });
      const savedPath = await core.exportReportAsset(`StudyPulse-${range}d`, dataBase64);
      if (!savedPath) return;
      setLastPath(savedPath);
      showToast(t("backup.exported"), "success");
    } catch (error) {
      showToast(errorText(error), "error");
    }
  }

  async function shareReport() {
    if (!lastPath) return;
    try {
      // The host reopens the file it exported itself; no path crosses the wire.
      await core.shareReport();
    } catch (error) {
      showToast(errorText(error), "error");
    }
  }
  if (query.isLoading) return <div className="page-content"><div className="skeleton-card" /></div>; if (query.error) return <div className="page-content"><div className="panel error-card">{errorText(query.error)}</div></div>; if (!report) return null;
  return <div className="page-content reports-page"><section className="panel report-sheet"><div className="section-header"><div><h2>{t("reports.title")}</h2><p className="muted">{report.fromDate} → {report.toDate}</p></div><div className="segmented"><button className={range === 7 ? "active" : ""} onClick={() => setRange(7)}>7d</button><button className={range === 30 ? "active" : ""} onClick={() => setRange(30)}>30d</button></div></div><div className="stat-grid"><div className="stat-card accent-sage"><span className="stat-label">{t("reports.studyTime")}</span><strong>{report.totalStudyMinutes}m</strong></div><div className="stat-card accent-clay"><span className="stat-label">{t("reports.sessions")}</span><strong>{report.sessionCount}</strong></div><div className="stat-card accent-gold"><span className="stat-label">{t("reports.score")}</span><strong>{report.averageScoreRate?.toFixed(1) ?? "—"}%</strong></div><div className="stat-card accent-plum"><span className="stat-label">{t("reports.mood")}</span><strong>{report.averageMoodScore?.toFixed(1) ?? "—"}/5</strong></div></div><div className="report-bars">{report.dailyStudyMinutes.map((value) => <div className="report-bar-row" key={value.date}><span>{value.date.slice(5)}</span><div><i style={{ width: `${Math.min(100, value.studyMinutes / Math.max(1, ...report.dailyStudyMinutes.map((item) => item.studyMinutes)) * 100)}%` }} /></div><strong>{value.studyMinutes}m</strong></div>)}</div><div className="button-row report-actions"><button className="button secondary" onClick={() => void exportText("md")}>Markdown</button><button className="button secondary" onClick={() => void exportText("html")}>HTML</button><button className="button secondary" onClick={() => void exportPng()}>PNG</button><button className="button subtle" onClick={() => window.print()}>PDF / Print</button>{lastPath && <button className="button primary" onClick={() => void shareReport()}>{t("reports.share")}</button>}</div></section></div>;
}
