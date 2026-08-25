import { WORKSPACES, type Page, type WorkspaceId } from "../app/navigation";
import { useI18n } from "../i18n";
import { AppIcon } from "./UIComponents";
import { WindowDragRegion } from "./WindowShell";

export function Topbar({
  activeWorkspace,
  activePage,
  onSelectPage,
  onOpenQuickSwitcher,
  userEmail,
  providerReady,
}: {
  activeWorkspace: WorkspaceId | "settings";
  activePage: Page;
  onSelectPage: (page: Page) => void;
  onOpenQuickSwitcher: () => void;
  userEmail?: string;
  providerReady: boolean;
}) {
  const { t } = useI18n();

  const currentWorkspace = WORKSPACES.find((ws) => ws.id === activeWorkspace);
  const subPages = currentWorkspace?.subPages ?? [];

  const wsLabel = activeWorkspace === "settings"
    ? t("nav.settings")
    : currentWorkspace
    ? t(currentWorkspace.labelKey)
    : t("brand.localWorkspace");

  const pageLabel = activePage === "settings"
    ? t("nav.settings")
    : t(`nav.${activePage}`);

  const isSameOrSingle = subPages.length <= 1 || wsLabel === pageLabel || activeWorkspace === (activePage as unknown as WorkspaceId);

  return (
    <header className="topbar">
      <div className="topbar-left">
        <div className="topbar-breadcrumb">
          {isSameOrSingle ? (
            <strong className="breadcrumb-page">{wsLabel}</strong>
          ) : (
            <>
              <span className="breadcrumb-workspace">{wsLabel}</span>
              <span className="breadcrumb-sep">/</span>
              <strong className="breadcrumb-page">{pageLabel}</strong>
            </>
          )}
        </div>

        {subPages.length > 1 && (
          <nav className="workspace-tabs" aria-label={t("topbar.subPages")}>
            {subPages.map((sub) => {
              const isActive = activePage === sub.id;
              return (
                <button
                  key={sub.id}
                  className={`workspace-tab ${isActive ? "active" : ""}`}
                  onClick={() => onSelectPage(sub.id)}
                  aria-current={isActive ? "page" : undefined}
                >
                  <AppIcon name={sub.icon} className="tab-icon" />
                  <span>{t(sub.labelKey)}</span>
                </button>
              );
            })}
          </nav>
        )}
      </div>

      <WindowDragRegion className="topbar-drag-fill" />

      <div className="topbar-actions window-no-drag">
        <button
          className="topbar-btn search-btn"
          onClick={onOpenQuickSwitcher}
          title={t("switcher.placeholder")}
          aria-label={t("switcher.placeholder")}
        >
          <AppIcon name="search" className="btn-icon" />
          <span className="btn-text">{t("switcher.trigger")}</span>
          <kbd className="btn-kbd">⌘K</kbd>
        </button>

        <button
          className="avatar"
          onClick={() => onSelectPage("settings")}
          title={userEmail ? t("topbar.connected", { email: userEmail }) : providerReady ? t("topbar.aiConnected") : t("nav.settings")}
          aria-label={t("topbar.accountSettings")}
        >
          {userEmail ? (
            userEmail.slice(0, 1).toUpperCase()
          ) : providerReady ? (
            <span className="avatar-dot ready" />
          ) : (
            "•"
          )}
        </button>
      </div>
    </header>
  );
}
