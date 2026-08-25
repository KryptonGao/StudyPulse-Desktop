import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { core } from "../lib/core";
import { useI18n } from "../i18n";
import type { AppSnapshot, ComprehensiveExam, Exam, Phase3Record } from "../types";

type Provider = AppSnapshot["provider"] | undefined;
type DraftItem = { id: string; title?: string; reason?: string; minutes?: number; priority?: number; task?: unknown; repairSuggestion?: string; questionNumber?: string; question?: string; evidence?: string };

function ready(provider: Provider) { return Boolean(provider?.cloud_account || provider?.byok_config); }
function message(error: unknown) { return error instanceof Error ? error.message : String(error); }
function asItems(payload: Record<string, unknown>): DraftItem[] { return Array.isArray(payload.items) ? payload.items as DraftItem[] : Array.isArray(payload.recommendations) ? payload.recommendations as DraftItem[] : []; }
function record(payload: Record<string, unknown>, status: "draft" | "saved" = "draft"): Phase3Record {
  const now = new Date().toISOString();
  return { id: crypto.randomUUID(), createdAt: now, updatedAt: now, status, payload, appliedActions: {} };
}

function DraftActions({ kind, value, onChanged }: { kind: "suggestions" | "dailyPlans" | "predictions"; value: Phase3Record; onChanged: () => void }) {
  const { t } = useI18n();
  const [chosen, setChosen] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const items = asItems(value.payload).filter((item) => item.task);
  if (!items.length) return null;
  async function apply() {
    if (!chosen.length) return;
    setBusy(true);
    try { await core.applyPhase3TaskActions(kind, value.id, chosen); onChanged(); setChosen([]); }
    catch (error) { window.alert(message(error)); }
    finally { setBusy(false); }
  }
  return <div className="phase3-actions">{items.map((item) => <label key={item.id}><input type="checkbox" checked={chosen.includes(item.id)} onChange={(event) => setChosen((current) => event.target.checked ? [...current, item.id] : current.filter((id) => id !== item.id))} disabled={Boolean(value.appliedActions[item.id])} /> {item.title} {value.appliedActions[item.id] ? `(${t("p3.applied")})` : ""}</label>)}<button className="button primary small" disabled={busy || !chosen.length} onClick={() => void apply()}>{t("p3.createTasks")}</button></div>;
}

export function TodayAiPanel({ provider }: { provider: Provider }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const home = useQuery({ queryKey: ["phase3", "homeAsk"], queryFn: () => core.phase3Records("homeAsk") });
  const suggestions = useQuery({ queryKey: ["phase3", "suggestions"], queryFn: () => core.phase3Records("suggestions") });
  const plans = useQuery({ queryKey: ["phase3", "dailyPlans"], queryFn: () => core.phase3Records("dailyPlans") });
  const [question, setQuestion] = useState(""); const [busy, setBusy] = useState(false);
  const refresh = () => void Promise.all([client.invalidateQueries({ queryKey: ["phase3", "homeAsk"] }), client.invalidateQueries({ queryKey: ["phase3", "suggestions"] }), client.invalidateQueries({ queryKey: ["phase3", "dailyPlans"] }), client.invalidateQueries({ queryKey: ["tasks"] })]);
  const latestHome = useMemo(() => home.data?.slice().sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))[0], [home.data]);
  const latestSuggestions = useMemo(() => suggestions.data?.slice().sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))[0], [suggestions.data]);
  const latestPlan = useMemo(() => plans.data?.slice().sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))[0], [plans.data]);
  async function ask() {
    if (!question.trim()) return; if (!ready(provider)) { window.alert(t("feature.providerRequired")); return; }
    setBusy(true); const active = latestHome ?? record({ messages: [] }, "saved");
    const prior = Array.isArray(active.payload.messages) ? active.payload.messages : [];
    const user = { id: crypto.randomUUID(), role: "user", content: question.trim(), createdAt: new Date().toISOString() };
    try {
      await core.upsertPhase3Record("homeAsk", { ...active, updatedAt: user.createdAt, payload: { ...active.payload, messages: [...prior, user] } });
      const result = await core.runAiFeature<string>({ caller: "HomeAsk", context: { sessionId: active.id, message: user.content } });
      await core.upsertPhase3Record("homeAsk", { ...active, status: "saved", updatedAt: new Date().toISOString(), payload: { ...active.payload, messages: [...prior, user, { id: crypto.randomUUID(), role: "assistant", content: result.output, createdAt: new Date().toISOString() }] } });
      setQuestion(""); refresh();
    } catch (error) { window.alert(message(error)); } finally { setBusy(false); }
  }
  async function generate(kind: "StudySuggestions" | "DailyPlan") {
    if (!ready(provider)) { window.alert(t("feature.providerRequired")); return; } setBusy(true);
    try { const result = await core.runAiFeature<Record<string, unknown>>({ caller: kind, context: { date: new Date().toISOString().slice(0, 10) } }); await core.upsertPhase3Record(kind === "StudySuggestions" ? "suggestions" : "dailyPlans", record(result.output)); refresh(); }
    catch (error) { window.alert(message(error)); } finally { setBusy(false); }
  }
  const chat = Array.isArray(latestHome?.payload.messages) ? latestHome.payload.messages as Array<{ id: string; role: string; content: string }> : [];
  return <section className="panel phase3-panel"><div className="section-header"><div><h2>{t("p3.todayTitle")}</h2><p className="muted">{t("p3.todayCopy")}</p></div></div><div className="phase3-grid"><div><h3>{t("p3.homeAsk")}</h3><div className="phase3-chat">{chat.slice(-6).map((entry) => <p key={entry.id} className={entry.role === "user" ? "phase3-user" : "phase3-assistant"}>{entry.content}</p>)}</div><div className="inline-form"><input value={question} onChange={(event) => setQuestion(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void ask(); }} placeholder={t("p3.askPlaceholder")} /><button className="button primary small" disabled={busy} onClick={() => void ask()}>{t("common.send")}</button></div></div><div><h3>{t("p3.suggestions")}</h3><button className="button subtle small" disabled={busy} onClick={() => void generate("StudySuggestions")}>{t("p3.generate")}</button>{latestSuggestions && <DraftList value={latestSuggestions} />} {latestSuggestions && <DraftActions kind="suggestions" value={latestSuggestions} onChanged={refresh} />}</div><div><h3>{t("p3.dailyPlan")}</h3><button className="button subtle small" disabled={busy} onClick={() => void generate("DailyPlan")}>{t("p3.generate")}</button>{latestPlan && <DraftList value={latestPlan} />} {latestPlan && <DraftActions kind="dailyPlans" value={latestPlan} onChanged={refresh} />}</div></div></section>;
}

