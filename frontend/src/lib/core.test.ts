import { describe, expect, it } from "vitest";
import { command, isDesktop } from "./core";

// These tests run under Vitest’s Node environment, so a normal test process
// must not be mistaken for the desktop host merely because `window` is absent.
describe("Tauri Core bridge", () => {
  it("does not treat a normal browser runtime as the desktop app", () => {
    expect(isDesktop).toBe(false);
  });

  it("fails closed when commands are called outside Tauri", async () => {
    // The guard must reject before invoking the Tauri plugin and preserve the
    // user-facing boundary error used by browser-preview callers.
    await expect(command("app_snapshot")).rejects.toThrow("Tauri desktop application");
  });
});
