import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import ReactMarkdown from "react-markdown";
import rehypeSanitize from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import { core, chooseBackupExportPath, chooseBackupToInspect, chooseDirectory, chooseSourceFiles, chooseWorkspaceToCreate, isDesktop } from "../lib/core";
import { languageLocale, localizeEnum, useI18n, type Translate } from "../i18n";
import type { AgentEvent, AgentEventKind, AgentMessage, AgentMode, AgentNotebook, AppSnapshot, Exam, Grade, SessionIntensity, TimeInvestmentSubject } from "../types";

type Page = "today" | "agent" | "tasks" | "subjects" | "exams" | "mistakes" | "timer" | "investment" | "library" | "settings";

const navigation: { id: Exclude<Page, "settings">; labelKey: string; icon: string }[] = [
  { id: "today", labelKey: "nav.today", icon: "✦" },
  { id: "agent", labelKey: "nav.agent", icon: "⌁" },
  { id: "tasks", labelKey: "nav.tasks", icon: "☑" },
  { id: "subjects", labelKey: "nav.subjects", icon: "◫" },
  { id: "exams", labelKey: "nav.exams", icon: "◷" },
  { id: "mistakes", labelKey: "nav.mistakes", icon: "!" },
  { id: "timer", labelKey: "nav.timer", icon: "◴" },
  { id: "investment", labelKey: "nav.investment", icon: "▥" },
  { id: "library", labelKey: "nav.library", icon: "▤" },
];

function formatDate(value: string | null | undefined, language: string): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleDateString(language === "en" ? "en-US" : language, { month: "short", day: "numeric" });
}

function formatDuration(seconds: number, t: Translate): string {
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  return hours ? `${t("duration.hours", { count: hours })} ${t("duration.minutes", { count: minutes % 60 })}` : t("duration.minutes", { count: minutes });
}

function errorMessage(error: unknown, t: Translate): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) return String(error.message);
  return error instanceof Error ? error.message : t("error.generic");
}

function pageLabel(t: Translate, page: Page): string {
  return page === "settings" ? t("nav.settings") : t(navigation.find((item) => item.id === page)?.labelKey ?? "nav.settings");
}

function taskTypeLabel(t: Translate, value: string): string {
  return value === "Homework" ? t("taskType.homework") : value === "Reading" ? t("taskType.reading") : value;
}

function intensityLabel(t: Translate, value: string | null | undefined): string {
  if (!value) return t("timer.focus");
  return localizeEnum(t, "intensity", value === "DeepFocus" ? "deepFocus" : value.toLowerCase());
}

function timerStatusLabel(t: Translate, value: string): string {
  return localizeEnum(t, "timer", value.toLowerCase());
}

function themeLabel(t: Translate, value: string): string {
  return localizeEnum(t, "theme", value.toLowerCase());
}

function modeLabel(t: Translate, value: string): string {
  const key = value === "DeepSolve" ? "deepSolve" : value === "DeepResearch" ? "deepResearch" : value === "QuestionLab" ? "questionLab" : value.toLowerCase();
  return localizeEnum(t, "mode", key);
}

function eventLabel(t: Translate, value: AgentEventKind): string {
  const key = value.replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase().replaceAll("_", "");
  const keyMap: Record<string, string> = {
    started: "started", statuschanged: "statusChanged", textdelta: "textDelta", toolrequested: "toolRequested", toolcompleted: "toolCompleted", confirmationrequired: "confirmationRequired", stagestarted: "stageStarted", stageprogress: "stageProgress", stagecompleted: "stageCompleted", inputrequired: "inputRequired", artifactcreated: "artifactCreated", failed: "failed", cancelled: "cancelled", completed: "completed",
  };
  return t(`event.${keyMap[key] ?? ""}`) === `event.${keyMap[key] ?? ""}` ? value.replaceAll("_", " ") : t(`event.${keyMap[key]}`);
}

