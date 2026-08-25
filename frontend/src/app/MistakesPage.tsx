import { useState, type ChangeEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { core } from "../lib/core";
import { useI18n } from "../i18n";
import { useToast } from "../components/Toast";
import { useConfirm } from "../components/ConfirmDialog";
import { MathText } from "../components/MathText";
import { AppIcon } from "../components/UIComponents";
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
  return (
    <div className="mistake-question-card">
      <div className="mistake-question-meta">
        <span className="eyebrow">{question.kind === "multipleChoice" ? t("mistakes.multipleChoice") : t("mistakes.fillBlank")} · {t("mistakes.difficulty", { count: question.difficulty })}</span>
        <span>{question.concept}</span>
      </div>
      <div className="mistake-question-prompt">
        <MathText content={question.prompt} />
      </div>
      {question.kind === "multipleChoice" ? (
        <div className="option-list">
          {question.options.map((option) => (
            <button key={option} className={`option ${answer === option ? "selected" : ""}`} onClick={() => onAnswer(option)} type="button">
              <MathText content={option} inline />
            </button>
          ))}
        </div>
      ) : (
        <input value={answer} onChange={(event) => onAnswer(event.target.value)} placeholder={t("mistakes.answerPlaceholder")} />
      )}
      {showAnswer && (
        <div className="mistake-answer-note">
          <strong><MathText content={question.answer} inline /></strong>
          <span><MathText content={question.explanation} /></span>
        </div>
      )}
    </div>
  );
}

