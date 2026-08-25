import { useEffect, useState, useCallback } from "react";
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { useQuery } from "@tanstack/react-query";
import { chooseDirectory, core, isDesktop } from "../lib/core";
import { useI18n } from "../i18n";
import {
  applyAppearanceToDom,
  detectAppearancePreferences,
  saveAppearancePreferences,
  type AppearancePreferences,
} from "../theme";
import {
  getWorkspaceForPage,
  WORKSPACES,
  type Page,
  type WorkspaceId,
} from "./navigation";

import { ToastProvider, useToast } from "../components/Toast";
import { ConfirmDialogProvider } from "../components/ConfirmDialog";
import { QuickSwitcher } from "../components/QuickSwitcher";
import { Sidebar } from "../components/Sidebar";
import { Topbar } from "../components/Topbar";
import { PageLoading, ErrorCard, EmptyState } from "../components/UIComponents";
import { WindowDragRegion, WindowShell } from "../components/WindowShell";

import { TodayPage } from "./TodayPage";
import { AgentPage } from "./AgentPage";
import { DiaryPage, FlashcardsPage, TrendsPage } from "./P1Pages";
import { CoachPage, ExamSimulationPage, ReversePlannerPage, ReportsPage } from "./P2Pages";
import { TasksPage, SubjectsPage, ExamsPage, TimerPage } from "./StudyPages";
import { MistakesPage } from "./MistakesPage";
import { InvestmentPage } from "./InsightsPages";
import { LibraryPage } from "./LibraryPage";
import { SettingsPage } from "./SettingsPage";

export function App() {
  return (
    <ToastProvider>
      <ConfirmDialogProvider>
        <AppContent />
      </ConfirmDialogProvider>
    </ToastProvider>
  );
}

export default App;

