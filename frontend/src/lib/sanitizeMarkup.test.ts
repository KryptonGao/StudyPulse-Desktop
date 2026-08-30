// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { sanitizeMarkup, svgSandboxDocument } from "./sanitizeMarkup";

describe("sanitizeMarkup", () => {
  it("strips event handler attributes from the svg root itself", () => {
    const cleaned = sanitizeMarkup(
      '<svg onload="alert(1)" onclick="alert(2)" width="10"><circle r="4"/></svg>',
      true,
    );
    expect(cleaned).not.toContain("onload");
    expect(cleaned).not.toContain("onclick");
    expect(cleaned).toContain("<svg");
    expect(cleaned).toContain("<circle");
  });

  it("strips javascript: urls from the svg root and its descendants", () => {
    const cleaned = sanitizeMarkup(
      '<svg xlink:href="javascript:alert(1)"><a href="javascript:alert(2)">x</a></svg>',
      true,
    );
    expect(cleaned).not.toContain("javascript:");
  });

  it("strips data: hrefs from the svg root itself", () => {
    const cleaned = sanitizeMarkup('<svg href="data:text/html;base64,PHNjcmlwdD4="></svg>', true);
    expect(cleaned).not.toContain("data:text/html");
  });

  it("keeps sanitizing the html body branch", () => {
    const cleaned = sanitizeMarkup(
      '<div><script>alert(1)</script><p onclick="alert(2)">ok</p></div>',
      false,
    );
    expect(cleaned).not.toContain("script");
    expect(cleaned).not.toContain("onclick");
    expect(cleaned).toContain("<p>ok</p>");
  });

  it("returns null when no svg root exists", () => {
    expect(sanitizeMarkup("<div>plain</div>", true)).toBeNull();
  });
});

describe("svgSandboxDocument", () => {
  it("wraps the svg in a standalone document for the sandboxed iframe", () => {
    const doc = svgSandboxDocument('<svg width="10"><circle r="4"/></svg>');
    expect(doc.startsWith("<!DOCTYPE html>")).toBe(true);
    expect(doc).toContain('charset="utf-8"');
    expect(doc).toContain('<svg width="10"><circle r="4"/></svg>');
  });

  it("carries no executable content when fed sanitized hostile svg", () => {
    const svg = sanitizeMarkup(
      '<svg onload="alert(1)"><script>alert(2)</script><circle r="4"/></svg>',
      true,
    );
    expect(svg).not.toBeNull();
    const doc = svgSandboxDocument(svg!);
    expect(doc).not.toContain("onload");
    expect(doc).not.toContain("<script");
    expect(doc).toContain("<circle");
  });
});
