import type { Task } from "../types";
import { useI18n } from "../i18n";
import { AppIcon, formatDate } from "./UIComponents";

export function TaskRow({
  task,
  compact = false,
  onToggle,
  onDelete,
  disabled = false,
}: {
  task: Task;
  compact?: boolean;
  onToggle?: () => void;
  onDelete?: () => void;
  disabled?: boolean;
}) {
  const { language, t } = useI18n();
  const taskType = task.task_type === "Reading" ? t("taskType.reading") : t("taskType.homework");
  const subject = task.subject || t("today.general");
  const dueDate = formatDate(task.due_date, language);

  if (compact) {
    return (
      <div className={`task-row ${task.is_completed ? "completed" : ""}`} data-due={dueDate}>
        <span className="task-bullet" aria-hidden="true" />
        <div className="task-info">
          <strong>{task.title}</strong>
          <span className="task-meta">
            <span>{subject}</span>
            <span>{taskType}</span>
            <span>{t("today.due", { date: dueDate })}</span>
          </span>
        </div>
        <span className={`priority priority-${task.importance}`}>
          {task.importance >= 4 ? t("today.high") : t("today.focus")}
        </span>
      </div>
    );
  }

  return (
    <div className={`task-row large ${task.is_completed ? "completed" : ""}`} data-due={dueDate}>
      {onToggle ? (
        <button
          className={`check ${task.is_completed ? "checked" : ""}`}
          onClick={onToggle}
          disabled={disabled}
          aria-label={task.is_completed ? t("tasks.markIncomplete") : t("tasks.markComplete")}
          type="button"
        >
          {task.is_completed ? "✓" : ""}
        </button>
      ) : (
        <span className="check" aria-hidden="true" />
      )}
      <div className="task-main">
        <strong>{task.title}</strong>
        <span className="task-meta">
          <span>{subject}</span>
          <span>{taskType}</span>
        </span>
      </div>
      <span className={`priority priority-${task.importance}`} title={t("tasks.priorityLabel", { count: task.importance })}>
        P{task.importance}
      </span>
      <time className="task-due" dateTime={task.due_date}>{dueDate}</time>
      {onDelete ? (
        <div className="task-actions">
          <button
            className="icon-btn-subtle"
            onClick={onDelete}
            disabled={disabled}
            title={t("tasks.delete")}
            aria-label={t("tasks.delete")}
            type="button"
          >
            <AppIcon name="trash" className="btn-icon" />
          </button>
        </div>
      ) : (
        <span className="task-actions" aria-hidden="true" />
      )}
    </div>
  );
}
