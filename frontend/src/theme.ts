export type AppearanceMode = "light" | "dark";
export type AppearancePreset = "openai" | "ocean" | "violet";
export type FontScale = 0.9 | 1 | 1.1 | 1.2;

export interface AppearancePreferences {
  mode: AppearanceMode;
  preset: AppearancePreset;
  fontScale: FontScale;
  light: {
    accent: string | null;
    background: string | null;
    text: string | null;
  };
  dark: {
    accent: string | null;
    background: string | null;
    text: string | null;
  };
}

export const APPEARANCE_STORAGE_KEY = "studypulse.appearance-v2";
export const LEGACY_THEME_STORAGE_KEY = "studypulse.visual-theme";

export const PRESET_DEFAULTS: Record<AppearancePreset, {
  light: { accent: string; background: string; text: string; surface: string; surfaceMuted: string; line: string };
  dark: { accent: string; background: string; text: string; surface: string; surfaceMuted: string; line: string };
}> = {
  openai: {
    light: {
      accent: "#10a37f",
      background: "#f7f7f8",
      text: "#202123",
      surface: "#ffffff",
      surfaceMuted: "#ececf1",
      line: "#e5e5e7",
    },
    dark: {
      accent: "#10a37f",
      background: "#212121",
      text: "#ececec",
      surface: "#2f2f2f",
      surfaceMuted: "#3a3a3a",
      line: "#424242",
    },
  },
  ocean: {
    light: {
      accent: "#0284c7",
      background: "#f7f8fa",
      text: "#0f172a",
      surface: "#ffffff",
      surfaceMuted: "#eef1f4",
      line: "#e2e5e9",
    },
    dark: {
      accent: "#38bdf8",
      background: "#1c1d20",
      text: "#f0f9ff",
      surface: "#25272b",
      surfaceMuted: "#303239",
      line: "#3a3c42",
    },
  },
  violet: {
    light: {
      accent: "#7c3aed",
      background: "#f8f8fa",
      text: "#1e1b4b",
      surface: "#ffffff",
      surfaceMuted: "#f0eff3",
      line: "#e4e2e8",
    },
    dark: {
      accent: "#a78bfa",
      background: "#1c1c20",
      text: "#f5f3ff",
      surface: "#28262e",
      surfaceMuted: "#34313c",
      line: "#403c49",
    },
  },
};

export function isValidHexColor(value: unknown): value is string {
  return typeof value === "string" && /^#[0-9a-fA-F]{6}$/.test(value.trim());
}

export function isValidFontScale(value: unknown): value is FontScale {
  return value === 0.9 || value === 1 || value === 1.1 || value === 1.2;
}

export function isValidAppearanceMode(value: unknown): value is AppearanceMode {
  return value === "light" || value === "dark";
}

export function isValidAppearancePreset(value: unknown): value is AppearancePreset {
  return value === "openai" || value === "ocean" || value === "violet";
}

export function defaultAppearancePreferences(): AppearancePreferences {
  return {
    mode: "light",
    preset: "openai",
    fontScale: 1,
    light: {
      accent: null,
      background: null,
      text: null,
    },
    dark: {
      accent: null,
      background: null,
      text: null,
    },
  };
}

export function sanitizeColor(value: unknown): string | null {
  return isValidHexColor(value) ? value.trim().toLowerCase() : null;
}

export function detectAppearancePreferences(storage?: Pick<Storage, "getItem">): AppearancePreferences {
  const store = storage ?? (typeof window === "undefined" ? undefined : window.localStorage);
  if (!store) return defaultAppearancePreferences();

  const saved = store.getItem(APPEARANCE_STORAGE_KEY);
  if (saved) {
    try {
      const parsed = JSON.parse(saved) as Record<string, unknown>;
      if (parsed && typeof parsed === "object") {
        const mode: AppearanceMode = isValidAppearanceMode(parsed.mode) ? parsed.mode : "light";
        const preset: AppearancePreset = isValidAppearancePreset(parsed.preset) ? parsed.preset : "openai";
        const fontScale: FontScale = isValidFontScale(parsed.fontScale) ? parsed.fontScale : 1;

        const lightObj = parsed.light && typeof parsed.light === "object" ? parsed.light as Record<string, unknown> : {};
        const darkObj = parsed.dark && typeof parsed.dark === "object" ? parsed.dark as Record<string, unknown> : {};

        return {
          mode,
          preset,
          fontScale,
          light: {
            accent: sanitizeColor(lightObj.accent),
            background: sanitizeColor(lightObj.background),
            text: sanitizeColor(lightObj.text),
          },
          dark: {
            accent: sanitizeColor(darkObj.accent),
            background: sanitizeColor(darkObj.background),
            text: sanitizeColor(darkObj.text),
          },
        };
      }
    } catch {
      // JSON parse failed; fall through to defaults
    }
  }

  // Safe migration fallback from legacy visual theme or defaults
  return defaultAppearancePreferences();
}