function MistakeCard({
  mistake,
  onEdit,
  onReview,
  onEnroll,
  onDelete,
  isReviewPending,
  isEnrollPending,
  isDeletePending,
}: {
  mistake: MistakeNote;
  onEdit: () => void;
  onReview: (quality: number) => void;
  onEnroll: () => void;
  onDelete: () => void;
  isReviewPending: boolean;
  isEnrollPending: boolean;
  isDeletePending: boolean;
}) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const [showReviewChoices, setShowReviewChoices] = useState(false);
  const masteryPercent = Math.round(mistake.mastery_score * 100);

  return (
    <article className="record-card mistake-card">
      <div className="mistake-card-head">
        <div className="mistake-badges">
          <span className="mistake-subject-tag">{mistake.subject || t("today.general")}</span>
          {mistake.tags.slice(0, 2).map((tag) => (
            <span key={tag} className="mistake-meta-tag">{tag}</span>
          ))}
        </div>
        <div className="mistake-mastery-wrap">
          <span className={`mistake-mastery-score ${masteryPercent >= 80 ? "mastery-high" : masteryPercent >= 50 ? "mastery-mid" : "mastery-low"}`}>
            {masteryPercent}%
          </span>
          <span className="mistake-mastery-label">{t("mistakes.mastery")}</span>
        </div>
      </div>

      <h3 className="mistake-title">
        <MathText content={mistake.title || t("mistakes.untitled")} inline />
      </h3>

      <div className={`mistake-body ${expanded ? "expanded" : "clamped"}`}>
        <MathText content={mistake.original_question || t("mistakes.noQuestion")} />
      </div>

      {mistake.original_question && mistake.original_question.length > 90 && (
        <button
          className="mistake-expand-btn"
          onClick={() => setExpanded((prev) => !prev)}
          type="button"
        >
          {expanded ? t("mistakes.collapse") : t("mistakes.expand")}
        </button>
      )}

      {mistake.error_reason && expanded && (
        <div className="mistake-reason-callout">
          <strong className="reason-title">{t("mistakes.errorReason")}</strong>
          <MathText content={mistake.error_reason} />
        </div>
      )}

      <div className="mistake-card-actions">
        <div className="review-action-group">
          <button
            className="button subtle small review-toggle"
            onClick={() => setShowReviewChoices((value) => !value)}
            aria-expanded={showReviewChoices}
            type="button"
          >
            {showReviewChoices ? t("mistakes.hideReview") : t("mistakes.review")}
          </button>
          {showReviewChoices && <div className="rating-pill-group">
          <button
            className="rating-pill rating-again"
            disabled={isReviewPending}
            onClick={() => { onReview(1); setShowReviewChoices(false); }}
            title={t("mistakes.again")}
            type="button"
          >
            {t("mistakes.again")}
          </button>
          <button
            className="rating-pill rating-hard"
            disabled={isReviewPending}
            onClick={() => { onReview(3); setShowReviewChoices(false); }}
            title={t("mistakes.hard")}
            type="button"
          >
            {t("mistakes.hard")}
          </button>
          <button
            className="rating-pill rating-good"
            disabled={isReviewPending}
            onClick={() => { onReview(4); setShowReviewChoices(false); }}
            title={t("mistakes.gotIt")}
            type="button"
          >
            {t("mistakes.gotIt")}
          </button>
          <button
            className="rating-pill rating-easy"
            disabled={isReviewPending}
            onClick={() => { onReview(5); setShowReviewChoices(false); }}
            title={t("mistakes.easy")}
            type="button"
          >
            {t("mistakes.easy")}
          </button>
          </div>}
        </div>

        <div className="secondary-action-group">
          <button className="icon-btn-subtle" onClick={onEdit} title={t("mistakes.edit")} type="button">
            <AppIcon name="planner" className="btn-icon" />
          </button>
          {mistake.review_state === null && (
            <button
              className="button subtle small"
              disabled={isEnrollPending}
              onClick={onEnroll}
              type="button"
            >
              {t("mistakes.enroll")}
            </button>
          )}
          <button
            className="icon-btn-subtle danger-icon"
            onClick={onDelete}
            disabled={isDeletePending}
            title={t("mistakes.delete")}
            aria-label={t("mistakes.delete")}
            type="button"
          >
            <AppIcon name="trash" className="btn-icon" />
          </button>
        </div>
      </div>
    </article>
  );
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
  onOperation: (op: WorkbenchOperation) => void;
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

  return (
    <section className="panel mistake-editor">
      <div className="section-header">
        <div>
          <h2>{editing ? t("mistakes.editTitle") : t("mistakes.newTitle")}</h2>
          <p className="muted">{t("mistakes.editorDescription")}</p>
        </div>
        <div className="button-row">
          <button className="button subtle small" onClick={onCancel} type="button">{t("common.cancel")}</button>
          <button className="button primary small" disabled={saving} onClick={onSave} type="button">{saving ? t("common.saving") : t("common.save")}</button>
        </div>
      </div>

      <div className="form-grid">
        <label>
          <span className="muted">{t("mistakes.fieldTitle")}</span>
          <input value={draft.title} onChange={(event) => onChange("title", event.target.value)} placeholder={t("mistakes.titlePlaceholder")} />
        </label>
        <label>
          <span className="muted">{t("mistakes.fieldSubject")}</span>
          <input value={draft.subject} onChange={(event) => onChange("subject", event.target.value)} placeholder={t("mistakes.subjectPlaceholder")} />
        </label>
        <label>
          <span className="muted">{t("mistakes.fieldTags")}</span>
          <input value={draft.tags.join(", ")} onChange={(event) => onTagsChange(event.target.value)} placeholder={t("mistakes.tagsPlaceholder")} />
        </label>
      </div>

      <div className="form-grid">
        <label>
          <span className="muted">{t("mistakes.originalQuestion")}</span>
          <textarea rows={4} value={draft.original_question} onChange={(event) => onChange("original_question", event.target.value)} placeholder={t("mistakes.questionPlaceholder")} />
        </label>
        <label>
          <span className="muted">{t("mistakes.errorReason")}</span>
          <textarea rows={4} value={draft.error_reason} onChange={(event) => onChange("error_reason", event.target.value)} placeholder={t("mistakes.reasonPlaceholder")} />
        </label>
      </div>

      <div className="form-grid">
        <label>
          <span className="muted">{t("mistakes.wrongSolution")}</span>
          <textarea rows={3} value={draft.wrong_solution} onChange={(event) => onChange("wrong_solution", event.target.value)} placeholder={t("mistakes.wrongSolutionPlaceholder")} />
        </label>
        <label>
          <span className="muted">{t("mistakes.correctSolution")}</span>
          <textarea rows={3} value={draft.correct_solution} onChange={(event) => onChange("correct_solution", event.target.value)} placeholder={t("mistakes.correctSolutionPlaceholder")} />
        </label>
      </div>

      <div className="mistake-workbench">
        <div className="mistake-workbench-tabs">
          <div className="segmented">
            <button className={operation === "analysis" ? "active" : ""} onClick={() => onOperation("analysis")} type="button">{t("mistakes.operation.analysis")}</button>
            <button className={operation === "questions" ? "active" : ""} onClick={() => onOperation("questions")} type="button">{t("mistakes.operation.questions")}</button>
            <button className={operation === "selfTest" ? "active" : ""} onClick={() => onOperation("selfTest")} type="button">{t("mistakes.operation.selfTest")}</button>
            <button className={operation === "mindMap" ? "active" : ""} onClick={() => onOperation("mindMap")} type="button">{t("mistakes.operation.mindMap")}</button>
            <button className={operation === "debate" ? "active" : ""} onClick={() => onOperation("debate")} type="button">{t("mistakes.operation.debate")}</button>
            <button className={operation === "faultLine" ? "active" : ""} onClick={() => onOperation("faultLine")} type="button">{t("mistakes.operation.faultLine")}</button>
            <button className={operation === "image" ? "active" : ""} onClick={() => onOperation("image")} type="button">{t("mistakes.operation.image")}</button>
          </div>
        </div>

        {operation === "analysis" && (
          <div className="mistake-workbench-pane">
            <div className="button-row">
              <button className="button secondary small" disabled={!aiReady || busy} onClick={onAnalyze} type="button">
                {busy ? t("feature.working") : t("mistakes.runAnalysis")}
              </button>
              {analysis && (
                <button className="button primary small" onClick={onApplyAnalysis} type="button">
                  {t("mistakes.applyAnalysis")}
                </button>
              )}
            </div>
            {analysis && (
              <div className="analysis-result">
                <h4>{analysis.tags.join(" · ") || t("mistakes.operation.analysis")} · {Math.round(analysis.confidence * 100)}%</h4>
                <MathText content={analysis.errorReason} />
                <div className="analysis-solutions">
                  <div>
                    <strong>{t("mistakes.wrongSolution")}</strong>
                    <MathText content={analysis.wrongSolution} />
                  </div>
                  <div>
                    <strong>{t("mistakes.correctSolution")}</strong>
                    <MathText content={analysis.correctSolution} />
                  </div>
                </div>
              </div>
            )}
          </div>
        )}

        {operation === "questions" && (
          <div className="mistake-workbench-pane">
            <div className="button-row">
              <button className="button secondary small" disabled={!aiReady || busy} onClick={() => onGenerateQuestions("similar_questions")} type="button">
                {busy ? t("feature.working") : t("mistakes.generateQuestions")}
              </button>
              {questions.length > 0 && (
                <button className="button primary small" onClick={onSaveQuestions} type="button">
                  {t("mistakes.saveQuestions")}
                </button>
              )}
            </div>
            {questions.map((q) => (
              <QuestionCard key={q.id} question={q} answer={answers[q.id] ?? ""} onAnswer={(val) => onAnswer(q.id, val)} showAnswer={true} />
            ))}
          </div>
        )}

        {operation === "selfTest" && (
          <div className="mistake-workbench-pane">
            <div className="button-row">
              <button className="button secondary small" disabled={!aiReady || busy} onClick={() => onGenerateQuestions("self_test_generate")} type="button">
                {busy ? t("feature.working") : t("mistakes.generateSelfTest")}
              </button>
              {questions.length > 0 && (
                <button className="button primary small" disabled={busy} onClick={onGrade} type="button">
                  {busy ? t("feature.working") : t("mistakes.gradeSelfTest")}
                </button>
              )}
              {grade && (
                <button className="button secondary small" onClick={onSaveGrade} type="button">
                  {t("mistakes.saveGrade")}
                </button>
              )}
            </div>
            {grade && (
              <div className="grade-summary">
                <strong>{t("mistakes.score", { count: Math.round(grade.score * 100) })}</strong>
                <p>{grade.summary}</p>
              </div>
            )}
            {questions.map((q) => (
              <QuestionCard key={q.id} question={q} answer={answers[q.id] ?? ""} onAnswer={(val) => onAnswer(q.id, val)} showAnswer={Boolean(grade)} />
            ))}
          </div>
        )}

        {operation === "mindMap" && (
          <div className="mistake-workbench-pane">
            <div className="button-row">
              <button className="button secondary small" disabled={!aiReady || busy} onClick={onGenerateMindMap} type="button">
                {busy ? t("feature.working") : t("mistakes.generateMindMap")}
              </button>
              {mindMap && (
                <button className="button primary small" onClick={() => onSaveSession("mind_map", mindMap)} type="button">
                  {t("mistakes.saveMap")}
                </button>
              )}
            </div>
            {mindMap ? (
              <div className="mind-map">
                <h3>{mindMap.title}</h3>
                {mindMap.nodes.map((node) => (
                  <div className={`mind-node mind-node-${node.kind}`} key={node.id} style={{ marginLeft: `${mindMapDepth(node.id, mindMap.nodes) * 18}px` }}>
                    <strong>{node.label}</strong>
                    <span>{node.description}</span>
                  </div>
                ))}
              </div>
            ) : (
              <p className="muted">{t("mistakes.mindMapHint")}</p>
            )}
          </div>
        )}

        {operation === "debate" && (
          <div className="mistake-workbench-pane">
            <div className="debate-history">
              {debateMessages.length ? (
                debateMessages.map((message, index) => (
                  <div className={`debate-message ${message.role}`} key={`${message.role}-${index}`}>
                    <small>{message.role === "user" ? t("mistakes.you") : t("mistakes.tutor")}</small>
                    <MathText content={message.content} />
                  </div>
                ))
              ) : (
                <p className="muted">{t("mistakes.debateHint")}</p>
              )}
            </div>
            <div className="debate-input">
              <textarea value={debateInput} onChange={(event) => onDebateInput(event.target.value)} placeholder={t("mistakes.debatePlaceholder")} />
              <div className="button-row">
                <button className="button secondary small" disabled={!aiReady || busy || !debateInput.trim()} onClick={onSendDebate} type="button">
                  {busy ? t("feature.working") : t("mistakes.sendDebate")}
                </button>
                {debateMessages.length > 0 && (
                  <button className="button primary small" onClick={onSaveDebate} type="button">
                    {t("mistakes.saveDebate")}
                  </button>
                )}
              </div>
            </div>
          </div>
        )}

        {operation === "faultLine" && (
          <div className="mistake-workbench-pane">
            <div className="button-row">
              <button className="button secondary small" disabled={!aiReady || busy} onClick={onGenerateFaultLine} type="button">
                {busy ? t("feature.working") : t("mistakes.findFaultLine")}
              </button>
              {faultLine && (
                <button className="button primary small" onClick={onCreateRepairs} disabled={busy} type="button">
                  {t("mistakes.createRepairTasks")}
                </button>
              )}
            </div>
            {faultLine ? (
              <div className="fault-line">
                <p>{faultLine.summary}</p>
                {faultLine.concepts.map((concept) => (
                  <div className="fault-line-row" key={concept.id}>
                    <div><strong>{concept.name}</strong><span>{concept.category} · {Math.round(concept.mastery * 100)}%</span></div>
                    <p>{concept.evidence.join(" · ")}</p>
                  </div>
                ))}
                {faultLine.repairTasks.map((repair) => (
                  <label className="repair-task" key={repair.id}>
                    <input type="checkbox" checked={selectedRepairs.has(repair.id)} onChange={() => onToggleRepair(repair.id)} />
                    <span><strong>{repair.title}</strong><small>{repair.reason} · {repair.durationMinutes} min</small></span>
                  </label>
                ))}
              </div>
            ) : (
              <p className="muted">{t("mistakes.faultLineHint")}</p>
            )}
          </div>
        )}

        {operation === "image" && (
          <div className="mistake-workbench-pane">
            <p className="muted">{t("mistakes.imageHint")}</p>
            <input type="file" accept="image/*" onChange={onImage} />
            {imagePreview && <img src={imagePreview} alt="Preview" className="image-preview" />}
            <div className="button-row">
              <button className="button secondary small" disabled={!aiReady || busy || !attachment} onClick={onImageRecognition} type="button">
                {busy ? t("feature.working") : t("mistakes.recognizeImage")}
              </button>
              <button className="button secondary small" disabled={!aiReady || busy || !attachment} onClick={onOcr} type="button">
                {t("mistakes.runOcr")}
              </button>
              {ocr && (
                <button className="button primary small" onClick={onApplyOcr} type="button">
                  {t("mistakes.insertOcr")}
                </button>
              )}
            </div>
            {ocr && (
              <div className="ocr-result">
                <small>{t("mistakes.ocrConfidence", { count: Math.round(ocr.confidence * 100) })}</small>
                <p>{ocr.text}</p>
              </div>
            )}
          </div>
        )}
      </div>

      {sessions.length > 0 && (
        <details className="mistake-session-history">
          <summary>{t("mistakes.savedSessions", { count: sessions.length })}</summary>
          {sessions.slice(0, 6).map((session) => (
            <div key={session.id}>
              <span>{t(`mistakes.operation.${session.kind === "self_test_generate" || session.kind === "similar_questions" ? "questions" : session.kind === "self_test_grade" ? "selfTest" : session.kind}`)}</span>
              <small>{new Date(session.createdAt).toLocaleString()}</small>
            </div>
          ))}
        </details>
      )}
    </section>
  );
}

