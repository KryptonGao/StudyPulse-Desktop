import { useMemo, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { core } from "../lib/core";
import { useI18n } from "../i18n";
import { useToast } from "../components/Toast";
import { useConfirm } from "../components/ConfirmDialog";
import MathText from "../components/MathText";
import type { DiaryEntry, MistakeNote, TrendsSnapshot } from "../types";

// Diary dates are keyed by the local calendar day for editing, then converted
// to an ISO midnight value when crossing into the Core DTO.
function dayKey(value = new Date()): string {
  const year = value.getFullYear();
  const month = String(value.getMonth() + 1).padStart(2, "0");
  const day = String(value.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function diaryDate(day: string): string {
  // The explicit UTC suffix keeps a selected calendar day stable in storage;
  // display helpers below use a noon UTC anchor to avoid local offset rollover.
  return `${day}T00:00:00.000Z`;
}

function displayDate(value: string, language: string): string {
  // Formatting at noon UTC prevents a timestamp near midnight from rendering
  // as the previous day in a user's local timezone.
  const raw = value.slice(0, 10);
  const date = new Date(`${raw}T12:00:00Z`);
  return Number.isNaN(date.valueOf())
    ? value
    : date.toLocaleDateString(language === "en" ? "en-US" : language, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
}

function displayShortDate(value: string, language: string): string {
  // Charts use the same date anchor as the diary list but intentionally omit
  // the year to keep compact axis labels readable.
  const raw = value.slice(0, 10);
  const date = new Date(`${raw}T12:00:00Z`);
  return Number.isNaN(date.valueOf())
    ? raw
    : date.toLocaleDateString(language === "en" ? "en-US" : language, {
        month: "numeric",
        day: "numeric",
      });
}

function duration(minutes: number, t: (key: string, variables?: Record<string, string | number>) => string): string {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return hours ? `${t("duration.hours", { count: hours })} ${t("duration.minutes", { count: rest })}` : t("duration.minutes", { count: minutes });
}

function trendLevel(points: number): number {
  // Core provides activity points; this view maps them to five visual buckets
  // without recomputing the underlying study/review/grade formula.
  if (points <= 0) return 0;
  if (points <= 2) return 1;
  if (points <= 5) return 2;
  if (points <= 10) return 3;
  return 4;
}

function Section({ title, description, action }: { title: string; description?: string; action?: ReactNode }) {
  return <div className="section-header"><div><h2>{title}</h2>{description && <p className="muted">{description}</p>}</div>{action}</div>;
}

function TrendSvg({ values, color = "var(--sage-dark)", min = 0, max }: { values: (number | null)[]; color?: string; min?: number; max?: number }) {
  // Missing daily values are gaps, not zeroes. Coordinates are clamped to the
  // fixed viewBox so sparse or unusually large values remain drawable.
  const resolvedMax = max ?? Math.max(min + 1, ...values.filter((value): value is number => value !== null));
  const points = values.map((value, index) => {
    if (value === null) return null;
    const x = values.length <= 1 ? 0 : (index / (values.length - 1)) * 600;
    const y = 150 - ((value - min) / Math.max(1, resolvedMax - min)) * 130;
    return `${x.toFixed(1)},${Math.max(10, Math.min(150, y)).toFixed(1)}`;
  }).filter((value): value is string => value !== null).join(" ");
  // `role="img"` gives the non-text chart a stable accessible name; the data
  // remains summarized in adjacent labels and stat values.
  return <svg className="p1-trend-svg" viewBox="0 0 600 170" role="img" aria-label="trend chart"><line x1="0" x2="600" y1="150" y2="150" className="chart-axis" /><polyline fill="none" stroke={color} strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" points={points} /></svg>;
}

type DiaryDraft = Pick<DiaryEntry, "date" | "mood_score" | "energy_score" | "energy_tag" | "content">;

function emptyDiary(day = dayKey()): DiaryDraft {
  return { date: day, mood_score: 3, energy_score: 3, energy_tag: "", content: "" };
}

export function DiaryPage() {
  const { language, t } = useI18n();
  const { showToast } = useToast();
  const confirm = useConfirm();
  const queryClient = useQueryClient();
  const entriesQuery = useQuery({ queryKey: ["diary"], queryFn: core.diaryEntries });
  const [rangeDays, setRangeDays] = useState<7 | 30>(30);
  const trendQuery = useQuery({ queryKey: ["trends", rangeDays], queryFn: () => core.learningTrends(rangeDays) });
  const [draft, setDraft] = useState<DiaryDraft>(() => emptyDiary());
  const [editingId, setEditingId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // The diary list is sorted locally for stable newest-first editing, while
  // trend data remains the Core-derived snapshot for the selected range.
  const entries = useMemo(() => [...(entriesQuery.data ?? [])].sort((a, b) => b.date.localeCompare(a.date) || b.updated_at.localeCompare(a.updated_at)), [entriesQuery.data]);
  const save = async () => {
    // Editing preserves identity and creation metadata; a new entry gets a
    // UUID and both mood/energy values are clamped to the five-point contract.
    if (!draft.date) {
      showToast(t("diary.validationDate"), "error");
      return;
    }
    if (saving) return;
    const current = editingId ? entries.find((entry) => entry.id === editingId) : undefined;
    const now = new Date().toISOString();
    setSaving(true);
    try {
      await core.upsertDiaryEntry({
        id: current?.id ?? crypto.randomUUID(),
        date: diaryDate(draft.date),
        mood_score: Math.max(1, Math.min(5, draft.mood_score)),
        energy_score: Math.max(1, Math.min(5, draft.energy_score)),
        energy_tag: draft.energy_tag.trim(),
        content: draft.content,
        phase_id: current?.phase_id ?? null,
        created_at: current?.created_at ?? now,
        updated_at: now,
        extra_json: current?.extra_json ?? "{}",
      });
      // A diary write affects both the list and the trend projection, so both
      // query families are refreshed after persistence succeeds.
      await queryClient.invalidateQueries({ queryKey: ["diary"] });
      await queryClient.invalidateQueries({ queryKey: ["trends"] });
      setEditingId(null);
      setDraft(emptyDiary());
      showToast(t("common.saved"), "success");
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setSaving(false);
    }
  };
  const edit = (entry: DiaryEntry) => {
    // The form edits only the user-facing draft fields; immutable record
    // fields are recovered from the selected entry during save.
    setEditingId(entry.id);
    setDraft({ date: entry.date.slice(0, 10), mood_score: entry.mood_score, energy_score: entry.energy_score, energy_tag: entry.energy_tag, content: entry.content });
  };
  const remove = async (entry: DiaryEntry) => {
    // Deletion is confirmed in the UI, then the same diary/trends invalidation
    // keeps derived charts from displaying removed data.
    try {
      const ok = await confirm({ title: t("diary.delete"), message: t("diary.confirmDelete"), isDestructive: true });
      if (!ok) return;
      await core.deleteDiaryEntry(entry.id);
      await queryClient.invalidateQueries({ queryKey: ["diary"] });
      await queryClient.invalidateQueries({ queryKey: ["trends"] });
      if (editingId === entry.id) {
        setEditingId(null);
        setDraft(emptyDiary());
      }
      showToast(t("common.saved"), "success");
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  };
  if (entriesQuery.isLoading || trendQuery.isLoading) return <div className="page-content"><div className="skeleton-card" /><div className="skeleton-card short" /></div>;
  if (entriesQuery.error || trendQuery.error) return <div className="page-content"><div className="panel error-card"><strong>{t("error.section")}</strong><p>{String(entriesQuery.error ?? trendQuery.error)}</p></div></div>;
  const trend = trendQuery.data!;
  const moodValues = trend.daily_points.map((point) => point.mood_score);
  const energyValues = trend.daily_points.map((point) => point.energy_score);
  return <div className="page-content p1-page">
    <Section title={t("diary.title")} description={t("diary.description")} action={<div className="inline-form"><div className="segmented"><button className={rangeDays === 7 ? "active" : ""} onClick={() => setRangeDays(7)}>7d</button><button className={rangeDays === 30 ? "active" : ""} onClick={() => setRangeDays(30)}>30d</button></div><button className="button subtle" onClick={() => { setEditingId(null); setDraft(emptyDiary()); }}>{t("diary.new")}</button></div>} />
    <div className="two-column p1-columns">
      <section className="panel diary-editor">
        <Section title={editingId ? t("diary.edit") : t("diary.entryTitle")} description={t("diary.entryDescription")} />
        <div className="p1-form">
          <label>{t("diary.date")}<input type="date" value={draft.date} onChange={(event) => setDraft((value) => ({ ...value, date: event.target.value }))} /></label>
          <div><span className="form-label">{t("diary.mood")}</span><div className="mood-picker">{[1, 2, 3, 4, 5].map((score) => <button key={score} className={draft.mood_score === score ? "selected" : ""} onClick={() => setDraft((value) => ({ ...value, mood_score: score }))} type="button">{["😢", "😕", "🙂", "😊", "🤩"][score - 1]}<small>{score}</small></button>)}</div></div>
          <label>{t("diary.energy")}<input type="range" min="1" max="5" value={draft.energy_score} onChange={(event) => setDraft((value) => ({ ...value, energy_score: Number(event.target.value) }))} /><span className="range-value">{draft.energy_score}/5</span></label>
          <label>{t("diary.tag")}<input value={draft.energy_tag} onChange={(event) => setDraft((value) => ({ ...value, energy_tag: event.target.value }))} placeholder={t("diary.tagPlaceholder")} /></label>
          <label>{t("diary.content")}<textarea rows={8} value={draft.content} onChange={(event) => setDraft((value) => ({ ...value, content: event.target.value }))} placeholder={t("diary.contentPlaceholder")} /></label>
          <div className="form-actions"><button className="button primary" onClick={() => void save()} disabled={saving}>{saving ? t("common.saving") : editingId ? t("diary.update") : t("diary.save")}</button>{editingId && <button className="button subtle" onClick={() => { setEditingId(null); setDraft(emptyDiary()); }} disabled={saving}>{t("diary.cancel")}</button>}</div>
        </div>
      </section>
      <section className="panel p1-chart-card">
        <Section title={t("diary.trendTitle")} description={t("diary.trendDescription")} action={<span className="count-badge">{rangeDays}d</span>} />
        <div className="chart-block"><div className="chart-heading"><span>{t("diary.mood")}</span><strong>{trend.average_mood?.toFixed(1) ?? "—"}/5</strong></div><TrendSvg values={moodValues} color="var(--clay)" min={1} max={5} /></div>
        <div className="chart-block"><div className="chart-heading"><span>{t("diary.energy")}</span><strong>{trend.average_energy?.toFixed(1) ?? "—"}/5</strong></div><TrendSvg values={energyValues} color="var(--plum)" min={1} max={5} /></div>
        <div className="p1-mini-stats"><span>{t("diary.activeDays")} <strong>{trend.active_days}</strong></span><span>{t("diary.studyMinutes")} <strong>{trend.total_study_minutes}</strong></span></div>
      </section>
    </div>
    <section className="panel diary-list-panel">
      <Section title={t("diary.history")} description={t("diary.historyDescription")} action={<span className="count-badge">{entries.length}</span>} />
      {entries.length ? <div className="diary-list">{entries.map((entry) => <article className="diary-entry" key={entry.id}><div className="diary-entry-head"><div><strong>{displayDate(entry.date, language)}</strong><span>{["😢", "😕", "🙂", "😊", "🤩"][entry.mood_score - 1]} · {t("diary.energyShort")} {entry.energy_score}/5 {entry.energy_tag && `· ${entry.energy_tag}`}</span></div><div className="row-actions"><button className="button subtle small" onClick={() => edit(entry)}>{t("diary.edit")}</button><button className="button danger outline small" onClick={() => void remove(entry)}>{t("diary.delete")}</button></div></div>{entry.content ? <p>{entry.content}</p> : <p className="muted">{t("diary.noContent")}</p>}</article>)}</div> : <div className="p1-empty"><h3>{t("diary.empty")}</h3><p>{t("diary.emptyCopy")}</p></div>}
    </section>
  </div>;
}

export function TrendsPage() {
  const { language, t } = useI18n();
  const query = useQuery({ queryKey: ["trends", 90], queryFn: () => core.learningTrends(90) });
  const [mode, setMode] = useState<"score" | "ranking">("score");
  if (query.isLoading) return <div className="page-content"><div className="skeleton-card" /><div className="skeleton-card short" /></div>;
  if (query.error) return <div className="page-content"><div className="panel error-card"><strong>{t("error.section")}</strong><p>{String(query.error)}</p></div></div>;
  const trend: TrendsSnapshot = query.data!;
  // The score/ranking toggle changes presentation only; all subject trend
  // values and thresholds are already computed by Core.
  const studyValues = trend.daily_points.map((point) => point.study_minutes);
  return <div className="page-content p1-page">
    <Section title={t("trends.title")} description={t("trends.description")} action={<div className="segmented"><button className={mode === "score" ? "active" : ""} onClick={() => setMode("score")}>{t("trends.score")}</button><button className={mode === "ranking" ? "active" : ""} onClick={() => setMode("ranking")}>{t("trends.ranking")}</button></div>} />
    <section className="panel heatmap-panel"><Section title={t("trends.heatmapTitle")} description={t("trends.heatmapDescription")} action={<span className="status-pill on">{t("trends.activeDays", { count: trend.active_days })}</span>} /><div className="heatmap-grid">{trend.daily_points.map((point) => <div className={`heat-cell heat-level-${trendLevel(point.activity_points)}`} title={`${displayDate(point.date, language)} · ${point.activity_points} ${t("trends.points")}`} key={point.date} />)}</div><div className="heatmap-footer"><span>{t("trends.streak", { count: trend.current_streak })}</span><span className="heat-legend"><i className="heat-cell heat-level-0" /><i className="heat-cell heat-level-1" /><i className="heat-cell heat-level-2" /><i className="heat-cell heat-level-3" /><i className="heat-cell heat-level-4" /></span></div></section>
    <div className="stat-grid p1-stat-grid"><div className="stat-card"><span className="stat-label">{t("trends.studyTime")}</span><strong>{duration(trend.total_study_minutes, t)}</strong><span className="stat-detail">{t("trends.range", { start: displayShortDate(trend.start_date, language), end: displayShortDate(trend.end_date, language) })}</span></div><div className="stat-card accent-clay"><span className="stat-label">{t("trends.averageMood")}</span><strong>{trend.average_mood?.toFixed(1) ?? "—"}</strong><span className="stat-detail">{t("trends.outOfFive")}</span></div><div className="stat-card accent-plum"><span className="stat-label">{t("trends.averageEnergy")}</span><strong>{trend.average_energy?.toFixed(1) ?? "—"}</strong><span className="stat-detail">{t("trends.outOfFive")}</span></div><div className="stat-card accent-gold"><span className="stat-label">{t("trends.dueReviews")}</span><strong>{trend.srs.due_count}</strong><span className="stat-detail">{t("trends.upcomingReviews", { count: trend.srs.upcoming_count })}</span></div></div>
    <div className="two-column p1-columns"><section className="panel p1-chart-card"><Section title={t("trends.studyChartTitle")} description={t("trends.studyChartDescription")} /><TrendSvg values={studyValues} color="var(--sage-dark)" max={Math.max(25, ...studyValues)} /><div className="chart-labels"><span>{displayShortDate(trend.start_date, language)}</span><span>{displayShortDate(trend.end_date, language)}</span></div></section><section className="panel srs-summary"><Section title={t("trends.srsTitle")} description={t("trends.srsDescription")} /><div className="srs-summary-grid"><div><strong>{trend.srs.total_enrolled}</strong><span>{t("trends.enrolled")}</span></div><div><strong>{trend.srs.due_count}</strong><span>{t("trends.due")}</span></div><div><strong>{trend.srs.upcoming_count}</strong><span>{t("trends.nextSeven")}</span></div></div><p className="muted">{t("trends.flashcardHint")}</p></section></div>
    <section className="panel subject-trends"><Section title={t("trends.subjectTitle")} description={t("trends.subjectDescription")} action={<span className="count-badge">{trend.subjects.length}</span>} />{trend.subjects.length ? <div className="subject-trend-grid">{trend.subjects.map((subject) => <article className={`subject-trend-card ${subject.needs_attention ? "attention" : ""}`} key={subject.subject}><div className="subject-trend-head"><div><strong>{subject.display_name || subject.subject}</strong><span>{subject.grade_count} {t("trends.grades")} · {subject.mistake_count} {t("trends.mistakes")}</span></div><span className={`trend-badge ${subject.trend}`}>{t(`trends.${subject.trend}`)}</span></div>{mode === "score" ? <><div className="subject-score"><strong>{Math.round(subject.latest_score_rate * 100)}%</strong><span>{t("trends.latest")}</span></div><div className="subject-progress"><i style={{ width: `${Math.round(subject.average_score_rate * 100)}%` }} /></div><span className="muted">{t("trends.average")} {Math.round(subject.average_score_rate * 100)}% · {subject.due_mistake_count} {t("trends.dueMistakes")}</span></> : <><div className="subject-score"><strong>{subject.latest_ranking ?? "—"}</strong><span>{t("trends.latestRanking")}</span></div><span className="muted">{t("trends.averageRanking")} {subject.average_ranking?.toFixed(0) ?? "—"} · {subject.due_mistake_count} {t("trends.dueMistakes")}</span></>}{subject.needs_attention && <span className="attention-note">{t("trends.needsAttention")}</span>}</article>)}</div> : <div className="p1-empty"><h3>{t("trends.noSubjects")}</h3><p>{t("trends.noSubjectsCopy")}</p></div>}</section>
  </div>;
}

type ReviewQuality = 1 | 3 | 4 | 5;

function sortedMistakes(values: MistakeNote[]): MistakeNote[] {
  // Sorting is copy-on-read so React Query’s cached array is never mutated by
  // the flashcard session; stable id ordering breaks equal-date ties.
  return values.slice().sort((left, right) => (left.review_state?.next_review_date ?? "").localeCompare(right.review_state?.next_review_date ?? "") || left.id.localeCompare(right.id));
}

export function FlashcardsPage() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const queryClient = useQueryClient();
  const dueQuery = useQuery({ queryKey: ["flashcards"], queryFn: core.dueMistakes });
  const allQuery = useQuery({ queryKey: ["mistakes"], queryFn: core.mistakes });
  const [queue, setQueue] = useState<MistakeNote[]>([]);
  const [sessionActive, setSessionActive] = useState(false);
  const [summary, setSummary] = useState(false);
  const [flipped, setFlipped] = useState(false);
  const [requeued, setRequeued] = useState<string[]>([]);
  const [stats, setStats] = useState({ reviewed: 0, again: 0, hard: 0, good: 0, easy: 0 });
  const [busy, setBusy] = useState(false);
  // Before a session starts, the queue comes from Core’s due snapshot. Once a
  // rating is made, local queue state controls requeue order for this session.
  const activeQueue = sessionActive ? queue : sortedMistakes(dueQuery.data ?? []);
  const current = activeQueue[0];
  const rate = async (quality: ReviewQuality) => {
    // Again/Hard/Good/Easy intentionally use 1/3/4/5 to match the iOS/Core
    // SRS contract. Ratings require the answer side to be visible first.
    if (!current || !flipped || busy) return;
    setBusy(true);
    try {
      await core.reviewMistake(current.id, quality);
      const nextStats = { ...stats, reviewed: stats.reviewed + 1, again: stats.again + (quality === 1 ? 1 : 0), hard: stats.hard + (quality === 3 ? 1 : 0), good: stats.good + (quality === 4 ? 1 : 0), easy: stats.easy + (quality === 5 ? 1 : 0) };
      setStats(nextStats);
      const rest = activeQueue.slice(1);
      // An Again card returns once to the tail, but cannot loop indefinitely in
      // the same session because `requeued` records that it already returned.
      if (quality === 1 && !requeued.includes(current.id)) {
        setRequeued((values) => [...values, current.id]);
        rest.push(current);
      }
      setQueue(rest);
      setFlipped(false);
      setSessionActive(true);
      if (!rest.length) setSummary(true);
      // Review changes the source mistake, due queue, and derived trends; the
      // three invalidations intentionally mirror the Mistakes page contract.
      await queryClient.invalidateQueries({ queryKey: ["mistakes"] });
      await queryClient.invalidateQueries({ queryKey: ["flashcards"] });
      await queryClient.invalidateQueries({ queryKey: ["trends"] });
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setBusy(false);
    }
  };
  const reset = () => {
    // Reset only clears session presentation/statistics; persisted SRS state is
    // already owned by Core and will be fetched again on the next session.
    setQueue([]);
    setSessionActive(false);
    setSummary(false);
    setFlipped(false);
    setRequeued([]);
    setStats({ reviewed: 0, again: 0, hard: 0, good: 0, easy: 0 });
  };
  if (dueQuery.isLoading || allQuery.isLoading) return <div className="page-content"><div className="skeleton-card" /></div>;
  if (dueQuery.error || allQuery.error) return <div className="page-content"><div className="panel error-card"><strong>{t("error.section")}</strong><p>{String(dueQuery.error ?? allQuery.error)}</p></div></div>;
  const enrolled = (allQuery.data ?? []).filter((mistake) => mistake.review_state !== null).length;
  return (
    <div className="page-content p1-page">
      <Section
        title={t("flashcards.title")}
        description={t("flashcards.description")}
        action={<span className="status-pill on">{t("flashcards.enrolled", { count: enrolled })}</span>}
      />
      {summary ? (
        <section className="panel flashcard-summary">
          <div className="summary-mark">✓</div>
          <h2>{t("flashcards.summaryTitle")}</h2>
          <p className="muted">{t("flashcards.summaryCopy", { count: stats.reviewed })}</p>
          <div className="review-stat-grid">
            <span><strong>{stats.again}</strong>{t("flashcards.again")}</span>
            <span><strong>{stats.hard}</strong>{t("flashcards.hard")}</span>
            <span><strong>{stats.good}</strong>{t("flashcards.good")}</span>
            <span><strong>{stats.easy}</strong>{t("flashcards.easy")}</span>
          </div>
          <button className="button primary" onClick={reset}>{t("flashcards.reviewAgain")}</button>
        </section>
      ) : current ? (
        <section className="flashcard-session">
          <div className="flashcard-toolbar">
            <span>{t("flashcards.progress", { current: stats.reviewed + 1, total: stats.reviewed + activeQueue.length })}</span>
            <span>{current.subject || t("today.general")}</span>
          </div>
          <button
            className={`flashcard-card panel ${flipped ? "flipped" : ""}`}
            onClick={() => setFlipped((value) => !value)}
            type="button"
          >
            <span className="flashcard-side-label">{flipped ? t("flashcards.answer") : t("flashcards.question")}</span>
            <h2>
              <MathText content={current.title || t("mistakes.untitled")} inline />
            </h2>
            {!flipped ? (
              <div className="flashcard-question">
                <MathText content={current.original_question || t("mistakes.noQuestion")} />
              </div>
            ) : (
              <div className="flashcard-answer">
                <div className="flashcard-answer-field">
                  <strong>{t("flashcards.reason")}</strong>
                  <MathText content={current.error_reason || "—"} />
                </div>
                <div className="flashcard-answer-field">
                  <strong>{t("flashcards.wrongSolution")}</strong>
                  <MathText content={current.wrong_solution || "—"} />
                </div>
                <div className="flashcard-answer-field">
                  <strong>{t("flashcards.correctSolution")}</strong>
                  <MathText content={current.correct_solution || "—"} />
                </div>
              </div>
            )}
            <span className="flashcard-hint">{flipped ? t("flashcards.clickQuestion") : t("flashcards.clickAnswer")}</span>
          </button>
          <div className="rating-row">
            <button className="rating-button again" disabled={!flipped || busy} onClick={() => void rate(1)}>
              <strong>{t("flashcards.again")}</strong><span>{t("flashcards.againHint")}</span>
            </button>
            <button className="rating-button hard" disabled={!flipped || busy} onClick={() => void rate(3)}>
              <strong>{t("flashcards.hard")}</strong><span>{t("flashcards.hardHint")}</span>
            </button>
            <button className="rating-button good" disabled={!flipped || busy} onClick={() => void rate(4)}>
              <strong>{t("flashcards.good")}</strong><span>{t("flashcards.goodHint")}</span>
            </button>
            <button className="rating-button easy" disabled={!flipped || busy} onClick={() => void rate(5)}>
              <strong>{t("flashcards.easy")}</strong><span>{t("flashcards.easyHint")}</span>
            </button>
          </div>
        </section>
      ) : (
        <section className="panel p1-empty flashcard-empty">
          <div className="empty-orb">✦</div>
          <h3>{t("flashcards.empty")}</h3>
          <p>{enrolled ? t("flashcards.emptyDue") : t("flashcards.emptyEnroll")}</p>
        </section>
      )}
    </div>
  );
}
