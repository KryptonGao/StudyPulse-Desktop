import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { core } from "../lib/core";
import { localizeEnum, useI18n, type Translate } from "../i18n";
import type { AppSnapshot, ComprehensiveExam, Exam, Grade, SessionIntensity, Task } from "../types";
import {
  EmptyState,
  ErrorCard,
  formatDate,
  formatDuration,
  PageLoading,
  SectionHeader,
  StatusBadge,
} from "../components/UIComponents";
import { useToast } from "../components/Toast";
import { useConfirm } from "../components/ConfirmDialog";
import { TaskRow } from "../components/TaskRow";
import { ExamAiPanel } from "./P3Pages";

function intensityLabel(t: Translate, value: string | null | undefined): string {
  if (!value) return t("timer.focus");
  return localizeEnum(t, "intensity", value === "DeepFocus" ? "deepFocus" : value.toLowerCase());
}

function timerStatusLabel(t: Translate, value: string): string {
  return localizeEnum(t, "timer", value.toLowerCase());
}

/* -------------------------------------------------------------------------- */
/* Tasks Page                                                                 */
/* -------------------------------------------------------------------------- */
export function TasksPage() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const confirm = useConfirm();
  const queryClient = useQueryClient();

  const query = useQuery({ queryKey: ["tasks"], queryFn: core.tasks });
  const [title, setTitle] = useState("");
  const [subject, setSubject] = useState("");
  const [importance, setImportance] = useState(3);

  const mutation = useMutation({
    mutationFn: core.upsertTask,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["tasks"] });
      setTitle("");
      setSubject("");
    },
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  const toggle = useMutation({
    mutationFn: ({ id, completed }: { id: string; completed: boolean }) =>
      core.setTaskCompleted(id, completed),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["tasks"] }),
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  const remove = useMutation({
    mutationFn: core.deleteTask,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["tasks"] }),
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;

  const tasks: Task[] = [...(query.data ?? [])].sort(
    (a, b) => Number(a.is_completed) - Number(b.is_completed) || a.due_date.localeCompare(b.due_date)
  );

  function addTask() {
    const trimmed = title.trim();
    if (!trimmed) {
      showToast(t("tasks.validation"), "error");
      return;
    }
    const now = new Date().toISOString();
    mutation.mutate({
      id: crypto.randomUUID(),
      title: trimmed,
      task_type: "Homework",
      due_date: now,
      reminder_date: now,
      subject: subject.trim(),
      importance,
      notes: "",
      is_completed: false,
      reminder_event_id: null,
      reminder_calendar_id: null,
      created_at: now,
      phase_id: null,
      coach_execution_data: null,
      coach_goal_id: null,
      coach_proposal_id: null,
      extra_json: "{}",
    });
  }

  async function handleRemoveTask(task: Task) {
    try {
      if (await confirm({
        title: t("tasks.delete"),
        message: t("tasks.deleteConfirm", { title: task.title }),
        isDestructive: true,
      })) {
        remove.mutate(task.id);
      }
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  return (
    <div className="page-content">
      <SectionHeader
        title={t("tasks.title")}
        description={t("tasks.description")}
        action={
          <div className="inline-form">
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") addTask();
              }}
              placeholder={t("tasks.addPlaceholder")}
            />
            <input
              value={subject}
              onChange={(e) => setSubject(e.target.value)}
              placeholder={t("subjects.title")}
              style={{ width: "120px" }}
            />
            <select
              value={importance}
              onChange={(e) => setImportance(Number(e.target.value))}
              aria-label={t("exams.importance")}
            >
              <option value={1}>P1</option>
              <option value={2}>P2</option>
              <option value={3}>P3</option>
              <option value={4}>P4</option>
              <option value={5}>P5</option>
            </select>
            <button
              className="button primary small"
              onClick={addTask}
              disabled={mutation.isPending}
            >
              {mutation.isPending ? t("tasks.saving") : t("common.add")}
            </button>
          </div>
        }
      />

      {tasks.length ? (
        <div className="task-list panel">
          {tasks.map((task) => (
            <TaskRow
              key={task.id}
              task={task}
              onToggle={() => toggle.mutate({ id: task.id, completed: !task.is_completed })}
              onDelete={() => void handleRemoveTask(task)}
              disabled={toggle.isPending || remove.isPending}
            />
          ))}
        </div>
      ) : (
        <div className="panel">
          <EmptyState title={t("tasks.noTasks")} copy={t("tasks.noTasksCopy")} />
        </div>
      )}
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Subjects & Grades Page                                                     */
/* -------------------------------------------------------------------------- */
export function SubjectsPage() {
  const { language, t } = useI18n();
  const { showToast } = useToast();
  const queryClient = useQueryClient();

  const subjectsQuery = useQuery({ queryKey: ["subjects"], queryFn: core.subjects });
  const gradesQuery = useQuery({ queryKey: ["grades"], queryFn: core.grades });
  const gradeExamsQuery = useQuery({ queryKey: ["exams"], queryFn: core.exams });

  const [subjectName, setSubjectName] = useState("");
  const [fullScore, setFullScore] = useState(100);
  const [gradeSubject, setGradeSubject] = useState("");
  const [gradeScore, setGradeScore] = useState(0);
  const [gradeFullScore, setGradeFullScore] = useState(100);
  const [gradeExamName, setGradeExamName] = useState("");
  const [gradeExamId, setGradeExamId] = useState("");

  const addSubject = useMutation({
    mutationFn: core.upsertSubject,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["subjects"] });
      setSubjectName("");
      showToast(t("common.saved"), "success");
    },
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  const addGrade = useMutation({
    mutationFn: core.upsertGrade,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["grades"] });
      setGradeExamName("");
      showToast(t("common.saved"), "success");
    },
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  if (subjectsQuery.isLoading || gradesQuery.isLoading || gradeExamsQuery.isLoading)
    return <PageLoading />;
  if (subjectsQuery.error) return <ErrorCard error={subjectsQuery.error} />;
  if (gradesQuery.error) return <ErrorCard error={gradesQuery.error} />;

  const subjects = subjectsQuery.data ?? [];
  const grades = gradesQuery.data ?? [];

  function saveSubject() {
    const name = subjectName.trim();
    if (!name) {
      showToast(t("subjects.validationName"), "error");
      return;
    }
    addSubject.mutate({
      id: crypto.randomUUID(),
      name,
      display_name: name,
      enabled: true,
      full_score: Math.max(1, fullScore),
      extra_json: "{}",
    });
  }

  function saveGrade() {
    const subject = gradeSubject.trim();
    if (!subject) {
      showToast(t("subjects.validationGrade"), "error");
      return;
    }
    const linkedExam = (gradeExamsQuery.data ?? []).find((exam) => exam.id === gradeExamId);
    const value: Grade = {
      id: crypto.randomUUID(),
      subject,
      score: gradeScore,
      raw_score: gradeScore,
      ranking: null,
      importance: 3,
      image_base64: null,
      image_file_name: null,
      date: new Date().toISOString(),
      exam_name: gradeExamName.trim() || linkedExam?.name || "",
      exam_id: gradeExamId || null,
      full_score: Math.max(1, gradeFullScore),
      phase_id: null,
      extra_json: "{}",
    };
    addGrade.mutate(value);
  }

  return (
    <div className="page-content">
      <SectionHeader
        title={t("subjects.title")}
        description={t("subjects.description")}
      />

      <div className="two-column">
        {/* Subjects Column */}
        <section className="panel">
          <SectionHeader
            title={t("subjects.subjectsTitle")}
            description={t("subjects.subjectsDescription")}
            action={<span className="count-badge">{subjects.length}</span>}
          />
          <div className="inline-form stacked">
            <input
              value={subjectName}
              onChange={(e) => setSubjectName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveSubject();
              }}
              placeholder={t("subjects.newSubject")}
            />
            <input
              type="number"
              min="1"
              value={fullScore}
              onChange={(e) => setFullScore(Number(e.target.value))}
              aria-label={t("subjects.fullScore")}
            />
            <button
              className="button primary small"
              onClick={saveSubject}
              disabled={addSubject.isPending}
            >
              {addSubject.isPending ? t("tasks.saving") : t("subjects.addSubject")}
            </button>
          </div>

          {subjects.length ? (
            <div className="compact-list">
              {subjects.map((s) => (
                <div className="compact-row" key={s.id}>
                  <div>
                    <strong>{s.display_name || s.name}</strong>
                    <span>
                      {s.name} · {t("subjects.fullScore")} {s.full_score}
                    </span>
                  </div>
                  <StatusBadge
                    status={s.enabled ? "on" : "off"}
                    label={s.enabled ? t("subjects.active") : t("subjects.off")}
                  />
                </div>
              ))}
            </div>
          ) : (
            <EmptyState title={t("subjects.none")} copy={t("subjects.noneCopy")} />
          )}
        </section>

        {/* Grades Column */}
        <section className="panel">
          <SectionHeader
            title={t("subjects.gradesTitle")}
            description={t("subjects.gradesDescription")}
            action={<span className="count-badge">{grades.length}</span>}
          />
          <div className="form-grid">
            <input
              value={gradeSubject}
              onChange={(e) => setGradeSubject(e.target.value)}
              placeholder={t("subjects.subjectsTitle")}
            />
            <select value={gradeExamId} onChange={(e) => setGradeExamId(e.target.value)}>
              <option value="">{t("subjects.examOptional")}</option>
              {(gradeExamsQuery.data ?? []).map((exam) => (
                <option key={exam.id} value={exam.id}>
                  {exam.name}
                </option>
              ))}
            </select>
            <input
              value={gradeExamName}
              onChange={(e) => setGradeExamName(e.target.value)}
              placeholder={t("subjects.examOptional")}
            />
            <input
              type="number"
              min="0"
              value={gradeScore}
              onChange={(e) => setGradeScore(Number(e.target.value))}
              placeholder={t("subjects.score")}
            />
            <input
              type="number"
              min="1"
              value={gradeFullScore}
              onChange={(e) => setGradeFullScore(Number(e.target.value))}
              placeholder={t("subjects.fullScore")}
            />
            <button
              className="button primary small"
              onClick={saveGrade}
              disabled={addGrade.isPending}
            >
              {addGrade.isPending ? t("tasks.saving") : t("subjects.addGrade")}
            </button>
          </div>

          {grades.length ? (
            <div className="compact-list">
              {grades
                .slice()
                .reverse()
                .map((grade) => (
                  <div className="compact-row" key={grade.id}>
                    <div>
                      <strong>{grade.subject}</strong>
                      <span>
                        {grade.exam_name || t("subjects.grade")} · {grade.score}/
                        {grade.full_score ?? "—"}
                      </span>
                    </div>
                    <span>{formatDate(grade.date, language)}</span>
                  </div>
                ))}
            </div>
          ) : (
            <EmptyState title={t("subjects.noneGrades")} copy={t("subjects.noneGradesCopy")} />
          )}
        </section>
      </div>
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Exams Page                                                                 */
/* -------------------------------------------------------------------------- */
export function ExamsPage({ provider }: { provider: AppSnapshot["provider"] | undefined }) {
  const { language, t } = useI18n();
  const { showToast } = useToast();
  const confirm = useConfirm();
  const queryClient = useQueryClient();

  const query = useQuery({ queryKey: ["exams"], queryFn: core.exams });
  const comprehensiveQuery = useQuery({
    queryKey: ["comprehensive-exams"],
    queryFn: core.comprehensiveExams,
  });
  const subjectsQuery = useQuery({ queryKey: ["subjects"], queryFn: core.subjects });

  const [name, setName] = useState("");
  const [subject, setSubject] = useState("");
  const [examDate, setExamDate] = useState("");
  const [importance, setImportance] = useState(3);
  const [comprehensiveName, setComprehensiveName] = useState("");
  const [comprehensiveDate, setComprehensiveDate] = useState("");
  const [comprehensiveSubjects, setComprehensiveSubjects] = useState("");

  const mutation = useMutation({
    mutationFn: core.upsertExam,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["exams"] });
      setName("");
      setExamDate("");
      showToast(t("common.saved"), "success");
    },
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  const remove = useMutation({
    mutationFn: core.deleteExam,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["exams"] }),
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  const comprehensiveMutation = useMutation({
    mutationFn: core.upsertComprehensiveExam,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["comprehensive-exams"] });
      setComprehensiveName("");
      setComprehensiveDate("");
      showToast(t("common.saved"), "success");
    },
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  const comprehensiveRemove = useMutation({
    mutationFn: core.deleteComprehensiveExam,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["comprehensive-exams"] }),
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  if (query.isLoading || comprehensiveQuery.isLoading || subjectsQuery.isLoading)
    return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;

  const exams = query.data ?? [];
  const comprehensiveExams = comprehensiveQuery.data ?? [];

  function saveExam() {
    const trimmed = name.trim();
    if (!trimmed) {
      showToast(t("exams.validationName"), "error");
      return;
    }
    if (!examDate) {
      showToast(t("exams.validationDate"), "error");
      return;
    }
    const value: Exam = {
      id: crypto.randomUUID(),
      name: trimmed,
      exam_date: new Date(`${examDate}T09:00:00`).toISOString(),
      exam_end_date: null,
      importance,
      subject: subject.trim(),
      exam_name: trimmed,
      mastery_degree: 0,
      time_slot: null,
      phase_id: null,
      checklist: [],
      location_school: "",
      location_classroom: "",
      location_seat: "",
      countdown_notify_days: [7, 1],
      exam_review: null,
      extra_json: "{}",
    };
    mutation.mutate(value);
  }

  function saveComprehensiveExam() {
    if (!comprehensiveName.trim()) {
      showToast(t("exams.validationName"), "error");
      return;
    }
    if (!comprehensiveDate) {
      showToast(t("exams.validationDate"), "error");
      return;
    }
    const value: ComprehensiveExam = {
      id: crypto.randomUUID(),
      name: comprehensiveName.trim(),
      exam_date: new Date(`${comprehensiveDate}T09:00:00`).toISOString(),
      exam_end_date: null,
      importance: 3,
      subject: comprehensiveSubjects
        .split(",")
        .map((i) => i.trim())
        .filter(Boolean),
      exam_name: comprehensiveName.trim(),
      mastery_degree: 0,
      subject_time_slots: null,
      phase_id: null,
      extra_json: "{}",
    };
    comprehensiveMutation.mutate(value);
  }

  const handleRemoveExam = async (id: string, examName: string) => {
    try {
      const ok = await confirm({
        title: t("exams.remove"),
        message: `${examName}?`,
        isDestructive: true,
      });
      if (ok) remove.mutate(id);
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  };

  const handleRemoveComprehensiveExam = async (id: string, examName: string) => {
    try {
      const ok = await confirm({
        title: t("exams.remove"),
        message: `${examName}?`,
        isDestructive: true,
      });
      if (ok) comprehensiveRemove.mutate(id);
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  };

  return (
    <div className="page-content">
      <SectionHeader
        title={t("exams.title")}
        description={t("exams.description")}
        action={
          <div className="inline-form">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("exams.name")}
            />
            <select value={subject} onChange={(e) => setSubject(e.target.value)}>
              <option value="">{t("exams.subject")}</option>
              {(subjectsQuery.data ?? []).map((s) => (
                <option key={s.id} value={s.name}>
                  {s.display_name || s.name}
                </option>
              ))}
            </select>
            <input
              type="date"
              value={examDate}
              onChange={(e) => setExamDate(e.target.value)}
            />
            <input
              type="number"
              min="1"
              max="5"
              value={importance}
              onChange={(e) => setImportance(Number(e.target.value))}
              aria-label={t("exams.importance")}
            />
            <button
              className="button primary small"
              onClick={saveExam}
              disabled={mutation.isPending}
            >
              {mutation.isPending ? t("tasks.saving") : t("exams.add")}
            </button>
          </div>
        }
      />

      {exams.length ? (
        <div className="record-grid">
          {exams
            .slice()
            .sort((a, b) => a.exam_date.localeCompare(b.exam_date))
            .map((exam) => (
              <div className="record-card" key={exam.id}>
                <div className="record-index">{formatDate(exam.exam_date, language)}</div>
                <h3>{exam.name || exam.exam_name}</h3>
                <div className="record-field">
                  <span>{t("exams.subject")}</span>
                  <strong>{exam.subject || t("today.general")}</strong>
                </div>
                <div className="record-field">
                  <span>{t("exams.importance")}</span>
                  <strong>P{exam.importance}</strong>
                </div>
                <button
                  className="button subtle small"
                  onClick={() => void handleRemoveExam(exam.id, exam.name || exam.exam_name)}
                  disabled={remove.isPending}
                >
                  {t("exams.remove")}
                </button>
              </div>
            ))}
        </div>
      ) : (
        <div className="panel">
          <EmptyState title={t("exams.none")} copy={t("exams.noneCopy")} />
        </div>
      )}

      {/* Comprehensive Exams Section */}
      <section className="panel comprehensive-panel">
        <SectionHeader
          title={t("exams.comprehensive")}
          description={t("exams.comprehensiveDescription")}
        />
        <div className="inline-form">
          <input
            value={comprehensiveName}
            onChange={(e) => setComprehensiveName(e.target.value)}
            placeholder={t("exams.name")}
          />
          <input
            value={comprehensiveSubjects}
            onChange={(e) => setComprehensiveSubjects(e.target.value)}
            placeholder={t("exams.subjectsComma")}
          />
          <input
            type="date"
            value={comprehensiveDate}
            onChange={(e) => setComprehensiveDate(e.target.value)}
          />
          <button
            className="button primary small"
            onClick={saveComprehensiveExam}
            disabled={comprehensiveMutation.isPending}
          >
            {t("exams.add")}
          </button>
        </div>

        {comprehensiveExams.length ? (
          <div className="compact-list">
            {comprehensiveExams.map((exam) => (
              <div className="compact-row" key={exam.id}>
                <div>
                  <strong>{exam.name}</strong>
                  <span>
                    {formatDate(exam.exam_date, language)} ·{" "}
                    {exam.subject.join(", ") || t("today.general")}
                  </span>
                </div>
                <button
                  className="button subtle small"
                  onClick={() => void handleRemoveComprehensiveExam(exam.id, exam.name)}
                  disabled={comprehensiveRemove.isPending}
                >
                  {t("exams.remove")}
                </button>
              </div>
            ))}
          </div>
        ) : (
          <p className="muted small-copy">{t("exams.comprehensiveEmpty")}</p>
        )}
      </section>

      {/* Phase 3 Exam AI */}
      <ExamAiPanel provider={provider} exams={exams} comprehensiveExams={comprehensiveExams} />
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Timer Page                                                                 */
/* -------------------------------------------------------------------------- */
export function TimerPage() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const queryClient = useQueryClient();

  const query = useQuery({ queryKey: ["timer"], queryFn: core.timer, refetchInterval: 1000 });
  const [intensity, setIntensity] = useState<SessionIntensity>("DeepFocus");
  const [minutes, setMinutes] = useState(25);
  const [busy, setBusy] = useState(false);

  const mutate = async (action: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    try {
      await action();
      await queryClient.invalidateQueries({ queryKey: ["timer"] });
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;
  const timer = query.data!;
  const remaining = Math.max(0, timer.target_duration_seconds - timer.elapsed_seconds);

  return (
    <div className="page-content timer-page">
      <SectionHeader
        title={t("timer.title")}
        description={t("timer.description")}
      />

      <div className="timer-card panel">
        <div className={`timer-ring ${timer.status.toLowerCase()}`}>
          <span className="timer-digits">
            {timer.status === "Idle"
              ? `${minutes}:00`
              : `${String(Math.floor(remaining / 60)).padStart(2, "0")}:${String(
                  remaining % 60
                ).padStart(2, "0")}`}
          </span>
          <small className="timer-status-tag">{timerStatusLabel(t, timer.status)}</small>
        </div>

        <div className="timer-controls">
          {timer.status === "Idle" ? (
            <>
              <label>
                {t("timer.minutes")}
                <input
                  type="number"
                  min="1"
                  max="240"
                  value={minutes}
                  onChange={(e) => setMinutes(Math.max(1, Number(e.target.value)))}
                />
              </label>
              <label>
                {t("timer.intensity")}
                <select
                  value={intensity}
                  onChange={(e) => setIntensity(e.target.value as SessionIntensity)}
                >
                  <option value="Peak">{t("intensity.peak")}</option>
                  <option value="DeepFocus">{t("intensity.deepFocus")}</option>
                  <option value="Steady">{t("intensity.steady")}</option>
                  <option value="Light">{t("intensity.light")}</option>
                  <option value="Recovery">{t("intensity.recovery")}</option>
                </select>
              </label>
              <button
                className="button primary"
                onClick={() => void mutate(() => core.startTimer(intensity, minutes * 60))}
                disabled={busy}
              >
                {t("timer.start")}
              </button>
            </>
          ) : (
            <>
              <div className="timer-meta">
                <span>{intensityLabel(t, timer.intensity)}</span>
                <span>
                  {t("timer.elapsed", { duration: formatDuration(timer.elapsed_seconds, t) })}
                </span>
              </div>
              <div className="timer-running-actions">
                {timer.status === "Running" ? (
                  <button className="button secondary" onClick={() => void mutate(core.pauseTimer)} disabled={busy}>
                    {t("timer.pause")}
                  </button>
                ) : (
                  <button className="button primary" onClick={() => void mutate(core.resumeTimer)} disabled={busy}>
                    {t("timer.resume")}
                  </button>
                )}
                <button className="button subtle" onClick={() => void mutate(core.finishTimer)} disabled={busy}>
                  {t("timer.finish")}
                </button>
                <button className="button danger" onClick={() => void mutate(core.cancelTimer)} disabled={busy}>
                  {t("timer.cancel")}
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