function DraftList({ value }: { value: Phase3Record }) { const items = asItems(value.payload); return <div className="compact-list phase3-list">{items.map((item) => <div className="compact-row" key={item.id}><div><strong>{item.title}</strong><span>{item.reason}{item.minutes ? ` · ${item.minutes} min` : ""}</span></div><span className="priority priority-3">{item.priority ?? 3}</span></div>)}</div>; }

export function ExamAiPanel({ provider, exams, comprehensiveExams }: { provider: Provider; exams: Exam[]; comprehensiveExams: ComprehensiveExam[] }) {
  const { t } = useI18n(); const client = useQueryClient();
  const predictions = useQuery({ queryKey: ["phase3", "predictions"], queryFn: () => core.phase3Records("predictions") });
  const autopsies = useQuery({ queryKey: ["phase3", "autopsies"], queryFn: () => core.phase3Records("autopsies") });
  const [examChoice, setExamChoice] = useState(""); const [discussion, setDiscussion] = useState(""); const [files, setFiles] = useState<File[]>([]); const [busy, setBusy] = useState(false);
  const refresh = () => void Promise.all([client.invalidateQueries({ queryKey: ["phase3", "predictions"] }), client.invalidateQueries({ queryKey: ["phase3", "autopsies"] }), client.invalidateQueries({ queryKey: ["tasks"] }), client.invalidateQueries({ queryKey: ["mistakes"] })]);
  const prediction = useMemo(() => predictions.data?.slice().sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))[0], [predictions.data]);
  const autopsy = useMemo(() => autopsies.data?.slice().sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))[0], [autopsies.data]);
  async function predict() {
    if (!examChoice || !ready(provider)) { window.alert(examChoice ? t("feature.providerRequired") : t("p3.selectExam")); return; } setBusy(true);
    const [kind, examId] = examChoice.split(":");
    try { const result = await core.runAiFeature<Record<string, unknown>>({ caller: "ScorePrediction", context: { kind, examId } }); await core.upsertPhase3Record("predictions", record({ ...result.output, kind, examId }, "saved")); refresh(); }
    catch (error) { window.alert(message(error)); } finally { setBusy(false); }
  }
  async function discuss() {
    if (!prediction || !discussion.trim() || !ready(provider)) return; setBusy(true);
    try { const result = await core.runAiFeature<string>({ caller: "PredictionDiscussion", context: { predictionId: prediction.id, message: discussion.trim() } }); const rows = Array.isArray(prediction.payload.discussion) ? prediction.payload.discussion : []; await core.upsertPhase3Record("predictions", { ...prediction, updatedAt: new Date().toISOString(), payload: { ...prediction.payload, discussion: [...rows, { id: crypto.randomUUID(), question: discussion.trim(), answer: result.output }] } }); setDiscussion(""); refresh(); }
    catch (error) { window.alert(message(error)); } finally { setBusy(false); }
  }
  async function autopsyRun() {
    const selected = exams.find((exam) => exam.id === examChoice.replace(/^single:/, ""));
    if (!selected || !ready(provider)) { window.alert(t("p3.selectSingle")); return; } setBusy(true);
    try {
      const attachments = await Promise.all(files.slice(0, 4).map(async (file) => { if (file.size > 8 * 1024 * 1024) throw new Error(t("p3.imageTooLarge")); const encoded = await fileToBase64(file); const path = `images/autopsy-${crypto.randomUUID()}-${file.name.replace(/[^A-Za-z0-9._-]/g, "-")}`; await core.writeMedia(path, encoded); return { kind: "image" as const, sourcePath: path, dataBase64: encoded, mimeType: file.type || "image/png" }; }));
      const result = await core.runAiFeature<Record<string, unknown>>({ caller: "ExamAutopsy", context: { examId: selected.id }, attachments });
      await core.upsertPhase3Record("autopsies", record({ ...result.output, examId: selected.id, subject: selected.subject, imagePaths: attachments.map((item) => item.sourcePath) })); refresh();
    } catch (error) { window.alert(message(error)); } finally { setBusy(false); }
  }
  return <section className="panel phase3-panel"><h2>{t("p3.examTitle")}</h2><p className="muted">{t("p3.examCopy")}</p><div className="inline-form"><select value={examChoice} onChange={(event) => setExamChoice(event.target.value)}><option value="">{t("p3.selectExam")}</option>{exams.map((exam) => <option key={exam.id} value={`single:${exam.id}`}>{exam.name}</option>)}{comprehensiveExams.map((exam) => <option key={exam.id} value={`comprehensive:${exam.id}`}>{exam.name}</option>)}</select><button className="button primary small" disabled={busy} onClick={() => void predict()}>{t("p3.predict")}</button></div>{prediction && <div className="phase3-result"><strong>{t("p3.estimate")}: {String(prediction.payload.pointEstimate ?? "—")}</strong><span>{t("p3.range")}: {String(prediction.payload.lowerBound ?? "—")} – {String(prediction.payload.upperBound ?? "—")}</span><DraftActions kind="predictions" value={{ ...prediction, payload: { ...prediction.payload, items: prediction.payload.recommendations } }} onChanged={refresh} /><div className="inline-form"><input value={discussion} onChange={(event) => setDiscussion(event.target.value)} placeholder={t("p3.discussPlaceholder")} /><button className="button subtle small" disabled={busy} onClick={() => void discuss()}>{t("common.send")}</button></div></div>}<div className="phase3-autopsy"><h3>{t("p3.autopsy")}</h3><input type="file" accept="image/*" multiple onChange={(event) => setFiles(Array.from(event.target.files ?? []).slice(0, 4))} /><button className="button subtle small" disabled={busy} onClick={() => void autopsyRun()}>{t("p3.runAutopsy")}</button>{autopsy && <AutopsyActions value={autopsy} onChanged={refresh} />}</div></section>;
}

