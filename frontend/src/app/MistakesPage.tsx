import { useState, type ChangeEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { core } from "../lib/core";
import { useI18n } from "../i18n";
import type {
  AppSnapshot,
  MistakeAiSession,
  MistakeAnalysisOutput,
  MistakeDebateOutput,
  MistakeFaultLineOutput,
  MistakeMindMapOutput,
  MistakeNote,
  MistakeOcrOutput,
  MistakePracticeQuestion,
  MistakeQuestionSetOutput,
  MistakeQuizGradeOutput,
  Task,
} from "../types";

type Provider = AppSnapshot["provider"] | undefined;
type EditableMistakeField = "title" | "subject" | "original_question" | "error_reason" | "wrong_solution" | "correct_solution";
type WorkbenchOperation = "analysis" | "questions" | "selfTest" | "mindMap" | "debate" | "faultLine" | "image";
type AiAttachment = { kind: "image"; sourcePath?: string; dataBase64: string; mimeType?: string };
type DebateMessage = { role: "user" | "assistant"; content: string };

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) return String(error.message);
  return "Something went wrong.";
}

function providerReady(provider: Provider): boolean {
  return Boolean(provider?.cloud_account || provider?.byok_config);
}

function emptyMistake(): MistakeNote {
  return {
    id: crypto.randomUUID(), title: "", subject: "", original_question: "", source: "Manual",
    date: new Date().toISOString(), error_reason: "", wrong_solution: "", correct_solution: "",
    question_images: [], reason_images: [], wrong_solution_images: [], correct_solution_images: [],
    review_state: null, phase_id: null, exposure_count: 0, mastery_score: 0, mastery_history: [],
    handwriting_history: [], difficulty: 3, tags: [], audio_file_name: null, extra_json: "{}",
  };
}

function updateTags(value: string): string[] {
  return value.split(",").map((tag) => tag.trim()).filter(Boolean).filter((tag, index, values) => values.indexOf(tag) === index).slice(0, 8);
}

function sessionsFor(mistake: MistakeNote): MistakeAiSession[] {
  try {
    const extra = JSON.parse(mistake.extra_json) as { studypulseAiSessions?: unknown };
    if (!Array.isArray(extra.studypulseAiSessions)) return [];
    return extra.studypulseAiSessions.filter((value): value is MistakeAiSession => {
      if (!value || typeof value !== "object") return false;
      const row = value as Record<string, unknown>;
      return typeof row.id === "string" && typeof row.kind === "string" && typeof row.createdAt === "string" && "payload" in row;
    }).slice().reverse();
  } catch {
    return [];
  }
}

function readFileAsAttachment(file: File): Promise<AiAttachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("The image could not be read."));
    reader.onload = () => {
      const value = String(reader.result ?? "");
      const comma = value.indexOf(",");
      if (comma < 0) {
        reject(new Error("The image encoding is invalid."));
        return;
      }
      resolve({ kind: "image", dataBase64: value.slice(comma + 1), mimeType: file.type || "image/png" });
    };
    reader.readAsDataURL(file);
  });
}

function mediaMime(path: string): string {
  const extension = path.split(".").pop()?.toLowerCase();
  return extension === "jpg" || extension === "jpeg" ? "image/jpeg" : extension === "webp" ? "image/webp" : "image/png";
}

function taskFromRepair(repair: MistakeFaultLineOutput["repairTasks"][number], subject: string): Task {
  const now = new Date();
  const due = new Date(now.getTime() + 86_400_000).toISOString();
  return {
    id: crypto.randomUUID(), title: repair.title, task_type: "Homework", due_date: due, reminder_date: due,
    subject: subject || repair.concept, importance: repair.importance, notes: repair.reason,
    is_completed: false, reminder_event_id: null, reminder_calendar_id: null, created_at: now.toISOString(),
    phase_id: null, coach_execution_data: null, coach_goal_id: null, coach_proposal_id: null, extra_json: "{}",
  };
}

function mindMapDepth(id: string, nodes: MistakeMindMapOutput["nodes"], seen = new Set<string>()): number {
  if (seen.has(id)) return 0;
  const node = nodes.find((value) => value.id === id);
  if (!node?.parentId) return 0;
  const next = new Set(seen);
  next.add(id);
  return Math.min(4, mindMapDepth(node.parentId, nodes, next) + 1);
}

