import { WORKSPACES, type Page, type WorkspaceId } from "../app/navigation";
import { useI18n } from "../i18n";
import { AppIcon } from "./UIComponents";
import { WindowDragRegion } from "./WindowShell";

export function Sidebar({
  activeWorkspace,
  activePage,
  onSelectWorkspace,
  onSelectPage,
  workspacePath,
  isCollapsed,
  onToggleCollapse,
  onOpenQuickSwitcher,
}: {
  activeWorkspace: WorkspaceId | "settings";
  activePage: Page;
  onSelectWorkspace: (ws: WorkspaceId) => void;
  onSelectPage: (page: Page) => void;
  workspacePath: string;
  isCollapsed: boolean;
  onToggleCollapse: () => void;
  onOpenQuickSwitcher: () => void;
}) {
  const { t } = useI18n();

  const folderName =
    workspacePath.split(/[\\/]/).filter(Boolean).at(-1) ?? t("workspace.default");

  return (
    <aside className={`sidebar ${isCollapsed ? "collapsed" : ""}`} aria-label={t("nav.aria")}>
      <WindowDragRegion className="sidebar-window-chrome" />

      <div className="sidebar-header">
        <button
          className="brand"
          onClick={() => onSelectPage("today")}
          title="StudyPulse"
          type="button"
        >
          <div className="brand-mark">SP</div>
          {!isCollapsed && (
            <div className="brand-text">
              <strong>StudyPulse</strong>
              <span>{folderName}</span>
            </div>
          )}
        </button>
        <button
          className="sidebar-collapse-btn"
          onClick={onToggleCollapse}
          title={t("settings.sidebarToggle")}
          aria-label={t("settings.sidebarToggle")}
        >
          <AppIcon name={isCollapsed ? "chevron-right" : "chevron-left"} className="btn-icon" />
        </button>
      </div>

      <div className="sidebar-quick-search">
        <button
          className="quick-search-trigger"
          onClick={onOpenQuickSwitcher}
          title={t("switcher.placeholder")}
          aria-label={t("switcher.placeholder")}
        >
          <AppIcon name="search" className="quick-search-icon" />
          {!isCollapsed && <span className="quick-search-label">{t("switcher.trigger")}</span>}
          {!isCollapsed && <kbd className="quick-search-kbd">⌘K</kbd>}
        </button>
      </div>

      <nav className="nav-list" aria-label={t("nav.aria")}>
        {WORKSPACES.map((ws) => {
          const isActive = activeWorkspace === ws.id;
          return (
            <button
              key={ws.id}
              className={`nav-item ${isActive ? "active" : ""}`}
              onClick={() => onSelectWorkspace(ws.id)}
              title={isCollapsed ? t(ws.labelKey) : undefined}
              aria-label={t(ws.labelKey)}
              aria-current={isActive ? "page" : undefined}
            >
              <AppIcon name={ws.icon} className="nav-icon" />
              {!isCollapsed && <span className="nav-label">{t(ws.labelKey)}</span>}
            </button>
          );
        })}
      </nav>

      <div className="sidebar-bottom">
        <button
          className={`nav-item ${activePage === "settings" ? "active" : ""}`}
          onClick={() => onSelectPage("settings")}
          title={isCollapsed ? t("nav.settings") : undefined}
          aria-label={t("nav.settings")}
          aria-current={activePage === "settings" ? "page" : undefined}
        >
          <AppIcon name="settings" className="nav-icon" />
          {!isCollapsed && <span className="nav-label">{t("nav.settings")}</span>}
        </button>
        {!isCollapsed && (
          <div className="local-note">
            <span className="status-dot" />
            <span>{t("local.stored")}</span>
          </div>
        )}
      </div>
    </aside>
  );
}