export default function App() {
  const { language, t } = useI18n();
  const queryClient = useQueryClient();
  const [page, setPage] = useState<Page>("today");
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const snapshot = useQuery({ queryKey: ["snapshot"], queryFn: core.snapshot, enabled: isDesktop });
  const workspace = snapshot.data?.workspace ?? null;
  const refresh = useCallback(() => queryClient.invalidateQueries(), [queryClient]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.lang = language;
  }, [language, theme]);

  useEffect(() => {
    if (!isDesktop) return;
    void core.restoreAi().then(() => queryClient.invalidateQueries({ queryKey: ["snapshot"] })).catch(() => undefined);
    let dispose: (() => void) | undefined;
    void onOpenUrl((urls) => {
      const callback = urls[0];
      if (callback) void core.completeCloudAuth(callback).then(refresh).catch((error) => window.alert(errorMessage(error, t)));
    }).then((unlisten) => { dispose = unlisten; });
    return () => dispose?.();
  }, [queryClient, refresh, t]);

  async function openExistingWorkspace() {
    try {
      const path = await chooseDirectory(t("dialog.openWorkspace"));
      if (!path) return;
      await core.openWorkspace(path);
      await refresh();
      setPage("today");
    } catch (error) { window.alert(errorMessage(error, t)); }
  }

  async function createWorkspace() {
    try {
      const path = await chooseWorkspaceToCreate(t("dialog.createWorkspace"));
      if (!path) return;
      await core.createWorkspace(path);
      await refresh();
      setPage("today");
    } catch (error) { window.alert(errorMessage(error, t)); }
  }

  async function importBackup() {
    const path = await chooseBackupToInspect(t("dialog.inspectBackup"));
    if (!path) return;
    try {
      const inspection = await core.inspectBackup(path);
      const conflictText = inspection.conflicts.length ? `\n${t("backup.conflicts", { count: inspection.conflicts.length })}` : "";
      const shouldApply = window.confirm(`${t("backup.ready", { schema: inspection.schema_version, records: inspection.added_records })}${conflictText}\n\n${t("backup.applyReplace")}`);
      if (shouldApply) {
        await core.applyBackup(inspection.id, "Replace");
        await refresh();
      }
    } catch (error) {
      window.alert(errorMessage(error, t));
    }
  }

  async function exportBackup() {
    const path = await chooseBackupExportPath(t("dialog.exportBackup"));
    if (!path) return;
    try {
      await core.exportBackup(path, languageLocale(language));
      window.alert(t("backup.exported"));
    } catch (error) {
      window.alert(errorMessage(error, t));
    }
  }

  if (!isDesktop) return <WelcomePage onCreate={createWorkspace} onOpen={openExistingWorkspace} />;
  if (snapshot.isLoading) return <div className="loading-screen"><div className="brand-mark">SP</div><p>{t("loading.opening")}</p></div>;
  if (snapshot.error) return <div className="loading-screen"><div className="brand-mark">SP</div><h2>{t("loading.failed")}</h2><p className="muted">{errorMessage(snapshot.error, t)}</p><button className="button primary" onClick={() => void snapshot.refetch()}>{t("common.retry")}</button></div>;
  if (!workspace) return <WelcomePage onCreate={createWorkspace} onOpen={openExistingWorkspace} />;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><div className="brand-mark">SP</div><div><strong>StudyPulse</strong><span>{t("brand.localWorkspace")}</span></div></div>
        <div className="workspace-pill"><span className="status-dot" /> <span title={workspace.root_path}>{workspace.root_path.split(/[\\/]/).filter(Boolean).at(-1) ?? t("workspace.default")}</span></div>
        <nav className="nav-list" aria-label={t("nav.aria")}>
          {navigation.map((item) => <button key={item.id} className={`nav-item ${page === item.id ? "active" : ""}`} onClick={() => setPage(item.id)}><span className="nav-icon">{item.icon}</span>{t(item.labelKey)}</button>)}
        </nav>
        <div className="sidebar-bottom"><button className={`nav-item ${page === "settings" ? "active" : ""}`} onClick={() => setPage("settings")}><span className="nav-icon">⚙</span>{t("nav.settings")}</button><div className="local-note"><span className="status-dot" /> {t("local.stored")}</div></div>
      </aside>
      <main className="main-column">
        <header className="topbar"><div><p className="eyebrow">{page === "today" ? t("top.learningRhythm") : pageLabel(t, page)}</p><h1>{page === "today" ? t("top.greeting") : pageLabel(t, page)}</h1></div><div className="topbar-actions"><button className="button subtle" onClick={() => void importBackup()}>{t("top.importBackup")}</button><button className="button subtle" onClick={() => void exportBackup()}>{t("top.exportBackup")}</button><button className="avatar">{snapshot.data?.provider.cloud_account?.email?.slice(0, 1).toUpperCase() ?? "•"}</button></div></header>
        {page === "today" && <TodayPage />}
        {page === "agent" && <AgentPage workspaceId={workspace.id} provider={snapshot.data?.provider} />}
        {page === "tasks" && <TasksPage />}
        {page === "subjects" && <SubjectsPage />}
        {page === "exams" && <ExamsPage />}
        {page === "mistakes" && <MistakesPage />}
        {page === "timer" && <TimerPage />}
        {page === "investment" && <InvestmentPage />}
        {page === "library" && <LibraryPage />}
        {page === "settings" && <SettingsPage provider={snapshot.data?.provider} onChanged={refresh} theme={theme} onThemeChange={setTheme} />}
      </main>
    </div>
  );
}

function WelcomePage({ onCreate, onOpen }: { onCreate: () => void; onOpen: () => void }) {
  const { t } = useI18n();
  return <div className="welcome-screen"><div className="welcome-card"><div className="brand-mark large">SP</div><p className="eyebrow">{t("welcome.eyebrow")}</p><h1>{t("welcome.title")}</h1><p className="welcome-copy">{t("welcome.copy")}</p><div className="welcome-actions"><button className="button primary" onClick={onCreate}>{t("welcome.create")}</button><button className="button secondary" onClick={onOpen}>{t("welcome.open")}</button></div><div className="privacy-callout"><span>◉</span><div><strong>{t("welcome.localTitle")}</strong><p>{t("welcome.localCopy")}</p></div></div></div></div>;
}

function SectionHeader({ title, description, action }: { title: string; description?: string; action?: ReactNode }) {
  return <div className="section-header"><div><h2>{title}</h2>{description && <p className="muted">{description}</p>}</div>{action}</div>;
}

function StatCard({ label, value, detail, accent = "sage" }: { label: string; value: string | number; detail?: string; accent?: string }) {
  return <div className={`stat-card accent-${accent}`}><span className="stat-label">{label}</span><strong>{value}</strong>{detail && <span className="stat-detail">{detail}</span>}</div>;
}

function TodayPage() {
  const { language, t } = useI18n();
  const query = useQuery({ queryKey: ["today"], queryFn: core.today });
  const tasks = useQuery({ queryKey: ["tasks"], queryFn: core.tasks });
  const exams = useQuery({ queryKey: ["exams"], queryFn: core.exams });
  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;
  const value = query.data!;
  const openTasks = tasks.data?.filter((task) => !task.is_completed).slice(0, 4) ?? [];
  const nextExam = exams.data?.slice().sort((a, b) => a.exam_date.localeCompare(b.exam_date))[0];
  return <div className="page-content"><div className="hero-banner"><div><span className="eyebrow">{formatDate(new Date().toISOString(), language)}</span><h2>{t("today.heroTitle")}<br /><em>{t("today.heroEmphasis")}</em></h2><p>{t("today.heroCopy")}</p></div><div className="hero-orbit"><span>✦</span><span>⌁</span><span>◌</span></div></div><div className="stat-grid"><StatCard label={t("today.openTasks")} value={value.open_task_count} detail={openTasks[0]?.title ?? t("today.nothingUrgent")} /><StatCard label={t("today.studyTime")} value={formatDuration(value.study_minutes * 60, t)} detail={t("today.streak", { count: value.streak_days })} accent="clay" /><StatCard label={t("today.dueMistakes")} value={value.due_mistake_count} detail={t("today.readyReview")} accent="plum" /><StatCard label={t("today.upcomingExams")} value={value.upcoming_exam_ids.length} detail={nextExam?.name ?? t("today.noExamsSoon")} accent="gold" /></div><div className="two-column"><section className="panel"><SectionHeader title={t("today.nextTitle")} description={t("today.nextDescription")} />{openTasks.length ? <div className="task-preview">{openTasks.map((task) => <div className="task-row" key={task.id}><span className="task-bullet" /><div><strong>{task.title}</strong><span>{task.subject || t("today.general")} · {t("today.due", { date: formatDate(task.due_date, language) })}</span></div><span className={`priority priority-${task.importance}`}>{task.importance >= 4 ? t("today.high") : t("today.focus")}</span></div>)}</div> : <EmptyState title={t("today.clearTitle")} copy={t("today.clearCopy")} />}</section><section className="panel reflection"><SectionHeader title={t("today.noteTitle")} description={t("today.noteDescription")} /><div className="reflection-quote">{t("today.quote")}</div><div className="phase-chip">{value.suggestions[0] ?? t("today.streak", { count: value.streak_days })}</div></section></div></div>;
}