export function saveAppearancePreferences(prefs: AppearancePreferences, storage?: Pick<Storage, "setItem">): void {
  const store = storage ?? (typeof window === "undefined" ? undefined : window.localStorage);
  if (!store) return;

  const sanitized: AppearancePreferences = {
    mode: isValidAppearanceMode(prefs.mode) ? prefs.mode : "light",
    preset: isValidAppearancePreset(prefs.preset) ? prefs.preset : "openai",
    fontScale: isValidFontScale(prefs.fontScale) ? prefs.fontScale : 1,
    light: {
      accent: sanitizeColor(prefs.light?.accent),
      background: sanitizeColor(prefs.light?.background),
      text: sanitizeColor(prefs.light?.text),
    },
    dark: {
      accent: sanitizeColor(prefs.dark?.accent),
      background: sanitizeColor(prefs.dark?.background),
      text: sanitizeColor(prefs.dark?.text),
    },
  };

  store.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(sanitized));
}

/**
 * Calculates luminance for a 6-digit hex color and returns either dark (#111111)
 * or light (#ffffff) text color to guarantee high-contrast readability on buttons.
 */
export function getReadableTextColor(hexColor: string): string {
  if (!isValidHexColor(hexColor)) return "#ffffff";
  const hex = hexColor.replace("#", "");
  const r = parseInt(hex.substring(0, 2), 16) / 255;
  const g = parseInt(hex.substring(2, 4), 16) / 255;
  const b = parseInt(hex.substring(4, 6), 16) / 255;

  const toLinear = (c: number) => (c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
  const lum = 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);

  return lum > 0.45 ? "#111111" : "#ffffff";
}

/**
 * Converts hex to rgba string with given alpha.
 */
function hexToRgba(hex: string, alpha: number): string {
  const clean = hex.replace("#", "");
  const r = parseInt(clean.substring(0, 2), 16);
  const g = parseInt(clean.substring(2, 4), 16);
  const b = parseInt(clean.substring(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/**
 * Applies active preferences to document attributes and CSS custom properties.
 */
export function applyAppearanceToDom(prefs: AppearancePreferences): void {
  if (typeof document === "undefined") return;

  const root = document.documentElement;
  const mode = prefs.mode;
  const preset = prefs.preset;
  const presetColors = PRESET_DEFAULTS[preset][mode];

  root.dataset.theme = mode;
  root.dataset.preset = preset;

  const customColors = mode === "light" ? prefs.light : prefs.dark;
  const accent = customColors.accent || presetColors.accent;
  const background = customColors.background || presetColors.background;
  const text = customColors.text || presetColors.text;
  const accentText = getReadableTextColor(accent);

  root.style.setProperty("--font-scale", String(prefs.fontScale));
  root.style.setProperty("--accent-base", accent);
  root.style.setProperty("--accent-hover", accent);
  root.style.setProperty("--accent-contrast", accentText);
  root.style.setProperty("--accent-subtle", hexToRgba(accent, mode === "dark" ? 0.2 : 0.12));
  root.style.setProperty("--bg-app-solid", background);
  root.style.setProperty("--bg-app", hexToRgba(background, mode === "dark" ? 0.8 : 0.74));
  root.style.setProperty("--bg-surface-solid", presetColors.surface);
  root.style.setProperty("--bg-surface", hexToRgba(presetColors.surface, mode === "dark" ? 0.86 : 0.82));
  root.style.setProperty("--bg-subtle", hexToRgba(presetColors.surfaceMuted, mode === "dark" ? 0.52 : 0.62));
  root.style.setProperty("--text-primary", text);
  root.style.setProperty("--border-subtle", hexToRgba(presetColors.line, mode === "dark" ? 0.68 : 0.72));

  // Legacy mappings for backwards compatibility
  root.style.setProperty("--accent", accent);
  root.style.setProperty("--accent-text", accentText);
  root.style.setProperty("--canvas", background);
  root.style.setProperty("--ink", text);
}
