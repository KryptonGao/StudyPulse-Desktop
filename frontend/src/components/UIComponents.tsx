/* eslint-disable react-refresh/only-export-components */
import type { ReactNode } from "react";
import { useI18n, type Translate } from "../i18n";

const sidebarIconSprite = new URL("../assets/sidebar-icons.svg", import.meta.url).href;

export function AppIcon({ name, className = "nav-icon" }: { name: string; className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <use href={`${sidebarIconSprite}#${name}`} />
    </svg>
  );
}

export function SectionHeader({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="section-header">
      <div className="section-header-titles">
        <h2>{title}</h2>
        {description && <p className="muted">{description}</p>}
      </div>
      {action && <div className="section-header-actions">{action}</div>}
    </div>
  );
}

export function StatCard({
  label,
  value,
  detail,
  accent,
}: {
  label: string;
  value: string | number;
  detail?: string;
  accent?: "default" | "accent" | "blue" | "purple" | "amber";
}) {
  return (
    <div className={`stat-card ${accent ? `stat-accent-${accent}` : ""}`}>
      <span className="stat-label">{label}</span>
      <strong className="stat-value">{value}</strong>
      {detail && <span className="stat-detail">{detail}</span>}
    </div>
  );
}

export function EmptyState({
  title,
  copy,
  action,
  icon,
}: {
  title: string;
  copy: string;
  action?: ReactNode;
  icon?: string;
}) {
  return (
    <div className="empty-state">
      <div className="empty-icon-wrap">
        {icon ? <AppIcon name={icon} className="empty-svg-icon" /> : <span className="empty-dot">○</span>}
      </div>
      <h3>{title}</h3>
      <p>{copy}</p>
      {action && <div className="empty-action">{action}</div>}
    </div>
  );
}

export function PageLoading() {
  return (
    <div className="page-content page-loading-wrapper">
      <div className="skeleton-card" />
      <div className="skeleton-grid">
        <div className="skeleton-card short" />
        <div className="skeleton-card short" />
      </div>
    </div>
  );
}

export function ErrorCard({ error }: { error: unknown }) {
  const { t } = useI18n();
  const message = errorMessage(error, t);
  return (
    <div className="page-content">
      <div className="panel error-card">
        <strong>{t("error.section")}</strong>
        <p>{message}</p>
      </div>
    </div>
  );
}

export function StatusBadge({
  status,
  label,
}: {
  status: "on" | "off" | "warning" | "info";
  label: string;
}) {
  return (
    <span className={`status-badge status-${status}`}>
      <span className="status-dot-indicator" />
      {label}
    </span>
  );
}

export function errorMessage(error: unknown, t: Translate): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) return String(error.message);
  return error instanceof Error ? error.message : t("error.generic");
}

export function formatDate(value: string | null | undefined, language: string): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.valueOf())
    ? value
    : date.toLocaleDateString(language === "en" ? "en-US" : language, {
        month: "short",
        day: "numeric",
      });
}

export function formatDuration(seconds: number, t: Translate): string {
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  return hours
    ? `${t("duration.hours", { count: hours })} ${t("duration.minutes", { count: minutes % 60 })}`
    : t("duration.minutes", { count: minutes });
}
