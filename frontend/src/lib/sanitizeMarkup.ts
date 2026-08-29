export function sanitizeMarkup(markup: string, allowSvg: boolean): string | null {
  if (typeof DOMParser === "undefined") return null;
  const doc = new DOMParser().parseFromString(markup, "text/html");
  const root = allowSvg ? doc.querySelector("svg") : doc.body;
  if (!root) return null;

  root
    .querySelectorAll("script, iframe, object, embed, foreignObject, form")
    .forEach((el) => el.remove());

  // querySelectorAll("*") does not match the root itself, so the root's own
  // attributes (e.g. <svg onload=...>) must be cleaned explicitly.
  for (const el of [root, ...root.querySelectorAll("*")]) {
    for (const attr of [...el.attributes]) {
      const name = attr.name.toLowerCase();
      const val = attr.value.toLowerCase().replace(/\s/g, "");
      if (name.startsWith("on") || val.includes("javascript:") || val.includes("data:text/html")) {
        el.removeAttribute(attr.name);
      }
      if ((name === "href" || name === "xlink:href" || name === "src") && val.startsWith("data:")) {
        el.removeAttribute(attr.name);
      }
    }
  }
  return allowSvg ? root.outerHTML : root.innerHTML;
}

// Sanitized SVG is rendered inside <iframe sandbox="" srcDoc>, never injected
// into the parent document, so any handler that survives sanitization stays
// isolated from the app.
export function svgSandboxDocument(svg: string): string {
  return [
    '<!DOCTYPE html><html><head><meta charset="utf-8"><style>',
    "html,body{margin:0;padding:0}",
    "svg{width:100%;height:auto;display:block}",
    "</style></head><body>",
    svg,
    "</body></html>",
  ].join("");
}
