export type VisualTheme = "anthropic" | "apple";

export const VISUAL_THEME_STORAGE_KEY = "studypulse.visual-theme";

export function isVisualTheme(value: string | null | undefined): value is VisualTheme {
  return value === "anthropic" || value === "apple";
}

export function detectVisualTheme(storage?: Pick<Storage, "getItem">): VisualTheme {
  const saved = (storage ?? (typeof window === "undefined" ? undefined : window.localStorage))?.getItem(VISUAL_THEME_STORAGE_KEY);
  return isVisualTheme(saved) ? saved : "anthropic";
}

export function saveVisualTheme(theme: VisualTheme, storage?: Pick<Storage, "setItem">): void {
  (storage ?? (typeof window === "undefined" ? undefined : window.localStorage))?.setItem(VISUAL_THEME_STORAGE_KEY, theme);
}
