import type { ReactNode } from "react";

// Keep the Tauri-provided drag bridge as the single source of truth. The
// explicit value means this is a direct-only region: descendants can never
// become draggable accidentally if chrome is added inside the node later.
const TAURI_DRAG_REGION = "true";

export function WindowShell({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return <div className={`window-shell ${className}`.trim()}>{children}</div>;
}

export function WindowDragRegion({ className = "" }: { className?: string }) {
  return (
    <div
      className={`window-drag-region ${className}`.trim()}
      data-tauri-drag-region={TAURI_DRAG_REGION}
      aria-hidden="true"
    />
  );
}