function AutopsyActions({ value, onChanged }: { value: Phase3Record; onChanged: () => void }) {
  const { t } = useI18n(); const [mistakes, setMistakes] = useState<string[]>([]); const [tasks, setTasks] = useState<string[]>([]); const [busy, setBusy] = useState(false); const items = asItems(value.payload);
  async function apply() { setBusy(true); try { await core.applyExamAutopsyActions(value.id, mistakes, tasks); setMistakes([]); setTasks([]); onChanged(); } catch (error) { window.alert(message(error)); } finally { setBusy(false); } }
  return <div className="phase3-list">{items.map((item) => <div className="compact-row" key={item.id}><div><strong>{item.questionNumber || t("p3.question")}</strong><span>{item.repairSuggestion || item.evidence}</span></div><label><input type="checkbox" checked={mistakes.includes(item.id)} onChange={(event) => setMistakes((ids) => event.target.checked ? [...ids, item.id] : ids.filter((id) => id !== item.id))} /> {t("p3.importMistake")}</label><label><input type="checkbox" checked={tasks.includes(item.id)} onChange={(event) => setTasks((ids) => event.target.checked ? [...ids, item.id] : ids.filter((id) => id !== item.id))} /> {t("p3.repairTask")}</label></div>)}<button className="button primary small" disabled={busy || (!mistakes.length && !tasks.length)} onClick={() => void apply()}>{t("p3.applySelected")}</button></div>;
}

function fileToBase64(file: File): Promise<string> { return new Promise((resolve, reject) => { const reader = new FileReader(); reader.onerror = () => reject(new Error("Could not read image")); reader.onload = () => resolve(String(reader.result).split(",").at(-1) ?? ""); reader.readAsDataURL(file); }); }
