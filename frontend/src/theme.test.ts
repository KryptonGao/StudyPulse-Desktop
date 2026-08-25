import { describe, expect, it } from "vitest";
import {
  APPEARANCE_STORAGE_KEY,
  detectAppearancePreferences,
  getReadableTextColor,
  isValidFontScale,
  isValidHexColor,
  saveAppearancePreferences,
  type AppearancePreferences,
} from "./theme";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
}

describe("appearance preferences", () => {
  it("defaults to OpenAI light with standard scale and null custom colors", () => {
    const storage = memoryStorage();
    const prefs = detectAppearancePreferences(storage);
    expect(prefs.mode).toBe("light");
    expect(prefs.preset).toBe("openai");
    expect(prefs.fontScale).toBe(1);
    expect(prefs.light.accent).toBeNull();
    expect(prefs.dark.accent).toBeNull();
  });

  it("validates 6-digit hex colors strictly", () => {
    expect(isValidHexColor("#10a37f")).toBe(true);
    expect(isValidHexColor("#ffffff")).toBe(true);
    expect(isValidHexColor("#000000")).toBe(true);
    expect(isValidHexColor("#123456")).toBe(true);
    expect(isValidHexColor("red")).toBe(false);
    expect(isValidHexColor("#fff")).toBe(false);
    expect(isValidHexColor("#1234567")).toBe(false);
    expect(isValidHexColor("10a37f")).toBe(false);
    expect(isValidHexColor(null)).toBe(false);
  });

  it("validates font scale bounds", () => {
    expect(isValidFontScale(0.9)).toBe(true);
    expect(isValidFontScale(1)).toBe(true);
    expect(isValidFontScale(1.1)).toBe(true);
    expect(isValidFontScale(1.2)).toBe(true);
    expect(isValidFontScale(0.8)).toBe(false);
    expect(isValidFontScale(1.5)).toBe(false);
    expect(isValidFontScale("1")).toBe(false);
  });

  it("persists and restores custom preferences across light and dark modes", () => {
    const storage = memoryStorage();
    const custom: AppearancePreferences = {
      mode: "dark",
      preset: "ocean",
      fontScale: 1.1,
      light: {
        accent: "#0088cc",
        background: "#f0f4f8",
        text: "#112233",
      },
      dark: {
        accent: "#00aaff",
        background: "#0a1018",
        text: "#eef4ff",
      },
    };

    saveAppearancePreferences(custom, storage);
    const raw = storage.getItem(APPEARANCE_STORAGE_KEY);
    expect(raw).toBeTruthy();

    const loaded = detectAppearancePreferences(storage);
    expect(loaded.mode).toBe("dark");
    expect(loaded.preset).toBe("ocean");
    expect(loaded.fontScale).toBe(1.1);
    expect(loaded.light.accent).toBe("#0088cc");
    expect(loaded.light.background).toBe("#f0f4f8");
    expect(loaded.light.text).toBe("#112233");
    expect(loaded.dark.accent).toBe("#00aaff");
    expect(loaded.dark.background).toBe("#0a1018");
    expect(loaded.dark.text).toBe("#eef4ff");
  });

  it("sanitizes invalid hex colors during save and detection", () => {
    const storage = memoryStorage();
    const invalid: AppearancePreferences = {
      mode: "light",
      preset: "violet",
      fontScale: 1,
      light: {
        accent: "not-a-color",
        background: null,
        text: "#gggggg",
      },
      dark: {
        accent: null,
        background: null,
        text: null,
      },
    };

    saveAppearancePreferences(invalid, storage);
    const loaded = detectAppearancePreferences(storage);
    expect(loaded.light.accent).toBeNull();
    expect(loaded.light.text).toBeNull();
  });

  it("calculates accessible button text contrast using luminance", () => {
    // Light accents should get dark text (#111111)
    expect(getReadableTextColor("#ffffff")).toBe("#111111");
    expect(getReadableTextColor("#f7f7f8")).toBe("#111111");
    expect(getReadableTextColor("#e0e0e0")).toBe("#111111");
    expect(getReadableTextColor("#eab308")).toBe("#111111"); // Yellow

    // Dark accents should get light text (#ffffff)
    expect(getReadableTextColor("#000000")).toBe("#ffffff");
    expect(getReadableTextColor("#10a37f")).toBe("#ffffff"); // OpenAI green
    expect(getReadableTextColor("#0284c7")).toBe("#ffffff"); // Ocean blue
    expect(getReadableTextColor("#7c3aed")).toBe("#ffffff"); // Violet
  });

  it("gracefully falls back when storage has corrupted JSON", () => {
    const storage = memoryStorage();
    storage.setItem(APPEARANCE_STORAGE_KEY, "{corrupted-json...");
    const prefs = detectAppearancePreferences(storage);
    expect(prefs.preset).toBe("openai");
    expect(prefs.mode).toBe("light");
    expect(prefs.fontScale).toBe(1);
  });
});
