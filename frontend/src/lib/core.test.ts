import { describe, expect, it } from "vitest";
import { command, isDesktop } from "./core";

describe("Tauri Core bridge", () => {
  it("does not treat a normal browser runtime as the desktop app", () => {
    expect(isDesktop).toBe(false);
  });

  it("fails closed when commands are called outside Tauri", async () => {
    await expect(command("app_snapshot")).rejects.toThrow("Tauri desktop application");
  });
});