function TasksPage() {
  const { language, t } = useI18n();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["tasks"], queryFn: core.tasks });
  const [title, setTitle] = useState("");
  const mutation = useMutation({ mutationFn: core.upsertTask, onSuccess: () => queryClient.invalidateQueries({ queryKey: ["tasks"] }), onError: (error) => window.alert(errorMessage(error, t)) });
  const toggle = useMutation({ mutationFn: ({ id, completed }: { id: string; completed: boolean }) => core.setTaskCompleted(id, completed), onSuccess: () => queryClient.invalidateQueries({ queryKey: ["tasks"] }), onError: (error) => window.alert(errorMessage(error, t)) });
  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;
  const tasks = [...(query.data ?? [])].sort((a, b) => Number(a.is_completed) - Number(b.is_completed) || a.due_date.localeCompare(b.due_date));
  function addTask() {
    const trimmed = title.trim();
    if (!trimmed) { window.alert(t("tasks.validation")); return; }
    const now = new Date().toISOString();
    mutation.mutate({ id: crypto.randomUUID(), title: trimmed, task_type: "Homework", due_date: now, reminder_date: now, subject: "", importance: 3, notes: "", is_completed: false, reminder_event_id: null, reminder_calendar_id: null, created_at: now, phase_id: null, coach_execution_data: null, coach_goal_id: null, coach_proposal_id: null, extra_json: "{}" });
    setTitle("");
  }
  return <div className="page-content"><SectionHeader title={t("tasks.title")} description={t("tasks.description")} action={<div className="inline-form"><input value={title} onChange={(event) => setTitle(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") addTask(); }} placeholder={t("tasks.addPlaceholder")} /><button className="button primary small" onClick={addTask} disabled={mutation.isPending}>{mutation.isPending ? t("tasks.saving") : t("common.add")}</button></div>} />{tasks.length ? <div className="task-list panel">{tasks.map((task) => <div className={`task-row large ${task.is_completed ? "completed" : ""}`} key={task.id}><button className={`check ${task.is_completed ? "checked" : ""}`} onClick={() => toggle.mutate({ id: task.id, completed: !task.is_completed })} disabled={toggle.isPending}>{task.is_completed ? "✓" : ""}</button><div className="task-main"><strong>{task.title}</strong><span>{task.subject || t("today.general")} · {taskTypeLabel(t, task.task_type)} · {formatDate(task.due_date, language)}</span></div><span className={`priority priority-${task.importance}`}>{task.importance >= 4 ? t("today.high") : task.importance >= 3 ? t("today.focus") : t("timer.focus")}</span></div>)}</div> : <div className="panel"><EmptyState title={t("tasks.noTasks")} copy={t("tasks.noTasksCopy")} /></div>}</div>;
}

function SubjectsPage() {
  const { language, t } = useI18n();
  const queryClient = useQueryClient();
  const subjectsQuery = useQuery({ queryKey: ["subjects"], queryFn: core.subjects });
  const gradesQuery = useQuery({ queryKey: ["grades"], queryFn: core.grades });
  const [subjectName, setSubjectName] = useState("");
  const [fullScore, setFullScore] = useState(100);
  const [gradeSubject, setGradeSubject] = useState("");
  const [gradeScore, setGradeScore] = useState(0);
  const [gradeFullScore, setGradeFullScore] = useState(100);
  const [gradeExamName, setGradeExamName] = useState("");
  const addSubject = useMutation({ mutationFn: core.upsertSubject, onSuccess: () => { void queryClient.invalidateQueries({ queryKey: ["subjects"] }); setSubjectName(""); }, onError: (error) => window.alert(errorMessage(error, t)) });
  const addGrade = useMutation({ mutationFn: core.upsertGrade, onSuccess: () => { void queryClient.invalidateQueries({ queryKey: ["grades"] }); setGradeExamName(""); }, onError: (error) => window.alert(errorMessage(error, t)) });
  if (subjectsQuery.isLoading || gradesQuery.isLoading) return <PageLoading />;
  if (subjectsQuery.error) return <ErrorCard error={subjectsQuery.error} />;
  if (gradesQuery.error) return <ErrorCard error={gradesQuery.error} />;
  const subjects = subjectsQuery.data ?? [];
  const grades = gradesQuery.data ?? [];
  function saveSubject() {
    const name = subjectName.trim();
    if (!name) { window.alert(t("subjects.validationName")); return; }
    addSubject.mutate({ id: crypto.randomUUID(), name, display_name: name, enabled: true, full_score: Math.max(1, fullScore), extra_json: "{}" });
  }
  function saveGrade() {
    const subject = gradeSubject.trim();
    if (!subject) { window.alert(t("subjects.validationGrade")); return; }
    const value: Grade = { id: crypto.randomUUID(), subject, score: gradeScore, raw_score: gradeScore, ranking: null, importance: 3, image_base64: null, image_file_name: null, date: new Date().toISOString(), exam_name: gradeExamName.trim(), exam_id: null, full_score: Math.max(1, gradeFullScore), phase_id: null, extra_json: "{}" };
    addGrade.mutate(value);
  }
  return <div className="page-content"><SectionHeader title={t("subjects.title")} description={t("subjects.description")} /><div className="two-column"><section className="panel"><SectionHeader title={t("subjects.subjectsTitle")} description={t("subjects.subjectsDescription")} action={<span className="count-badge">{subjects.length}</span>} /><div className="inline-form stacked"><input value={subjectName} onChange={(event) => setSubjectName(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") saveSubject(); }} placeholder={t("subjects.newSubject")} /><input type="number" min="1" value={fullScore} onChange={(event) => setFullScore(Number(event.target.value))} aria-label={t("subjects.fullScore")} /><button className="button primary small" onClick={saveSubject} disabled={addSubject.isPending}>{addSubject.isPending ? t("tasks.saving") : t("subjects.addSubject")}</button></div>{subjects.length ? <div className="compact-list">{subjects.map((subject) => <div className="compact-row" key={subject.id}><div><strong>{subject.display_name || subject.name}</strong><span>{subject.name} · {t("subjects.fullScore")} {subject.full_score}</span></div><span className={`status-pill ${subject.enabled ? "on" : "off"}`}>{subject.enabled ? t("subjects.active") : t("subjects.off")}</span></div>)}</div> : <EmptyState title={t("subjects.none")} copy={t("subjects.noneCopy")} />}</section><section className="panel"><SectionHeader title={t("subjects.gradesTitle")} description={t("subjects.gradesDescription")} action={<span className="count-badge">{grades.length}</span>} /><div className="form-grid"><input value={gradeSubject} onChange={(event) => setGradeSubject(event.target.value)} placeholder={t("subjects.subjectsTitle")} /><input value={gradeExamName} onChange={(event) => setGradeExamName(event.target.value)} placeholder={t("subjects.examOptional")} /><input type="number" min="0" value={gradeScore} onChange={(event) => setGradeScore(Number(event.target.value))} placeholder={t("subjects.score")} /><input type="number" min="1" value={gradeFullScore} onChange={(event) => setGradeFullScore(Number(event.target.value))} placeholder={t("subjects.fullScore")} /><button className="button primary small" onClick={saveGrade} disabled={addGrade.isPending}>{addGrade.isPending ? t("tasks.saving") : t("subjects.addGrade")}</button></div>{grades.length ? <div className="compact-list">{grades.slice().reverse().map((grade) => <div className="compact-row" key={grade.id}><div><strong>{grade.subject}</strong><span>{grade.exam_name || t("subjects.grade")} · {grade.score}/{grade.full_score ?? "—"}</span></div><span>{formatDate(grade.date, language)}</span></div>)}</div> : <EmptyState title={t("subjects.noneGrades")} copy={t("subjects.noneGradesCopy")} />}</section></div></div>;
}

function ExamsPage() {
  const { language, t } = useI18n();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["exams"], queryFn: core.exams });
  const subjectsQuery = useQuery({ queryKey: ["subjects"], queryFn: core.subjects });
  const [name, setName] = useState("");
  const [subject, setSubject] = useState("");
  const [examDate, setExamDate] = useState("");
  const [importance, setImportance] = useState(3);
  const mutation = useMutation({ mutationFn: core.upsertExam, onSuccess: () => { void queryClient.invalidateQueries({ queryKey: ["exams"] }); setName(""); setExamDate(""); }, onError: (error) => window.alert(errorMessage(error, t)) });
  const remove = useMutation({ mutationFn: core.deleteExam, onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["exams"] }), onError: (error) => window.alert(errorMessage(error, t)) });
  if (query.isLoading || subjectsQuery.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;
  const exams = query.data ?? [];
  function saveExam() {
    const trimmed = name.trim();
    if (!trimmed) { window.alert(t("exams.validationName")); return; }
    if (!examDate) { window.alert(t("exams.validationDate")); return; }
    const value: Exam = { id: crypto.randomUUID(), name: trimmed, exam_date: new Date(`${examDate}T09:00:00`).toISOString(), exam_end_date: null, importance, subject: subject.trim(), exam_name: trimmed, mastery_degree: 0, time_slot: null, phase_id: null, checklist: [], location_school: "", location_classroom: "", location_seat: "", countdown_notify_days: [7, 1], exam_review: null, extra_json: "{}" };
    mutation.mutate(value);
  }
  return <div className="page-content"><SectionHeader title={t("exams.title")} description={t("exams.description")} action={<div className="inline-form"><input value={name} onChange={(event) => setName(event.target.value)} placeholder={t("exams.name")} /><select value={subject} onChange={(event) => setSubject(event.target.value)}><option value="">{t("exams.subject")}</option>{(subjectsQuery.data ?? []).map((value) => <option key={value.id} value={value.name}>{value.display_name || value.name}</option>)}</select><input type="date" value={examDate} onChange={(event) => setExamDate(event.target.value)} /><input type="number" min="1" max="5" value={importance} onChange={(event) => setImportance(Number(event.target.value))} aria-label={t("exams.importance")} /><button className="button primary small" onClick={saveExam} disabled={mutation.isPending}>{mutation.isPending ? t("tasks.saving") : t("exams.add")}</button></div>} />{exams.length ? <div className="record-grid">{exams.slice().sort((a, b) => a.exam_date.localeCompare(b.exam_date)).map((exam) => <div className="record-card" key={exam.id}><div className="record-index">{formatDate(exam.exam_date, language)}</div><h3>{exam.name || exam.exam_name}</h3><div className="record-field"><span>{t("exams.subject")}</span><strong>{exam.subject || t("today.general")}</strong></div><div className="record-field"><span>{t("exams.importance")}</span><strong>{exam.importance}</strong></div><button className="button subtle small" onClick={() => remove.mutate(exam.id)} disabled={remove.isPending}>{t("exams.remove")}</button></div>)}</div> : <div className="panel"><EmptyState title={t("exams.none")} copy={t("exams.noneCopy")} /></div>}</div>;
}

function InvestmentPage() {
  const { language, t } = useI18n();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["investment"], queryFn: core.investmentSubjects });
  const [name, setName] = useState("");
  const [theme, setTheme] = useState("Ocean");
  const mutation = useMutation({ mutationFn: core.upsertInvestmentSubject, onSuccess: () => { void queryClient.invalidateQueries({ queryKey: ["investment"] }); setName(""); }, onError: (error) => window.alert(errorMessage(error, t)) });
  const remove = useMutation({ mutationFn: core.deleteInvestmentSubject, onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["investment"] }), onError: (error) => window.alert(errorMessage(error, t)) });
  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;
  const values = query.data ?? [];
  function saveSubject() {
    const trimmed = name.trim();
    if (!trimmed) { window.alert(t("investment.validation")); return; }
    const now = new Date().toISOString();
    const value: TimeInvestmentSubject = { id: crypto.randomUUID(), name: trimmed, symbol_name: "book.closed", theme, start_date: now, sort_order: values.length, created_at: now, is_archived: false, extra_json: "{}" };
    mutation.mutate(value);
  }
  return <div className="page-content"><SectionHeader title={t("investment.title")} description={t("investment.description")} action={<div className="inline-form"><input value={name} onChange={(event) => setName(event.target.value)} placeholder={t("investment.newSubject")} /><select value={theme} onChange={(event) => setTheme(event.target.value)}><option value="Ocean">{t("theme.ocean")}</option><option value="Coral">{t("theme.coral")}</option><option value="Violet">{t("theme.violet")}</option><option value="Sunshine">{t("theme.sunshine")}</option><option value="Mint">{t("theme.mint")}</option></select><button className="button primary small" onClick={saveSubject} disabled={mutation.isPending}>{mutation.isPending ? t("tasks.saving") : t("investment.add")}</button></div>} />{values.length ? <div className="record-grid">{values.map((value) => <div className="record-card" key={value.id}><div className="record-index">{value.sort_order + 1}</div><h3>{value.name}</h3><div className="record-field"><span>{t("investment.theme")}</span><strong>{themeLabel(t, value.theme)}</strong></div><div className="record-field"><span>{t("investment.started")}</span><strong>{formatDate(value.start_date, language)}</strong></div><button className="button subtle small" onClick={() => remove.mutate(value.id)} disabled={remove.isPending}>{t("investment.remove")}</button></div>)}</div> : <div className="panel"><EmptyState title={t("investment.none")} copy={t("investment.noneCopy")} /></div>}</div>;
}

function MistakesPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["mistakes"], queryFn: core.mistakes });
  const review = useMutation({ mutationFn: ({ id, quality }: { id: string; quality: number }) => core.reviewMistake(id, quality), onSuccess: () => queryClient.invalidateQueries({ queryKey: ["mistakes"] }) });
  const [showForm, setShowForm] = useState(false);
  const [title, setTitle] = useState("");
  const [subject, setSubject] = useState("");
  const [question, setQuestion] = useState("");
  const [errorReason, setErrorReason] = useState("");
  const addMistake = useMutation({ mutationFn: core.upsertMistake, onSuccess: () => { void queryClient.invalidateQueries({ queryKey: ["mistakes"] }); setTitle(""); setSubject(""); setQuestion(""); setErrorReason(""); setShowForm(false); }, onError: (error) => window.alert(errorMessage(error, t)) });
  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;
  const mistakes = query.data ?? [];
  function saveMistake() {
    const trimmedTitle = title.trim();
    if (!trimmedTitle) { window.alert(t("mistakes.validation")); return; }
    addMistake.mutate({ id: crypto.randomUUID(), title: trimmedTitle, subject: subject.trim(), original_question: question.trim(), source: "Manual", date: new Date().toISOString(), error_reason: errorReason.trim(), wrong_solution: "", correct_solution: "", question_images: [], reason_images: [], wrong_solution_images: [], correct_solution_images: [], review_state: null, phase_id: null, exposure_count: 0, mastery_score: 0, mastery_history: [], handwriting_history: [], difficulty: 3, tags: [], audio_file_name: null, extra_json: "{}" });
  }
  return <div className="page-content"><SectionHeader title={t("mistakes.title")} description={t("mistakes.description")} action={<button className="button primary small" onClick={() => setShowForm((value) => !value)}>{showForm ? t("mistakes.close") : t("mistakes.add")}</button>} />{showForm && <section className="panel mistake-form"><div className="form-grid"><input value={title} onChange={(event) => setTitle(event.target.value)} placeholder={t("mistakes.titlePlaceholder")} /><input value={subject} onChange={(event) => setSubject(event.target.value)} placeholder={t("mistakes.subjectPlaceholder")} /><textarea value={question} onChange={(event) => setQuestion(event.target.value)} placeholder={t("mistakes.questionPlaceholder")} /><textarea value={errorReason} onChange={(event) => setErrorReason(event.target.value)} placeholder={t("mistakes.reasonPlaceholder")} /><button className="button primary small" onClick={saveMistake} disabled={addMistake.isPending}>{addMistake.isPending ? t("tasks.saving") : t("mistakes.save")}</button></div></section>}{mistakes.length ? <div className="record-grid">{mistakes.map((mistake) => <div className="record-card mistake-card" key={mistake.id}><div className="mistake-head"><span className="tag">{mistake.subject || t("today.general")}</span><span>{Math.round(mistake.mastery_score * 100)}% {t("mistakes.mastery")}</span></div><h3>{mistake.title || t("mistakes.untitled")}</h3><p>{mistake.original_question || t("mistakes.noQuestion")}</p><div className="review-actions"><button className="button subtle small" onClick={() => review.mutate({ id: mistake.id, quality: 1 })}>{t("mistakes.again")}</button><button className="button secondary small" onClick={() => review.mutate({ id: mistake.id, quality: 3 })}>{t("mistakes.hard")}</button><button className="button primary small" onClick={() => review.mutate({ id: mistake.id, quality: 4 })}>{t("mistakes.gotIt")}</button></div></div>)}</div> : <div className="panel"><EmptyState title={t("mistakes.none")} copy={t("mistakes.noneCopy")} /></div>}</div>;
}

function TimerPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["timer"], queryFn: core.timer, refetchInterval: 1000 });
  const [intensity, setIntensity] = useState<SessionIntensity>("DeepFocus");
  const [minutes, setMinutes] = useState(25);
  const mutate = async (action: () => Promise<unknown>) => { try { await action(); await queryClient.invalidateQueries({ queryKey: ["timer"] }); } catch (error) { window.alert(errorMessage(error, t)); } };
  if (query.isLoading) return <PageLoading />;
  const timer = query.data!;
  const remaining = Math.max(0, timer.target_duration_seconds - timer.elapsed_seconds);
  return <div className="page-content timer-page"><SectionHeader title={t("timer.title")} description={t("timer.description")} /><div className="timer-card panel"><div className={`timer-ring ${timer.status.toLowerCase()}`}><span>{timer.status === "Idle" ? `${minutes}:00` : `${String(Math.floor(remaining / 60)).padStart(2, "0")}:${String(remaining % 60).padStart(2, "0")}`}</span><small>{timerStatusLabel(t, timer.status)}</small></div><div className="timer-controls">{timer.status === "Idle" ? <><label>{t("timer.minutes")}<input type="number" min="1" max="240" value={minutes} onChange={(event) => setMinutes(Math.max(1, Number(event.target.value)))} /></label><label>{t("timer.intensity")}<select value={intensity} onChange={(event) => setIntensity(event.target.value as SessionIntensity)}><option value="Peak">{t("intensity.peak")}</option><option value="DeepFocus">{t("intensity.deepFocus")}</option><option value="Steady">{t("intensity.steady")}</option><option value="Light">{t("intensity.light")}</option><option value="Recovery">{t("intensity.recovery")}</option></select></label><button className="button primary" onClick={() => void mutate(() => core.startTimer(intensity, minutes * 60))}>{t("timer.start")}</button></> : <><div className="timer-meta"><span>{intensityLabel(t, timer.intensity)}</span><span>{t("timer.elapsed", { duration: formatDuration(timer.elapsed_seconds, t) })}</span></div>{timer.status === "Running" ? <button className="button secondary" onClick={() => void mutate(core.pauseTimer)}>{t("timer.pause")}</button> : <button className="button primary" onClick={() => void mutate(core.resumeTimer)}>{t("timer.resume")}</button>}<button className="button subtle" onClick={() => void mutate(core.finishTimer)}>{t("timer.finish")}</button><button className="button danger" onClick={() => void mutate(core.cancelTimer)}>{t("timer.cancel")}</button></>}</div></div></div>;
}

function LibraryPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ["library"], queryFn: core.library });
  const [search, setSearch] = useState("");
  const results = useQuery({ queryKey: ["library-search", search], queryFn: () => core.searchLibrary(search), enabled: search.trim().length > 1 });
  async function importFiles() { for (const path of await chooseSourceFiles(t("dialog.addSources"))) { try { await core.importLibraryFile(path); } catch (error) { window.alert(errorMessage(error, t)); } } await queryClient.invalidateQueries({ queryKey: ["library"] }); }
  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;
  const files = query.data ?? [];
  return <div className="page-content"><SectionHeader title={t("library.title")} description={t("library.description")} action={<button className="button primary" onClick={() => void importFiles()}>{t("library.add")}</button>} /><div className="search-bar"><span>⌕</span><input value={search} onChange={(event) => setSearch(event.target.value)} placeholder={t("library.searchPlaceholder")} /></div>{search.trim().length > 1 && <section className="panel search-results"><SectionHeader title={t("library.results")} />{results.data?.length ? results.data.map((match) => <div className="search-result" key={`${match.relative_path}-${match.line_number}`}><strong>{match.relative_path}</strong><span>{t("library.line", { line: match.line_number ?? "—" })}</span><p>{match.snippet}</p></div>) : <p className="muted">{t("library.noMatches")}</p>}</section>}<div className="file-grid">{files.filter((file) => !file.is_directory).map((file) => <div className="file-card" key={file.relative_path}><span className="file-icon">{file.relative_path.endsWith(".md") ? "M↓" : "TXT"}</span><div><strong>{file.relative_path.split("/").at(-1)}</strong><span>{file.relative_path} · {Math.ceil(file.size_bytes / 1024)} KB</span></div></div>)}</div>{!files.length && <div className="panel"><EmptyState title={t("library.empty")} copy={t("library.emptyCopy")} /></div>}</div>;
}

