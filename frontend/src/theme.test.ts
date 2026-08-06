import { describe, expect, it } from "vitest";
import { detectVisualTheme, isVisualTheme, saveVisualTheme, VISUAL_THEME_STORAGE_KEY } from "./theme";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
}

describe("visual theme preference", () => {
  it("defaults to Anthropic and rejects unknown values", () => {
    const storage = memoryStorage();
    expect(detectVisualTheme(storage)).toBe("anthropic");
    expect(isVisualTheme("custom")).toBe(false);
  });

  it("persists a supported theme", () => {
    const storage = memoryStorage();
    saveVisualTheme("apple", storage);
    expect(storage.getItem(VISUAL_THEME_STORAGE_KEY)).toBe("apple");
    expect(detectVisualTheme(storage)).toBe("apple");
  });
});