function AppContent() {
  const { t } = useI18n();
  const { showToast } = useToast();

  // Appearance Preferences
  const [appearance, setAppearance] = useState<AppearancePreferences>(() =>
    detectAppearancePreferences()
  );

  useEffect(() => {
    applyAppearanceToDom(appearance);
    saveAppearancePreferences(appearance);
  }, [appearance]);

  // Core Snapshot Query
  const snapshotQuery = useQuery({
    queryKey: ["snapshot"],
    queryFn: core.snapshot,
    staleTime: 1000,
  });

  // Navigation State
  const [activeWorkspace, setActiveWorkspace] = useState<WorkspaceId | "settings">("today");
  const [activePage, setActivePage] = useState<Page>("today");
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const [isQuickSwitcherOpen, setIsQuickSwitcherOpen] = useState(false);

  // Agent Initial Goal Transfer
  const [initialAgentGoal, setInitialAgentGoal] = useState<string>();

  // Deep Link Auth Handler
  useEffect(() => {
    if (!isDesktop) return;
    let unlisten: (() => void) | undefined;
    void onOpenUrl((urls) => {
      for (const url of urls) {
        if (url.startsWith("studypulse://auth/callback")) {
          void core
            .completeCloudAuth(url)
            .then(() => {
              void snapshotQuery.refetch();
              showToast(t("settings.cloudConnected"), "success");
            })
            .catch((error) => {
              showToast(error instanceof Error ? error.message : String(error), "error");
            });
        }
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [snapshotQuery, showToast, t]);

  // Restore saved AI configuration on startup
  useEffect(() => {
    void core.restoreAi()
      .then(() => snapshotQuery.refetch())
      .catch(() => {
        // Safe silent fallback if no credentials exist
      });
  }, [snapshotQuery]);

  // Global Keyboard Shortcuts (⌘K / Ctrl+K)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setIsQuickSwitcherOpen((prev) => !prev);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const handleSelectWorkspace = useCallback((ws: WorkspaceId) => {
    setActiveWorkspace(ws);
    // Select default subpage for workspace
    const wsItem = WORKSPACES.find((w) => w.id === ws);
    if (wsItem) {
      setActivePage(wsItem.defaultPage);
    }
  }, []);

  const handleSelectPage = useCallback((page: Page) => {
    setActivePage(page);
    setActiveWorkspace(getWorkspaceForPage(page));
  }, []);

  const handleStartAgentTurnFromToday = (prompt: string) => {
    setInitialAgentGoal(prompt);
    setActiveWorkspace("agent");
    setActivePage("agent");
  };

  const handleInitialAgentGoalHandled = useCallback(() => {
    setInitialAgentGoal(undefined);
  }, []);

  if (snapshotQuery.isLoading) {
    return (
      <WindowShell className="standalone-shell">
        <WindowDragRegion className="standalone-drag-region" />
        <PageLoading />
      </WindowShell>
    );
  }
  if (snapshotQuery.error) {
    return (
      <WindowShell className="standalone-shell">
        <WindowDragRegion className="standalone-drag-region" />
        <ErrorCard error={snapshotQuery.error} />
      </WindowShell>
    );
  }

  const snapshot = snapshotQuery.data!;
  const rootPath = snapshot.workspace?.root_path;

  if (!rootPath) {
    return (
      <WindowShell className="standalone-shell">
        <WindowDragRegion className="standalone-drag-region" />
        <WelcomePage
          onOpened={() => void snapshotQuery.refetch()}
          showToast={showToast}
        />
      </WindowShell>
    );
  }

  const providerReady = Boolean(
    snapshot.provider?.cloud_account || snapshot.provider?.byok_config
  );
  const userEmail = snapshot.provider?.cloud_account?.email;

  return (
    <WindowShell className="app-workbench">
      <Sidebar
        activeWorkspace={activeWorkspace}
        activePage={activePage}
        onSelectWorkspace={handleSelectWorkspace}
        onSelectPage={handleSelectPage}
        workspacePath={rootPath}
        isCollapsed={isSidebarCollapsed}
        onToggleCollapse={() => setIsSidebarCollapsed((v) => !v)}
        onOpenQuickSwitcher={() => setIsQuickSwitcherOpen(true)}
      />

      <div className="main-viewport">
        <Topbar
          activeWorkspace={activeWorkspace}
          activePage={activePage}
          onSelectPage={handleSelectPage}
          onOpenQuickSwitcher={() => setIsQuickSwitcherOpen(true)}
          userEmail={userEmail}
          providerReady={providerReady}
        />

        <main className="page-host" role="main">
          {activePage === "today" && (
            <TodayPage
              provider={snapshot.provider}
              onStartAgentTurn={handleStartAgentTurnFromToday}
            />
          )}
          {activePage === "diary" && <DiaryPage />}

          {activePage === "agent" && (
            <AgentPage
              workspaceId={snapshot.workspace!.id}
              provider={snapshot.provider}
              initialGoal={initialAgentGoal}
              onInitialGoalHandled={handleInitialAgentGoalHandled}
            />
          )}
          {activePage === "coach" && <CoachPage provider={snapshot.provider} />}
          {activePage === "simulation" && (
            <ExamSimulationPage provider={snapshot.provider} />
          )}
          {activePage === "planner" && (
            <ReversePlannerPage provider={snapshot.provider} />
          )}

          {activePage === "tasks" && <TasksPage />}
          {activePage === "subjects" && <SubjectsPage />}
          {activePage === "exams" && <ExamsPage provider={snapshot.provider} />}
          {activePage === "timer" && <TimerPage />}

          {activePage === "mistakes" && (
            <MistakesPage provider={snapshot.provider} />
          )}
          {activePage === "flashcards" && <FlashcardsPage />}

          {activePage === "trends" && <TrendsPage />}
          {activePage === "reports" && <ReportsPage />}
          {activePage === "investment" && <InvestmentPage />}

          {activePage === "library" && <LibraryPage />}

          {activePage === "settings" && (
            <SettingsPage
              provider={snapshot.provider}
              onChanged={() => void snapshotQuery.refetch()}
              appearance={appearance}
              onAppearanceChange={setAppearance}
              workspacePath={rootPath}
            />
          )}
        </main>
      </div>

      <QuickSwitcher
        isOpen={isQuickSwitcherOpen}
        onClose={() => setIsQuickSwitcherOpen(false)}
        onSelectPage={handleSelectPage}
      />
    </WindowShell>
  );
}

function WelcomePage({
  onOpened,
  showToast,
}: {
  onOpened: () => void;
  showToast: (msg: string, type?: "info" | "success" | "error") => void;
}) {
  const { t } = useI18n();
  const [opening, setOpening] = useState(false);

  async function openExisting() {
    setOpening(true);
    try {
      const path = await chooseDirectory(t("dialog.chooseWorkspace"));
      if (path) {
        await core.openWorkspace(path);
        onOpened();
      }
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setOpening(false);
    }
  }

  async function createNew() {
    setOpening(true);
    try {
      const path = await chooseDirectory(t("dialog.createWorkspace"));
      if (path) {
        await core.createWorkspace(path);
        onOpened();
      }
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setOpening(false);
    }
  }

  return (
    <div className="welcome-wrapper">
      <div className="welcome-card panel">
        <div className="brand-mark large">SP</div>
        <h2>{t("brand.title")}</h2>
        <p className="muted">{t("brand.description")}</p>

        <div className="welcome-actions">
          <button
            className="button primary"
            onClick={() => void openExisting()}
            disabled={opening}
          >
            {t("welcome.openExisting")}
          </button>
          <button
            className="button secondary"
            onClick={() => void createNew()}
            disabled={opening}
          >
            {t("welcome.createNew")}
          </button>
        </div>

        <div className="welcome-footer">
          <EmptyState
            title={t("welcome.localFirst")}
            copy={t("welcome.localFirstCopy")}
          />
        </div>
      </div>
    </div>
  );
}
