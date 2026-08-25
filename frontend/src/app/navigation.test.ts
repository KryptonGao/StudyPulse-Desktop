import { describe, expect, it } from "vitest";
import {
  getAllSwitcherItems,
  getWorkspaceForPage,
  WORKSPACES,
  type Page,
} from "./navigation";

describe("navigation model", () => {
  it("maps every leaf page to a valid workspace", () => {
    const allLeafPages: Page[] = [
      "today", "diary",
      "agent", "coach", "simulation", "planner",
      "tasks", "subjects", "exams", "timer",
      "mistakes", "flashcards",
      "trends", "reports", "investment",
      "library",
      "settings",
    ];

    for (const page of allLeafPages) {
      const ws = getWorkspaceForPage(page);
      expect(ws).toBeDefined();
      if (page === "settings") {
        expect(ws).toBe("settings");
      } else {
        expect(["today", "agent", "study", "review", "insights", "library"]).toContain(ws);
      }
    }
  });

  it("contains all 6 workspaces with non-empty subpages", () => {
    expect(WORKSPACES.length).toBe(6);
    for (const ws of WORKSPACES) {
      expect(ws.subPages.length).toBeGreaterThan(0);
      expect(ws.subPages.some((p) => p.id === ws.defaultPage)).toBe(true);
    }
  });

  it("returns all switcher items including settings", () => {
    const items = getAllSwitcherItems();
    expect(items.length).toBe(17);
    expect(items.some((i) => i.id === "settings")).toBe(true);
    expect(items.some((i) => i.id === "today")).toBe(true);
    expect(items.some((i) => i.id === "flashcards")).toBe(true);
  });
});
