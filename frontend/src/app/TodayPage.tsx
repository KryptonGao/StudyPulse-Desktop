import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { core } from "../lib/core";
import { useI18n } from "../i18n";
import type { AppSnapshot } from "../types";
import {
  AppIcon,
  EmptyState,
  ErrorCard,
  formatDate,
  formatDuration,
  PageLoading,
  SectionHeader,
  StatCard,
} from "../components/UIComponents";
import { TaskRow } from "../components/TaskRow";

export function TodayPage({
  onStartAgentTurn,
}: {
  provider?: AppSnapshot["provider"];
  onStartAgentTurn?: (prompt: string) => void;
}) {
  const { language, t } = useI18n();
  const [quickPrompt, setQuickPrompt] = useState("");

  const query = useQuery({ queryKey: ["today"], queryFn: core.today });
  const tasks = useQuery({ queryKey: ["tasks"], queryFn: core.tasks });
  const exams = useQuery({ queryKey: ["exams"], queryFn: core.exams });

  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;
  if (tasks.error) return <ErrorCard error={tasks.error} />;
  if (exams.error) return <ErrorCard error={exams.error} />;

  const value = query.data!;
  const openTasks = tasks.data?.filter((task) => !task.is_completed).slice(0, 5) ?? [];
  const nextExam = exams.data?.slice().sort((a, b) => a.exam_date.localeCompare(b.exam_date))[0];

  const handleQuickAsk = () => {
    const trimmed = quickPrompt.trim();
    if (!trimmed || !onStartAgentTurn) return;
    onStartAgentTurn(trimmed);
    setQuickPrompt("");
  };

  return (
    <div className="page-content today-page">
      {/* Quick Agent Prompt Box */}
      <div className="today-hero-panel">
        <div className="today-hero-header">
          <div className="today-hero-title">
            <span className="eyebrow">{formatDate(new Date().toISOString(), language)}</span>
            <h2>{t("today.greeting")}</h2>
            <p className="muted">{t("today.heroCopy")}</p>
          </div>
          <div className="today-hero-stats">
            <span className="streak-pill">
              <AppIcon name="sparkles" className="streak-icon" />
              {t("today.streak", { count: value.streak_days })}
            </span>
          </div>
        </div>

        {onStartAgentTurn && (
          <div className="today-quick-prompt">
            <div className="quick-prompt-input-wrap">
              <AppIcon name="agent" className="quick-prompt-icon" />
              <input
                type="text"
                placeholder={t("today.quickAsk")}
                value={quickPrompt}
                onChange={(e) => setQuickPrompt(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleQuickAsk();
                }}
              />
              <button
                className="button primary small"
                onClick={handleQuickAsk}
                disabled={!quickPrompt.trim()}
              >
                {t("today.quickAskButton")} ↗
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Primary Metrics Grid */}
      <div className="stat-grid">
        <StatCard
          label={t("today.openTasks")}
          value={value.open_task_count}
          detail={openTasks[0]?.title ?? t("today.nothingUrgent")}
          accent="default"
        />
        <StatCard
          label={t("today.studyTime")}
          value={formatDuration(value.study_minutes * 60, t)}
          detail={t("today.streak", { count: value.streak_days })}
          accent="accent"
        />
        <StatCard
          label={t("today.dueMistakes")}
          value={value.due_mistake_count}
          detail={t("today.readyReview")}
          accent="purple"
        />
        <StatCard
          label={t("today.upcomingExams")}
          value={value.upcoming_exam_ids.length}
          detail={nextExam?.name ?? t("today.noExamsSoon")}
          accent="blue"
        />
      </div>

      {/* Two Column Layout */}
      <div className="two-column">
        <section className="panel">
          <SectionHeader
            title={t("today.nextTitle")}
            description={t("today.nextDescription")}
          />
          {openTasks.length ? (
            <div className="task-preview">
              {openTasks.map((task) => (
                <TaskRow task={task} compact key={task.id} />
              ))}
            </div>
          ) : (
            <EmptyState title={t("today.clearTitle")} copy={t("today.clearCopy")} />
          )}
        </section>

        <section className="panel reflection-panel">
          <SectionHeader
            title={t("today.noteTitle")}
            description={t("today.noteDescription")}
          />
          <div className="reflection-content">
            <p className="reflection-quote">{t("today.quote")}</p>
            {value.suggestions[0] && (
              <div className="phase-chip">
                <span className="chip-spark">✦</span> {value.suggestions[0]}
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