function AgentPage({ workspaceId, provider }: { workspaceId: string; provider?: AppSnapshot["provider"] }) {
  const { language, t } = useI18n();
  const queryClient = useQueryClient();
  const notebooksQuery = useQuery({ queryKey: ["notebooks"], queryFn: core.notebooks });
  const filesQuery = useQuery({ queryKey: ["library"], queryFn: core.library });
  const capabilitiesQuery = useQuery({ queryKey: ["capabilities"], queryFn: core.capabilities });
  const [selectedId, setSelectedId] = useState<string>();
  const [mode, setMode] = useState<AgentMode>("Chat");
  const [goal, setGoal] = useState("");
  const [sourcePaths, setSourcePaths] = useState<string[]>([]);
  const [runId, setRunId] = useState<string>();
  const [running, setRunning] = useState(false);
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [answer, setAnswer] = useState("");
  const [pending, setPending] = useState<AgentEvent>();
  const [pendingInput, setPendingInput] = useState("");
  const [activity, setActivity] = useState<string>();
  const notebooks = notebooksQuery.data ?? [];
  const selected = notebooks.find((notebook) => notebook.id === selectedId) ?? notebooks[0];
  const effectiveSourcePaths = selectedId ? sourcePaths : (selected?.source_paths ?? sourcePaths);
  const selectedMessages = selected?.messages ?? [];
  const files = (filesQuery.data ?? []).filter((file) => !file.is_directory);
  const canRun = Boolean(provider?.cloud_account || provider?.byok_config);

  async function persist(next: AgentNotebook[]) {
    await core.saveNotebooks(workspaceId, next);
    await queryClient.invalidateQueries({ queryKey: ["notebooks"] });
  }

  async function createNotebook(): Promise<AgentNotebook> {
    const notebook: AgentNotebook = { id: crypto.randomUUID(), title: t("agent.untitledNotebook", { count: notebooks.length + 1 }), source_paths: [], messages: [], last_goal: "", last_answer: "", updated_at: new Date().toISOString() };
    await persist([notebook, ...notebooks]);
    setSelectedId(notebook.id);
    setSourcePaths([]);
    return notebook;
  }

  async function toggleSource(path: string) {
    const nextPaths = effectiveSourcePaths.includes(path) ? effectiveSourcePaths.filter((value) => value !== path) : [...effectiveSourcePaths, path].sort();
    setSourcePaths(nextPaths);
    if (selected) await persist(notebooks.map((notebook) => notebook.id === selected.id ? { ...notebook, source_paths: nextPaths, updated_at: new Date().toISOString() } : notebook));
  }

  async function runAgent() {
    const trimmed = goal.trim();
    if (!trimmed || running) return;
    if (!canRun) { setActivity(t("agent.noProvider")); return; }
    let notebook = selected;
    let notebookList = notebooks;
    if (!notebook) { notebook = await createNotebook(); notebookList = [notebook, ...notebooks]; }
    const history = notebook.messages;
    const userMessage: AgentMessage = { id: crypto.randomUUID(), role: "User", content: trimmed, created_at: new Date().toISOString() };
    const optimisticNotebook = { ...notebook, source_paths: effectiveSourcePaths, messages: [...history, userMessage], last_goal: trimmed, last_answer: "", updated_at: new Date().toISOString() };
    await persist(notebookList.map((value) => value.id === notebook!.id ? optimisticNotebook : value));
    setGoal(""); setAnswer(""); setEvents([]); setPending(undefined); setActivity(t("agent.starting")); setRunning(true);
    try {
      const id = await core.startAgent({ mode, goal: trimmed, sourcePaths: effectiveSourcePaths, history });
      setRunId(id);
      let cursor = 0;
      let assembled = "";
      let finished = false;
      while (!finished) {
        const batch = await core.waitAgentEvents(id, cursor, 1000);
        for (const event of batch) {
          cursor = Math.max(cursor, event.sequence);
          setEvents((current) => [...current, event]);
          if (event.stage) setActivity(event.stage);
          if (event.kind === "TextDelta") { assembled += event.text ?? ""; setAnswer(assembled); }
          if ((event.kind === "ConfirmationRequired" || event.kind === "ToolRequested") && event.confirmation_id) setPending(event);
          if (event.kind === "InputRequired") setPending(event);
          if (["Failed", "Cancelled", "Completed"].includes(event.kind)) finished = true;
        }
      }
      const assistant: AgentMessage = { id: crypto.randomUUID(), role: "Assistant", content: assembled, created_at: new Date().toISOString() };
      await persist(notebookList.map((value) => value.id === notebook!.id ? { ...optimisticNotebook, messages: [...optimisticNotebook.messages, assistant], last_answer: assembled, updated_at: new Date().toISOString() } : value));
      setAnswer("");
      setActivity(t("agent.completed"));
    } catch (error) { setActivity(errorMessage(error, t)); } finally { setRunning(false); setPending(undefined); setRunId(undefined); }
  }

  async function resolveConfirmation(decision: "Allow" | "Deny") { if (!pending?.confirmation_id || !runId) return; await core.submitConfirmation(runId, pending.confirmation_id, decision); setPending(undefined); }
  async function resolveInput() { if (!pending?.confirmation_id || !runId) return; const answerJson = JSON.stringify({ answer: pendingInput }); await core.submitAgentInput(runId, pending.confirmation_id, answerJson); setPending(undefined); setPendingInput(""); }
  async function cancel() { if (runId) await core.cancelAgent(runId); }

  return <div className="page-content agent-page"><div className="agent-layout"><aside className="agent-notebooks panel"><SectionHeader title={t("agent.notebooks")} action={<button className="icon-button" onClick={() => void createNotebook()}>＋</button>} />{notebooks.map((notebook) => <button className={`notebook-item ${selected?.id === notebook.id ? "active" : ""}`} key={notebook.id} onClick={() => { setSelectedId(notebook.id); setSourcePaths(notebook.source_paths); }} disabled={running}><strong>{notebook.title}</strong><span>{t("agent.sources", { count: notebook.source_paths.length })} · {t("agent.messages", { count: notebook.messages.length })}</span></button>)}{!notebooks.length && <p className="muted small-copy">{t("agent.noNotebook")}</p>}</aside><section className="agent-main panel"><div className="agent-toolbar"><div><span className="eyebrow">{selected?.title ?? t("agent.newNotebook")}</span><h2>{t("agent.promptTitle")}</h2></div><select value={mode} onChange={(event) => setMode(event.target.value as AgentMode)} disabled={running}>{(capabilitiesQuery.data ?? []).map((capability) => <option key={capability.mode} value={capability.mode}>{modeLabel(t, capability.mode)}</option>)}</select></div><div className="agent-messages">{selectedMessages.map((message) => <div className={`message ${message.role === "User" ? "user" : "assistant"}`} key={message.id}>{message.role === "Assistant" ? <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>{message.content}</ReactMarkdown> : <p>{message.content}</p>}</div>)}{answer && <div className="message assistant"><ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>{answer}</ReactMarkdown></div>}{!selectedMessages.length && !answer && <div className="agent-empty"><div className="spark">✦</div><h3>{t("agent.emptyTitle")}</h3><p>{t("agent.emptyCopy")}</p></div>}</div>{pending && (pending.kind === "ConfirmationRequired" || pending.confirmation_id) && <div className="permission-card"><span className="permission-icon">!</span><div><strong>{t("agent.permission")}</strong><p>{pending.preview ?? pending.tool_name ?? t("agent.toolFallback")}</p></div><div className="permission-actions"><button className="button subtle small" onClick={() => void resolveConfirmation("Deny")}>{t("agent.deny")}</button><button className="button primary small" onClick={() => void resolveConfirmation("Allow")}>{t("agent.allowOnce")}</button></div></div>}{pending?.kind === "InputRequired" && <div className="permission-card"><span className="permission-icon">?</span><div><strong>{t("agent.inputRequired")}</strong><p>{pending.preview ?? t("agent.inputFallback")}</p><input value={pendingInput} onChange={(event) => setPendingInput(event.target.value)} placeholder={t("agent.answerPlaceholder")} /></div><button className="button primary small" onClick={() => void resolveInput()}>{t("agent.send")}</button></div>}<div className="agent-composer"><textarea value={goal} onChange={(event) => setGoal(event.target.value)} onKeyDown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === "Enter") void runAgent(); }} placeholder={t("agent.askPlaceholder")} disabled={running} /><div className="composer-footer"><div className="source-chips">{files.slice(0, 3).map((file) => <button key={file.relative_path} className={`source-chip ${effectiveSourcePaths.includes(file.relative_path) ? "selected" : ""}`} onClick={() => void toggleSource(file.relative_path)} disabled={running}>＋ {file.relative_path.split("/").at(-1)}</button>)}{files.length > 3 && <span className="muted small-copy">{t("agent.moreLibrary", { count: files.length - 3 })}</span>}</div><div className="composer-actions"><span className="activity">{activity ?? t("agent.shortcut")}</span>{running ? <button className="button danger" onClick={() => void cancel()}>{t("agent.cancel")}</button> : <button className="button primary" onClick={() => void runAgent()}>{t("agent.run")} <span>↗</span></button>}</div></div></div></section><aside className="agent-timeline panel"><SectionHeader title={t("agent.timeline")} description={t("agent.timelineDescription")} />{events.length ? <div className="timeline">{events.slice(-12).map((event) => <div className="timeline-item" key={`${event.run_id}-${event.sequence}`}><span className={`timeline-dot kind-${event.kind.toLowerCase()}`} /><div><strong>{eventLabel(t, event.kind)}</strong><span>{event.tool_name ?? event.stage ?? event.preview ?? formatDate(event.timestamp, language)}</span></div></div>)}</div> : <p className="muted small-copy">{t("agent.noEvents")}</p>}</aside></div></div>;
}

