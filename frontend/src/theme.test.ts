import { describe, expect, it } from "vitest";
import { detectVisualTheme, isVisualTheme, saveVisualTheme, VISUAL_THEME_STORAGE_KEY } from "./theme";

// A tiny in-memory adapter exercises the Storage contract without depending on
// jsdom, browser localStorage, or a persisted user preference.
function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
}

describe("visual theme preference", () => {
  // Unknown values are compatibility input, not new themes; the product
  // default must remain stable when storage contains an old/custom value.
  it("defaults to Anthropic and rejects unknown values", () => {
    const storage = memoryStorage();
    expect(detectVisualTheme(storage)).toBe("anthropic");
    expect(isVisualTheme("custom")).toBe(false);
  });

  it("persists a supported theme", () => {
    // Save and detect use the same adapter to prove the round trip without
    // testing visual rendering or document attributes here.
    const storage = memoryStorage();
    saveVisualTheme("apple", storage);
    expect(storage.getItem(VISUAL_THEME_STORAGE_KEY)).toBe("apple");
    expect(detectVisualTheme(storage)).toBe("apple");
  });
});
