export type Page =
  | "today"
  | "agent"
  | "tasks"
  | "subjects"
  | "exams"
  | "coach"
  | "simulation"
  | "planner"
  | "reports"
  | "mistakes"
  | "diary"
  | "trends"
  | "flashcards"
  | "timer"
  | "investment"
  | "library"
  | "settings";

export type WorkspaceId =
  | "today"
  | "agent"
  | "study"
  | "review"
  | "insights"
  | "library";

export interface WorkspaceItem {
  id: WorkspaceId;
  labelKey: string;
  icon: string;
  defaultPage: Page;
  subPages: { id: Page; labelKey: string; icon: string }[];
}

export const WORKSPACES: WorkspaceItem[] = [
  {
    id: "today",
    labelKey: "workspace.today",
    icon: "today",
    defaultPage: "today",
    subPages: [
      { id: "today", labelKey: "nav.today", icon: "today" },
      { id: "diary", labelKey: "nav.diary", icon: "diary" },
    ],
  },
  {
    id: "agent",
    labelKey: "workspace.agent",
    icon: "agent",
    defaultPage: "agent",
    subPages: [
      { id: "agent", labelKey: "nav.agent", icon: "agent" },
      { id: "coach", labelKey: "nav.coach", icon: "coach" },
      { id: "simulation", labelKey: "nav.simulation", icon: "simulation" },
      { id: "planner", labelKey: "nav.planner", icon: "planner" },
    ],
  },
  {
    id: "study",
    labelKey: "workspace.study",
    icon: "study",
    defaultPage: "tasks",
    subPages: [
      { id: "tasks", labelKey: "nav.tasks", icon: "tasks" },
      { id: "subjects", labelKey: "nav.subjects", icon: "subjects" },
      { id: "exams", labelKey: "nav.exams", icon: "exams" },
      { id: "timer", labelKey: "nav.timer", icon: "timer" },
    ],
  },
  {
    id: "review",
    labelKey: "workspace.review",
    icon: "review",
    defaultPage: "mistakes",
    subPages: [
      { id: "mistakes", labelKey: "nav.mistakes", icon: "mistakes" },
      { id: "flashcards", labelKey: "nav.flashcards", icon: "flashcards" },
    ],
  },
  {
    id: "insights",
    labelKey: "workspace.insights",
    icon: "insights",
    defaultPage: "trends",
    subPages: [
      { id: "trends", labelKey: "nav.trends", icon: "trends" },
      { id: "reports", labelKey: "nav.reports", icon: "reports" },
      { id: "investment", labelKey: "nav.investment", icon: "investment" },
    ],
  },
  {
    id: "library",
    labelKey: "workspace.library",
    icon: "library",
    defaultPage: "library",
    subPages: [
      { id: "library", labelKey: "nav.library", icon: "library" },
    ],
  },
];

/**
 * Resolves the parent workspace for a given leaf page.
 */
export function getWorkspaceForPage(page: Page): WorkspaceId | "settings" {
  if (page === "settings") return "settings";
  for (const ws of WORKSPACES) {
    if (ws.subPages.some((p) => p.id === page)) {
      return ws.id;
    }
  }
  return "today";
}

/**
 * Returns all searchable items for the Quick Switcher command palette.
 */
export interface SwitcherItem {
  id: Page;
  page: Page;
  workspaceId: WorkspaceId | "settings";
  labelKey: string;
  workspaceLabelKey: string;
  icon: string;
  keywords?: string[];
}

export function getAllSwitcherItems(): SwitcherItem[] {
  const items: SwitcherItem[] = [];

  for (const ws of WORKSPACES) {
    for (const sub of ws.subPages) {
      items.push({
        id: sub.id,
        page: sub.id,
        workspaceId: ws.id,
        labelKey: sub.labelKey,
        workspaceLabelKey: ws.labelKey,
        icon: sub.icon,
      });
    }
  }

  items.push({
    id: "settings",
    page: "settings",
    workspaceId: "settings",
    labelKey: "nav.settings",
    workspaceLabelKey: "nav.settings",
    icon: "settings",
  });

  return items;
}