function QuestionCard({
  question,
  answer,
  onAnswer,
  showAnswer,
}: {
  question: MistakePracticeQuestion;
  answer: string;
  onAnswer: (value: string) => void;
  showAnswer: boolean;
}) {
  const { t } = useI18n();
  return <div className="mistake-question-card">
    <div className="mistake-question-meta"><span className="eyebrow">{question.kind === "multipleChoice" ? t("mistakes.multipleChoice") : t("mistakes.fillBlank")} · {t("mistakes.difficulty", { count: question.difficulty })}</span><span>{question.concept}</span></div>
    <p className="mistake-question-prompt">{question.prompt}</p>
    {question.kind === "multipleChoice" ? <div className="option-list">{question.options.map((option) => <button key={option} className={`option ${answer === option ? "selected" : ""}`} onClick={() => onAnswer(option)} type="button">{option}</button>)}</div> : <input value={answer} onChange={(event) => onAnswer(event.target.value)} placeholder={t("mistakes.answerPlaceholder")} />}
    {showAnswer && <div className="mistake-answer-note"><strong>{question.answer}</strong><span>{question.explanation}</span></div>}
  </div>;
}

function MistakeEditor({
  draft,
  editing,
  provider,
  saving,
  busy,
  operation,
  analysis,
  questions,
  answers,
  grade,
  mindMap,
  faultLine,
  debateMessages,
  debateInput,
  ocr,
  attachment,
  imagePreview,
  sessions,
  selectedRepairs,
  onChange,
  onTagsChange,
  onOperation,
  onAnalyze,
  onApplyAnalysis,
  onGenerateQuestions,
  onAnswer,
  onGrade,
  onSaveQuestions,
  onSaveGrade,
  onGenerateMindMap,
  onGenerateFaultLine,
  onToggleRepair,
  onCreateRepairs,
  onDebateInput,
  onSendDebate,
  onSaveDebate,
  onImage,
  onImageRecognition,
  onOcr,
  onApplyOcr,
  onSaveSession,
  onCancel,
  onSave,
}: {
  draft: MistakeNote;
  editing: boolean;
  provider: Provider;
  saving: boolean;
  busy: boolean;
  operation: WorkbenchOperation;
  analysis: MistakeAnalysisOutput | null;
  questions: MistakePracticeQuestion[];
  answers: Record<string, string>;
  grade: MistakeQuizGradeOutput | null;
  mindMap: MistakeMindMapOutput | null;
  faultLine: MistakeFaultLineOutput | null;
  debateMessages: DebateMessage[];
  debateInput: string;
  ocr: MistakeOcrOutput | null;
  attachment: AiAttachment | null;
  imagePreview: string | null;
  sessions: MistakeAiSession[];
  selectedRepairs: Set<string>;
  onChange: (field: EditableMistakeField, value: string) => void;
  onTagsChange: (value: string) => void;
  onOperation: (operation: WorkbenchOperation) => void;
  onAnalyze: () => void;
  onApplyAnalysis: () => void;
  onGenerateQuestions: (kind: "similar_questions" | "self_test_generate") => void;
  onAnswer: (id: string, value: string) => void;
  onGrade: () => void;
  onSaveQuestions: () => void;
  onSaveGrade: () => void;
  onGenerateMindMap: () => void;
  onGenerateFaultLine: () => void;
  onToggleRepair: (id: string) => void;
  onCreateRepairs: () => void;
  onDebateInput: (value: string) => void;
  onSendDebate: () => void;
  onSaveDebate: () => void;
  onImage: (event: ChangeEvent<HTMLInputElement>) => void;
  onImageRecognition: () => void;
  onOcr: () => void;
  onApplyOcr: () => void;
  onSaveSession: (kind: string, payload: unknown) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const { t } = useI18n();
  const aiReady = providerReady(provider);
  return <section className="panel mistake-editor">
    <div className="section-header">
      <div><h2>{editing ? t("mistakes.edit") : t("mistakes.add")}</h2><p className="muted">{t("mistakes.aiDescription")}</p></div>
      <button className="button subtle small" onClick={onCancel} type="button">{t("mistakes.cancel")}</button>
    </div>
    <div className="mistake-editor-grid">
      <label className="mistake-field"><span className="form-label">{t("mistakes.titlePlaceholder")}</span><input value={draft.title} onChange={(event) => onChange("title", event.target.value)} /></label>
      <label className="mistake-field"><span className="form-label">{t("mistakes.subjectPlaceholder")}</span><input value={draft.subject} onChange={(event) => onChange("subject", event.target.value)} /></label>
      <label className="mistake-field full-width"><span className="form-label">{t("mistakes.originalQuestion")}</span><textarea value={draft.original_question} onChange={(event) => onChange("original_question", event.target.value)} /></label>
      <label className="mistake-field"><span className="form-label">{t("mistakes.errorReason")}</span><textarea value={draft.error_reason} onChange={(event) => onChange("error_reason", event.target.value)} /></label>
      <label className="mistake-field"><span className="form-label">{t("mistakes.wrongSolution")}</span><textarea value={draft.wrong_solution} onChange={(event) => onChange("wrong_solution", event.target.value)} /></label>
      <label className="mistake-field full-width"><span className="form-label">{t("mistakes.correctSolution")}</span><textarea value={draft.correct_solution} onChange={(event) => onChange("correct_solution", event.target.value)} /></label>
      <label className="mistake-field full-width"><span className="form-label">{t("mistakes.tags")}</span><input value={draft.tags.join(", ")} onChange={(event) => onTagsChange(event.target.value)} placeholder={t("mistakes.tagsPlaceholder")} /></label>
    </div>
    <div className="mistake-attachment-row">
      <label className="button subtle small file-button"><input type="file" accept="image/*" onChange={onImage} />{t("mistakes.addImage")}</label>
      {imagePreview && <img className="mistake-image-preview" src={imagePreview} alt={t("mistakes.imagePreview")} />}
      {attachment && <span className="muted small-copy">{t("mistakes.imageReady")}</span>}
    </div>
    <div className="mistake-editor-actions form-actions"><span className="muted small-copy">{aiReady ? t("mistakes.aiBoundary") : t("mistakes.aiProviderRequired")}</span><button className="button primary small" disabled={saving} onClick={onSave} type="button">{saving ? t("tasks.saving") : editing ? t("mistakes.saveChanges") : t("mistakes.save")}</button></div>

    <div className="mistake-workbench">
      <div className="mistake-workbench-tabs" role="tablist" aria-label={t("mistakes.workbench")}>{(["analysis", "questions", "selfTest", "mindMap", "debate", "faultLine", "image"] as WorkbenchOperation[]).map((item) => <button key={item} className={operation === item ? "active" : ""} onClick={() => onOperation(item)} type="button">{t(`mistakes.operation.${item}`)}</button>)}</div>
      {!aiReady && <p className="muted mistake-ai-lock">{t("mistakes.aiProviderRequired")}</p>}
      {operation === "analysis" && <div className="mistake-workbench-pane">
        <div className="button-row"><button className="button secondary small" disabled={!aiReady || busy} onClick={onAnalyze} type="button">{busy ? t("mistakes.aiAnalyzing") : t("mistakes.aiAnalyze")}</button>{analysis && <button className="button primary small" disabled={busy} onClick={onApplyAnalysis} type="button">{t("mistakes.applyAndSave")}</button>}</div>
        {analysis ? <div className="mistake-ai-result"><strong>{t("mistakes.aiDraft")}</strong><span>{t("mistakes.aiConfidence", { count: Math.round(analysis.confidence * 100) })}</span><div><small>{t("mistakes.errorReason")}</small><p>{analysis.errorReason}</p><small>{t("mistakes.correctSolution")}</small><p>{analysis.correctSolution}</p>{analysis.wrongSolution && <><small>{t("mistakes.wrongSolution")}</small><p>{analysis.wrongSolution}</p></>}{analysis.tags.length > 0 && <p className="tag-row">{analysis.tags.map((tag) => <span className="tag" key={tag}>{tag}</span>)}</p>}{analysis.evidence.length > 0 && <><small>{t("mistakes.aiEvidence")}</small>{analysis.evidence.map((item) => <p key={item}>{item}</p>)}</>}</div></div> : <p className="muted">{t("mistakes.analysisHint")}</p>}
      </div>}
      {operation === "questions" && <div className="mistake-workbench-pane"><div className="button-row"><button className="button secondary small" disabled={!aiReady || busy} onClick={() => onGenerateQuestions("similar_questions")} type="button">{busy ? t("feature.working") : t("mistakes.generateSimilar")}</button>{questions.length > 0 && <button className="button secondary small" disabled={busy} onClick={onGrade} type="button">{t("mistakes.submitTest")}</button>}{questions.length > 0 && <button className="button primary small" onClick={onSaveQuestions} type="button">{t("mistakes.saveQuestionSet")}</button>}</div>{questions.length ? <div className="mistake-question-list">{questions.map((question) => <QuestionCard key={question.id} question={question} answer={answers[question.id] ?? ""} onAnswer={(value) => onAnswer(question.id, value)} showAnswer={Boolean(grade)} />)}</div> : <p className="muted">{t("mistakes.similarHint")}</p>}{grade && <div className="mistake-grade"><strong>{Math.round(grade.score * 100)}%</strong><span>{grade.correctCount}/{grade.totalCount} · {grade.summary}</span></div>}</div>}
      {operation === "selfTest" && <div className="mistake-workbench-pane"><div className="button-row"><button className="button secondary small" disabled={!aiReady || busy} onClick={() => onGenerateQuestions("self_test_generate")} type="button">{busy ? t("feature.working") : t("mistakes.generateSelfTest")}</button>{questions.length > 0 && <button className="button primary small" disabled={busy} onClick={onGrade} type="button">{t("mistakes.submitTest")}</button>}{grade && <button className="button subtle small" onClick={onSaveGrade} type="button">{t("mistakes.saveTestResult")}</button>}</div>{questions.length ? <div className="mistake-question-list">{questions.map((question) => <QuestionCard key={question.id} question={question} answer={answers[question.id] ?? ""} onAnswer={(value) => onAnswer(question.id, value)} showAnswer={Boolean(grade)} />)}</div> : <p className="muted">{t("mistakes.selfTestHint")}</p>}{grade && <div className="mistake-grade"><strong>{Math.round(grade.score * 100)}%</strong><span>{grade.correctCount}/{grade.totalCount} · {grade.summary}</span>{grade.results.map((result) => <p key={result.questionId}>{result.isCorrect ? "✓" : "○"} {result.feedback}</p>)}</div>}</div>}
      {operation === "mindMap" && <div className="mistake-workbench-pane"><div className="button-row"><button className="button secondary small" disabled={!aiReady || busy} onClick={onGenerateMindMap} type="button">{busy ? t("feature.working") : t("mistakes.generateMindMap")}</button>{mindMap && <button className="button primary small" onClick={() => onSaveSession("mind_map", mindMap)} type="button">{t("mistakes.saveMap")}</button>}</div>{mindMap ? <div className="mind-map"><h3>{mindMap.title}</h3>{mindMap.nodes.map((node) => <div className={`mind-node mind-node-${node.kind}`} key={node.id} style={{ marginLeft: `${mindMapDepth(node.id, mindMap.nodes) * 18}px` }}><strong>{node.label}</strong><span>{node.description}</span></div>)}</div> : <p className="muted">{t("mistakes.mindMapHint")}</p>}</div>}
      {operation === "debate" && <div className="mistake-workbench-pane"><div className="debate-history">{debateMessages.length ? debateMessages.map((message, index) => <div className={`debate-message ${message.role}`} key={`${message.role}-${index}`}><small>{message.role === "user" ? t("mistakes.you") : t("mistakes.tutor")}</small><p>{message.content}</p></div>) : <p className="muted">{t("mistakes.debateHint")}</p>}</div><div className="debate-input"><textarea value={debateInput} onChange={(event) => onDebateInput(event.target.value)} placeholder={t("mistakes.debatePlaceholder")} /><div className="button-row"><button className="button secondary small" disabled={!aiReady || busy || !debateInput.trim()} onClick={onSendDebate} type="button">{busy ? t("feature.working") : t("mistakes.sendDebate")}</button>{debateMessages.length > 0 && <button className="button primary small" onClick={onSaveDebate} type="button">{t("mistakes.saveDebate")}</button>}</div></div></div>}
      {operation === "faultLine" && <div className="mistake-workbench-pane"><div className="button-row"><button className="button secondary small" disabled={!aiReady || busy} onClick={onGenerateFaultLine} type="button">{busy ? t("feature.working") : t("mistakes.findFaultLine")}</button>{faultLine && <button className="button primary small" onClick={onCreateRepairs} type="button">{t("mistakes.createRepairTasks")}</button>}</div>{faultLine ? <div className="fault-line"><p>{faultLine.summary}</p>{faultLine.concepts.map((concept) => <div className="fault-line-row" key={concept.id}><div><strong>{concept.name}</strong><span>{concept.category} · {Math.round(concept.mastery * 100)}%</span></div><p>{concept.evidence.join(" · ")}</p></div>)}{faultLine.repairTasks.map((repair) => <label className="repair-task" key={repair.id}><input type="checkbox" checked={selectedRepairs.has(repair.id)} onChange={() => onToggleRepair(repair.id)} /><span><strong>{repair.title}</strong><small>{repair.reason} · {repair.durationMinutes} min</small></span></label>)}</div> : <p className="muted">{t("mistakes.faultLineHint")}</p>}</div>}
      {operation === "image" && <div className="mistake-workbench-pane"><p className="muted">{t("mistakes.imageHint")}</p><div className="button-row"><button className="button secondary small" disabled={!aiReady || busy || !attachment} onClick={onImageRecognition} type="button">{busy ? t("feature.working") : t("mistakes.recognizeImage")}</button><button className="button secondary small" disabled={!aiReady || busy || !attachment} onClick={onOcr} type="button">{t("mistakes.runOcr")}</button>{ocr && <button className="button primary small" onClick={onApplyOcr} type="button">{t("mistakes.insertOcr")}</button>}</div>{ocr && <div className="ocr-result"><small>{t("mistakes.ocrConfidence", { count: Math.round(ocr.confidence * 100) })}</small><p>{ocr.text}</p></div>}</div>}
    </div>
    {sessions.length > 0 && <details className="mistake-session-history"><summary>{t("mistakes.savedSessions", { count: sessions.length })}</summary>{sessions.slice(0, 6).map((session) => <div key={session.id}><span>{t(`mistakes.operation.${session.kind === "self_test_generate" || session.kind === "similar_questions" ? "questions" : session.kind === "self_test_grade" ? "selfTest" : session.kind}`)}</span><small>{new Date(session.createdAt).toLocaleString()}</small></div>)}</details>}
  </section>;
}

export function MistakesPage({ provider }: { provider: Provider }) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["mistakes"], queryFn: core.mistakes });
  const [draft, setDraft] = useState<MistakeNote | null>(null);
  const [analysis, setAnalysis] = useState<MistakeAnalysisOutput | null>(null);
  const [questions, setQuestions] = useState<MistakePracticeQuestion[]>([]);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [grade, setGrade] = useState<MistakeQuizGradeOutput | null>(null);
  const [mindMap, setMindMap] = useState<MistakeMindMapOutput | null>(null);
  const [faultLine, setFaultLine] = useState<MistakeFaultLineOutput | null>(null);
  const [debateMessages, setDebateMessages] = useState<DebateMessage[]>([]);
  const [debateInput, setDebateInput] = useState("");
  const [ocr, setOcr] = useState<MistakeOcrOutput | null>(null);
  const [attachment, setAttachment] = useState<AiAttachment | null>(null);
  const [imagePreview, setImagePreview] = useState<string | null>(null);
  const [operation, setOperation] = useState<WorkbenchOperation>("analysis");
  const [busy, setBusy] = useState(false);
  const [selectedRepairs, setSelectedRepairs] = useState<Set<string>>(new Set());
  const editing = draft !== null && (query.data ?? []).some((mistake) => mistake.id === draft.id);

  function resetWorkbench() {
    setAnalysis(null); setQuestions([]); setAnswers({}); setGrade(null); setMindMap(null); setFaultLine(null);
    setDebateMessages([]); setDebateInput(""); setOcr(null); setAttachment(null); setImagePreview(null); setSelectedRepairs(new Set()); setOperation("analysis");
  }

  function refreshMistakeQueries() {
    void queryClient.invalidateQueries({ queryKey: ["mistakes"] });
    void queryClient.invalidateQueries({ queryKey: ["flashcards"] });
    void queryClient.invalidateQueries({ queryKey: ["due-mistakes"] });
    void queryClient.invalidateQueries({ queryKey: ["trends"] });
    void queryClient.invalidateQueries({ queryKey: ["today"] });
  }

  const review = useMutation({ mutationFn: ({ id, quality }: { id: string; quality: number }) => core.reviewMistake(id, quality), onSuccess: refreshMistakeQueries, onError: (error) => window.alert(errorText(error)) });
  const enroll = useMutation({ mutationFn: core.enrollMistake, onSuccess: refreshMistakeQueries, onError: (error) => window.alert(errorText(error)) });
  const save = useMutation({ mutationFn: core.upsertMistake, onSuccess: () => { refreshMistakeQueries(); setDraft(null); resetWorkbench(); }, onError: (error) => window.alert(errorText(error)) });

  function openNew() { resetWorkbench(); setDraft(emptyMistake()); }
  function openEdit(mistake: MistakeNote) {
    resetWorkbench(); setDraft({ ...mistake, tags: [...mistake.tags] });
    const path = mistake.question_images[0];
    if (path) void core.readMedia(path).then((data) => { const mime = mediaMime(path); setAttachment({ kind: "image", sourcePath: path, dataBase64: data, mimeType: mime }); setImagePreview(`data:${mime};base64,${data}`); }).catch(() => undefined);
  }
  function updateDraft(field: EditableMistakeField, value: string) { setDraft((current) => current ? { ...current, [field]: value } : current); }

  async function runMistake<T>(kind: string, context: unknown, image?: AiAttachment): Promise<T> {
    if (!providerReady(provider)) throw new Error(t("mistakes.aiProviderRequired"));
    if (!draft) throw new Error(t("mistakes.selectFirst"));
    const result = await core.runAiFeature<T>({ caller: "MistakeAnalysis", context: { kind, ...((context as Record<string, unknown>) ?? {}) }, attachments: image ? [image] : attachment ? [attachment] : [] });
    if (result.diagnostics.stale_result) window.alert(t("mistakes.staleResult"));
    return result.output;
  }

  async function analyze() {
    if (!draft?.original_question.trim()) { window.alert(t("mistakes.aiQuestionRequired")); return; }
    setBusy(true);
    try { setAnalysis(await runMistake<MistakeAnalysisOutput>("analysis", { mistake: mistakeContext(draft) })); } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  async function applyAnalysis() {
    if (!draft || !analysis) return;
    setBusy(true);
    try {
      if (editing) {
        const updated = await core.applyMistakeAiPatch(draft.id, { error_reason: analysis.errorReason, wrong_solution: analysis.wrongSolution, correct_solution: analysis.correctSolution, tags: analysis.tags, question_images: draft.question_images, result: analysis });
        setDraft({ ...updated, original_question: analysis.question || updated.original_question }); refreshMistakeQueries();
      } else {
        setDraft({ ...draft, original_question: analysis.question || draft.original_question, error_reason: analysis.errorReason, wrong_solution: analysis.wrongSolution, correct_solution: analysis.correctSolution, tags: analysis.tags.length ? analysis.tags : draft.tags });
      }
      setAnalysis(null);
    } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  async function generateQuestions(kind: "similar_questions" | "self_test_generate") {
    if (!draft) return;
    setBusy(true); setGrade(null);
    try { const value = await runMistake<MistakeQuestionSetOutput>(kind, { mistake: mistakeContext(draft) }); setQuestions(value.questions); setAnswers({}); setOperation(kind === "similar_questions" ? "questions" : "selfTest"); } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  async function gradeTest() {
    if (!draft || !questions.length) return;
    setBusy(true);
    try {
      const value = await runMistake<MistakeQuizGradeOutput>("self_test_grade", { mistake: mistakeContext(draft), questions, answers: questions.map((question) => ({ questionId: question.id, answer: answers[question.id] ?? "" })) });
      setGrade(value);
    } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  async function generateMindMap() {
    if (!draft) return;
    setBusy(true);
    try { setMindMap(await runMistake<MistakeMindMapOutput>("mind_map", { mistake: mistakeContext(draft) })); } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  async function generateFaultLine() {
    const mistakes = (query.data ?? []).slice(0, 12).map(mistakeContext);
    if (!mistakes.length) return;
    setBusy(true);
    try { setFaultLine(await runMistake<MistakeFaultLineOutput>("fault_line", { mistakes })); setSelectedRepairs(new Set()); } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  async function sendDebate() {
    if (!draft || !debateInput.trim()) return;
    const userText = debateInput.trim();
    const nextMessages = [...debateMessages, { role: "user" as const, content: userText }].slice(-24);
    setDebateInput(""); setBusy(true);
    try { const value = await runMistake<MistakeDebateOutput>("debate", { mistake: mistakeContext(draft), messages: nextMessages }); setDebateMessages([...nextMessages, { role: "assistant", content: value.reply }]); } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  async function handleImage(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) return;
    if (!file.type.startsWith("image/") || file.size > 8 * 1024 * 1024) { window.alert(t("mistakes.imageTooLarge")); return; }
    try {
      const value = await readFileAsAttachment(file);
      const extension = file.type.split("/")[1]?.replace("jpeg", "jpg") || "png";
      const path = `images/${crypto.randomUUID()}.${extension}`;
      await core.writeMedia(path, value.dataBase64);
      setAttachment({ ...value, sourcePath: path });
      setImagePreview(`data:${value.mimeType};base64,${value.dataBase64}`);
      setDraft((current) => current ? { ...current, question_images: [...current.question_images, path] } : current);
    } catch (error) { window.alert(errorText(error)); }
  }

  async function recognizeImage() {
    if (!draft || !attachment) return;
    setBusy(true);
    try { const value = await runMistake<MistakeAnalysisOutput>("image_recognition", { mistake: mistakeContext(draft) }, attachment); setAnalysis(value); setOperation("analysis"); } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  async function runOcr() {
    if (!draft || !attachment) return;
    setBusy(true);
    try { setOcr(await runMistake<MistakeOcrOutput>("ocr", { mistake: mistakeContext(draft) }, attachment)); } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  function saveSession(kind: string, payload: unknown) {
    if (!draft) return;
    if (!editing) { window.alert(t("mistakes.saveBeforeSession")); return; }
    void core.saveMistakeAiSession(draft.id, kind, payload).then(refreshMistakeQueries).catch((error) => window.alert(errorText(error)));
  }

  function saveDraft() {
    if (!draft) return;
    const value: MistakeNote = { ...draft, title: draft.title.trim(), subject: draft.subject.trim(), original_question: draft.original_question.trim(), error_reason: draft.error_reason.trim(), wrong_solution: draft.wrong_solution.trim(), correct_solution: draft.correct_solution.trim(), tags: updateTags(draft.tags.join(",")) };
    if (!value.title) { window.alert(t("mistakes.validation")); return; }
    save.mutate(value);
  }

  async function createRepairs() {
    if (!draft || !faultLine) return;
    const selected = faultLine.repairTasks.filter((repair) => selectedRepairs.has(repair.id));
    if (!selected.length || !window.confirm(t("mistakes.confirmRepairTasks", { count: selected.length }))) return;
    setBusy(true);
    try { for (const repair of selected) await core.upsertTask(taskFromRepair(repair, draft.subject)); await queryClient.invalidateQueries({ queryKey: ["tasks"] }); await core.saveMistakeAiSession(draft.id, "fault_line", { ...faultLine, createdTasks: selected.map((repair) => repair.id) }); } catch (error) { window.alert(errorText(error)); } finally { setBusy(false); }
  }

  if (query.isLoading) return <div className="page-content"><div className="skeleton-card" /><div className="skeleton-card short" /></div>;
  if (query.error) return <div className="page-content"><div className="panel error-card"><strong>{t("error.section")}</strong><p>{errorText(query.error)}</p></div></div>;
  const mistakes = query.data ?? [];
  const sessions = draft ? sessionsFor(draft) : [];
  return <div className="page-content mistakes-page">
    <div className="section-header"><div><h2>{t("mistakes.title")}</h2><p className="muted">{t("mistakes.description")}</p></div><button className="button primary small" onClick={() => { if (draft) { setDraft(null); resetWorkbench(); } else openNew(); }} type="button">{draft ? t("mistakes.close") : t("mistakes.add")}</button></div>
    {draft && <MistakeEditor draft={draft} editing={editing} provider={provider} saving={save.isPending} busy={busy} operation={operation} analysis={analysis} questions={questions} answers={answers} grade={grade} mindMap={mindMap} faultLine={faultLine} debateMessages={debateMessages} debateInput={debateInput} ocr={ocr} attachment={attachment} imagePreview={imagePreview} sessions={sessions} selectedRepairs={selectedRepairs} onChange={updateDraft} onTagsChange={(value) => setDraft((current) => current ? { ...current, tags: updateTags(value) } : current)} onOperation={setOperation} onAnalyze={() => void analyze()} onApplyAnalysis={() => void applyAnalysis()} onGenerateQuestions={(kind) => void generateQuestions(kind)} onAnswer={(id, value) => setAnswers((current) => ({ ...current, [id]: value }))} onGrade={() => void gradeTest()} onSaveQuestions={() => saveSession("similar_questions", { questions, grade })} onSaveGrade={() => grade && saveSession("self_test_grade", { questions, grade })} onGenerateMindMap={() => void generateMindMap()} onGenerateFaultLine={() => void generateFaultLine()} onToggleRepair={(id) => setSelectedRepairs((current) => { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; })} onCreateRepairs={() => void createRepairs()} onDebateInput={setDebateInput} onSendDebate={() => void sendDebate()} onSaveDebate={() => saveSession("debate", { messages: debateMessages })} onImage={handleImage} onImageRecognition={() => void recognizeImage()} onOcr={() => void runOcr()} onApplyOcr={() => { if (ocr) setDraft((current) => current ? { ...current, original_question: current.original_question ? `${current.original_question}\n\n${ocr.text}` : ocr.text } : current); }} onSaveSession={saveSession} onCancel={() => { setDraft(null); resetWorkbench(); }} onSave={saveDraft} />}
    {mistakes.length ? <div className="record-grid">{mistakes.map((mistake) => <article className="record-card mistake-card" key={mistake.id}><div className="mistake-head"><span className="tag">{mistake.subject || t("today.general")}</span><span>{Math.round(mistake.mastery_score * 100)}% {t("mistakes.mastery")}</span></div><h3>{mistake.title || t("mistakes.untitled")}</h3><p>{mistake.original_question || t("mistakes.noQuestion")}</p>{mistake.error_reason && <p className="mistake-summary"><strong>{t("mistakes.errorReason")}</strong>{mistake.error_reason}</p>}<div className="review-actions"><button className="button subtle small" onClick={() => openEdit(mistake)} type="button">{t("mistakes.edit")}</button><button className="button subtle small" disabled={review.isPending} onClick={() => review.mutate({ id: mistake.id, quality: 1 })} type="button">{t("mistakes.again")}</button><button className="button secondary small" disabled={review.isPending} onClick={() => review.mutate({ id: mistake.id, quality: 3 })} type="button">{t("mistakes.hard")}</button><button className="button primary small" disabled={review.isPending} onClick={() => review.mutate({ id: mistake.id, quality: 4 })} type="button">{t("mistakes.gotIt")}</button><button className="button secondary small" disabled={review.isPending} onClick={() => review.mutate({ id: mistake.id, quality: 5 })} type="button">{t("mistakes.easy")}</button>{mistake.review_state === null && <button className="button secondary small" disabled={enroll.isPending} onClick={() => enroll.mutate(mistake.id)} type="button">{t("mistakes.enroll")}</button>}</div></article>)}</div> : <div className="panel"><div className="empty-state"><div className="empty-orb">○</div><h3>{t("mistakes.none")}</h3><p>{t("mistakes.noneCopy")}</p></div></div>}
  </div>;
}

function mistakeContext(mistake: MistakeNote) {
  return { id: mistake.id, title: mistake.title.trim(), subject: mistake.subject.trim(), originalQuestion: mistake.original_question.trim(), errorReason: mistake.error_reason.trim(), wrongSolution: mistake.wrong_solution.trim(), correctSolution: mistake.correct_solution.trim(), tags: mistake.tags.slice(0, 8) };
}
