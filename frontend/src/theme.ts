// Visual style is independent from the light/dark color theme. Both values
// are product-supported identifiers used by the document data-style attribute.
export type VisualTheme = "anthropic" | "apple";

// Keep this preference separate from workspace data and the language key.
export const VISUAL_THEME_STORAGE_KEY = "studypulse.visual-theme";

export function isVisualTheme(value: string | null | undefined): value is VisualTheme {
  // Storage is untrusted input; only the two shipped styles may reach React.
  return value === "anthropic" || value === "apple";
}

export function detectVisualTheme(storage?: Pick<Storage, "getItem">): VisualTheme {
  // Injectable storage keeps tests and non-browser callers deterministic. An
  // absent/unknown value intentionally falls back to the Anthropic default.
  const saved = (storage ?? (typeof window === "undefined" ? undefined : window.localStorage))?.getItem(VISUAL_THEME_STORAGE_KEY);
  return isVisualTheme(saved) ? saved : "anthropic";
}

export function saveVisualTheme(theme: VisualTheme, storage?: Pick<Storage, "setItem">): void {
  // Only the style identifier is persisted; no workspace content or provider
  // credential is involved in this preference write.
  (storage ?? (typeof window === "undefined" ? undefined : window.localStorage))?.setItem(VISUAL_THEME_STORAGE_KEY, theme);
}