export function MistakesPage({ provider }: { provider: Provider }) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const confirm = useConfirm();
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

  const review = useMutation({
    mutationFn: ({ id, quality }: { id: string; quality: number }) => core.reviewMistake(id, quality),
    onSuccess: refreshMistakeQueries,
    onError: (error) => showToast(errorText(error), "error"),
  });
  const enroll = useMutation({
    mutationFn: core.enrollMistake,
    onSuccess: refreshMistakeQueries,
    onError: (error) => showToast(errorText(error), "error"),
  });
  const remove = useMutation({
    mutationFn: core.deleteMistake,
    onSuccess: (_, id) => {
      refreshMistakeQueries();
      if (draft?.id === id) {
        setDraft(null);
        resetWorkbench();
      }
    },
    onError: (error) => showToast(errorText(error), "error"),
  });
  const save = useMutation({
    mutationFn: core.upsertMistake,
    onSuccess: () => {
      refreshMistakeQueries();
      setDraft(null);
      resetWorkbench();
      showToast(t("common.saved"), "success");
    },
    onError: (error) => showToast(errorText(error), "error"),
  });

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
    if (result.diagnostics.stale_result) showToast(t("mistakes.staleResult"), "info");
    return result.output;
  }

  async function analyze() {
    if (!draft?.original_question.trim()) { showToast(t("mistakes.aiQuestionRequired"), "error"); return; }
    setBusy(true);
    try { setAnalysis(await runMistake<MistakeAnalysisOutput>("analysis", { mistake: mistakeContext(draft) })); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
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
      showToast(t("common.saved"), "success");
    } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function generateQuestions(kind: "similar_questions" | "self_test_generate") {
    if (!draft) return;
    setBusy(true); setGrade(null);
    try { const value = await runMistake<MistakeQuestionSetOutput>(kind, { mistake: mistakeContext(draft) }); setQuestions(value.questions); setAnswers({}); setOperation(kind === "similar_questions" ? "questions" : "selfTest"); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function gradeTest() {
    if (!draft || !questions.length) return;
    setBusy(true);
    try {
      const value = await runMistake<MistakeQuizGradeOutput>("self_test_grade", { mistake: mistakeContext(draft), questions, answers: questions.map((question) => ({ questionId: question.id, answer: answers[question.id] ?? "" })) });
      setGrade(value);
    } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function generateMindMap() {
    if (!draft) return;
    setBusy(true);
    try { setMindMap(await runMistake<MistakeMindMapOutput>("mind_map", { mistake: mistakeContext(draft) })); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function generateFaultLine() {
    const mistakes = (query.data ?? []).slice(0, 12).map(mistakeContext);
    if (!mistakes.length) return;
    setBusy(true);
    try { setFaultLine(await runMistake<MistakeFaultLineOutput>("fault_line", { mistakes })); setSelectedRepairs(new Set()); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function sendDebate() {
    if (!draft || !debateInput.trim()) return;
    const userText = debateInput.trim();
    const nextMessages = [...debateMessages, { role: "user" as const, content: userText }].slice(-24);
    setDebateInput(""); setBusy(true);
    try { const value = await runMistake<MistakeDebateOutput>("debate", { mistake: mistakeContext(draft), messages: nextMessages }); setDebateMessages([...nextMessages, { role: "assistant", content: value.reply }]); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function handleImage(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) return;
    if (!file.type.startsWith("image/") || file.size > 8 * 1024 * 1024) { showToast(t("mistakes.imageTooLarge"), "error"); return; }
    try {
      const value = await readFileAsAttachment(file);
      const extension = file.type.split("/")[1]?.replace("jpeg", "jpg") || "png";
      const path = `images/${crypto.randomUUID()}.${extension}`;
      await core.writeMedia(path, value.dataBase64);
      setAttachment({ ...value, sourcePath: path });
      setImagePreview(`data:${value.mimeType};base64,${value.dataBase64}`);
      setDraft((current) => current ? { ...current, question_images: [...current.question_images, path] } : current);
    } catch (error) { showToast(errorText(error), "error"); }
  }

  async function recognizeImage() {
    if (!draft || !attachment) return;
    setBusy(true);
    try { const value = await runMistake<MistakeAnalysisOutput>("image_recognition", { mistake: mistakeContext(draft) }, attachment); setAnalysis(value); setOperation("analysis"); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  async function runOcr() {
    if (!draft || !attachment) return;
    setBusy(true);
    try { setOcr(await runMistake<MistakeOcrOutput>("ocr", { mistake: mistakeContext(draft) }, attachment)); } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  function saveSession(kind: string, payload: unknown) {
    if (!draft) return;
    if (!editing) { showToast(t("mistakes.saveBeforeSession"), "error"); return; }
    void core.saveMistakeAiSession(draft.id, kind, payload).then(() => { refreshMistakeQueries(); showToast(t("common.saved"), "success"); }).catch((error) => showToast(errorText(error), "error"));
  }

  function saveDraft() {
    if (!draft) return;
    const value: MistakeNote = { ...draft, title: draft.title.trim(), subject: draft.subject.trim(), original_question: draft.original_question.trim(), error_reason: draft.error_reason.trim(), wrong_solution: draft.wrong_solution.trim(), correct_solution: draft.correct_solution.trim(), tags: updateTags(draft.tags.join(",")) };
    if (!value.title) { showToast(t("mistakes.validation"), "error"); return; }
    save.mutate(value);
  }

  async function deleteMistake(mistake: MistakeNote) {
    try {
      if (await confirm({
        title: t("mistakes.delete"),
        message: t("mistakes.deleteConfirm", { title: mistake.title || t("mistakes.untitled") }),
        isDestructive: true,
      })) {
        remove.mutate(mistake.id);
      }
    } catch (error) {
      showToast(errorText(error), "error");
    }
  }

  async function createRepairs() {
    if (!draft || !faultLine) return;
    const selected = faultLine.repairTasks.filter((repair) => selectedRepairs.has(repair.id));
    if (!selected.length) return;
    setBusy(true);
    try {
      const ok = await confirm({ title: t("mistakes.createRepairTasks"), message: t("mistakes.confirmRepairTasks", { count: selected.length }) });
      if (!ok) return;
      for (const repair of selected) await core.upsertTask(taskFromRepair(repair, draft.subject));
      await queryClient.invalidateQueries({ queryKey: ["tasks"] });
      await core.saveMistakeAiSession(draft.id, "fault_line", { ...faultLine, createdTasks: selected.map((repair) => repair.id) });
      showToast(t("common.saved"), "success");
    } catch (error) { showToast(errorText(error), "error"); } finally { setBusy(false); }
  }

  if (query.isLoading) return <div className="page-content"><div className="skeleton-card" /><div className="skeleton-card short" /></div>;
  if (query.error) return <div className="page-content"><div className="panel error-card"><strong>{t("error.section")}</strong><p>{errorText(query.error)}</p></div></div>;
  const mistakes = query.data ?? [];
  const sessions = draft ? sessionsFor(draft) : [];

  return (
    <div className="page-content mistakes-page">
      <div className="section-header">
        <div>
          <h2>{t("mistakes.title")}</h2>
          <p className="muted">{t("mistakes.description")}</p>
        </div>
        <button
          className="button primary small"
          onClick={() => { if (draft) { setDraft(null); resetWorkbench(); } else openNew(); }}
          type="button"
        >
          {draft ? t("mistakes.close") : t("mistakes.add")}
        </button>
      </div>

      {draft && (
        <MistakeEditor
          draft={draft}
          editing={editing}
          provider={provider}
          saving={save.isPending}
          busy={busy}
          operation={operation}
          analysis={analysis}
          questions={questions}
          answers={answers}
          grade={grade}
          mindMap={mindMap}
          faultLine={faultLine}
          debateMessages={debateMessages}
          debateInput={debateInput}
          ocr={ocr}
          attachment={attachment}
          imagePreview={imagePreview}
          sessions={sessions}
          selectedRepairs={selectedRepairs}
          onChange={updateDraft}
          onTagsChange={(value) => setDraft((current) => current ? { ...current, tags: updateTags(value) } : current)}
          onOperation={setOperation}
          onAnalyze={() => void analyze()}
          onApplyAnalysis={() => void applyAnalysis()}
          onGenerateQuestions={(kind) => void generateQuestions(kind)}
          onAnswer={(id, value) => setAnswers((current) => ({ ...current, [id]: value }))}
          onGrade={() => void gradeTest()}
          onSaveQuestions={() => saveSession("similar_questions", { questions, grade })}
          onSaveGrade={() => grade && saveSession("self_test_grade", { questions, grade })}
          onGenerateMindMap={() => void generateMindMap()}
          onGenerateFaultLine={() => void generateFaultLine()}
          onToggleRepair={(id) => setSelectedRepairs((current) => { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; })}
          onCreateRepairs={() => void createRepairs()}
          onDebateInput={setDebateInput}
          onSendDebate={() => void sendDebate()}
          onSaveDebate={() => saveSession("debate", { messages: debateMessages })}
          onImage={handleImage}
          onImageRecognition={() => void recognizeImage()}
          onOcr={() => void runOcr()}
          onApplyOcr={() => { if (ocr) setDraft((current) => current ? { ...current, original_question: current.original_question ? `${current.original_question}\n\n${ocr.text}` : ocr.text } : current); }}
          onSaveSession={saveSession}
          onCancel={() => { setDraft(null); resetWorkbench(); }}
          onSave={saveDraft}
        />
      )}

      {mistakes.length ? (
        <div className="record-grid mistake-grid">
          {mistakes.map((mistake) => (
            <MistakeCard
              key={mistake.id}
              mistake={mistake}
              onEdit={() => openEdit(mistake)}
               onReview={(quality) => review.mutate({ id: mistake.id, quality })}
               onEnroll={() => enroll.mutate(mistake.id)}
               onDelete={() => void deleteMistake(mistake)}
               isReviewPending={review.isPending}
               isEnrollPending={enroll.isPending}
               isDeletePending={remove.isPending}
             />
          ))}
        </div>
      ) : (
        <div className="panel">
          <div className="empty-state">
            <div className="empty-orb">○</div>
            <h3>{t("mistakes.none")}</h3>
            <p>{t("mistakes.noneCopy")}</p>
          </div>
        </div>
      )}
    </div>
  );
}

function mistakeContext(mistake: MistakeNote) {
  return { id: mistake.id, title: mistake.title.trim(), subject: mistake.subject.trim(), originalQuestion: mistake.original_question.trim(), errorReason: mistake.error_reason.trim(), wrongSolution: mistake.wrong_solution.trim(), correctSolution: mistake.correct_solution.trim(), tags: mistake.tags.slice(0, 8) };
}