function SettingsPage({ provider, onChanged, theme, onThemeChange }: { provider?: AppSnapshot["provider"]; onChanged: () => void; theme: "light" | "dark"; onThemeChange: (theme: "light" | "dark") => void }) {
  const { language, setLanguage, t } = useI18n();
  const [baseUrl, setBaseUrl] = useState("https://api.openai.com/v1");
  const [model, setModel] = useState("gpt-4o-mini");
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(false);
  async function signIn() { setBusy(true); try { const url = await core.loginUrl(); await core.openExternal(url); } catch (error) { window.alert(errorMessage(error, t)); } finally { setBusy(false); } }
  async function saveByok() { setBusy(true); try { await core.saveByok({ apiKey, baseUrl, model }); setApiKey(""); onChanged(); } catch (error) { window.alert(errorMessage(error, t)); } finally { setBusy(false); } }
  async function disconnect() { setBusy(true); try { await core.disconnectAi(); onChanged(); } catch (error) { window.alert(errorMessage(error, t)); } finally { setBusy(false); } }
  return <div className="page-content settings-page"><SectionHeader title={t("settings.title")} description={t("settings.description")} /><section className="settings-card panel"><div className="settings-heading"><div><span className="eyebrow">{t("settings.appearance")}</span><h3>{t("settings.appearanceTitle")}</h3></div></div><div className="setting-row"><div><strong>{t("settings.theme")}</strong><span>{t("settings.themeDescription")}</span></div><div className="segmented"><button className={theme === "light" ? "active" : ""} onClick={() => onThemeChange("light")}>{t("theme.light")}</button><button className={theme === "dark" ? "active" : ""} onClick={() => onThemeChange("dark")}>{t("theme.dark")}</button></div></div><div className="setting-row"><div><strong>{t("settings.language")}</strong><span>{t("settings.languageDescription")}</span></div><select value={language} onChange={(event) => setLanguage(event.target.value as typeof language)} aria-label={t("language.label")}><option value="zh-CN">简体中文</option><option value="zh-TW">繁體中文</option><option value="en">English</option><option value="ja">日本語</option><option value="ko">한국어</option></select></div></section><section className="settings-card panel"><div className="settings-heading"><div><span className="eyebrow">{t("settings.provider")}</span><h3>{t("settings.providerTitle")}</h3><p>{t("settings.providerDescription")}</p></div><span className={`connection-badge ${provider?.cloud_account || provider?.byok_config ? "connected" : ""}`}>{provider?.cloud_account ? t("settings.cloudConnected") : provider?.byok_config ? t("settings.byokConnected") : t("settings.notConnected")}</span></div><div className="provider-option"><div><strong>{t("settings.cloudName")}</strong><span>{t("settings.cloudCopy")}</span></div><button className="button secondary" onClick={() => void signIn()} disabled={busy}>{busy ? t("settings.working") : t("settings.signIn")}</button></div><div className="provider-option"><div><strong>{t("settings.byokName")}</strong><span>{t("settings.byokCopy")}</span></div><div className="byok-form"><input value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} placeholder={t("settings.baseUrl")} /><input value={model} onChange={(event) => setModel(event.target.value)} placeholder={t("settings.model")} /><input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={provider?.has_saved_byok ? t("settings.savedKey") : t("settings.apiKey")} /><button className="button primary" onClick={() => void saveByok()} disabled={busy}>{busy ? t("settings.saving") : t("settings.saveByok")}</button></div></div>{(provider?.cloud_account || provider?.byok_config || provider?.has_saved_byok) && <button className="button danger outline" onClick={() => void disconnect()} disabled={busy}>{t("settings.disconnect")}</button>}</section><section className="settings-card panel"><div className="settings-heading"><div><span className="eyebrow">{t("settings.privacy")}</span><h3>{t("settings.privacyTitle")}</h3><p>{t("settings.privacyCopy")}</p></div></div></section></div>;
}

function PageLoading() { return <div className="page-content"><div className="skeleton-card" /><div className="skeleton-card short" /></div>; }
function ErrorCard({ error }: { error: unknown }) { const { t } = useI18n(); return <div className="page-content"><div className="panel error-card"><strong>{t("error.section")}</strong><p>{errorMessage(error, t)}</p></div></div>; }
function EmptyState({ title, copy }: { title: string; copy: string }) { return <div className="empty-state"><div className="empty-orb">○</div><h3>{title}</h3><p>{copy}</p></div>; }
