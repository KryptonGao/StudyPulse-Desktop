import React from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";

// Customize sanitize schema to allow KaTeX math markup and classes
const katexSanitizeSchema = {
  ...defaultSchema,
  attributes: {
    ...defaultSchema.attributes,
    "*": [
      ...(defaultSchema.attributes?.["*"] || []),
      "className",
      "style",
      "aria-hidden",
    ],
    span: [
      ...(defaultSchema.attributes?.span || []),
      "className",
      "style",
      "aria-hidden",
    ],
    math: ["xmlns", "display"],
    annotation: ["encoding"],
  },
  tagNames: [
    ...(defaultSchema.tagNames || []),
    "math",
    "mrow",
    "mi",
    "mo",
    "mn",
    "ms",
    "mtext",
    "mspace",
    "mfrac",
    "msqrt",
    "mroot",
    "msub",
    "msup",
    "msubsup",
    "mtable",
    "mtr",
    "mtd",
    "annotation",
    "semantics",
  ],
};

function normalizeLatexSource(raw: string): string {
  if (!raw) return "";

  let text = raw;

  // Normalize common typo \para to \parallel
  text = text.replace(/\\para(?![a-zA-Z])/g, "\\parallel ");

  // If text contains bare \begin{cases} ... \end{cases} not wrapped in $$, wrap it
  text = text.replace(
    /(?<!\$)\\(begin|end)\{cases\}(?!\$)/g,
    (match) => (match.startsWith("\\begin") ? "$$\\begin{cases}" : "\\end{cases}$$")
  );

  return text;
}

export function MathText({
  content,
  className = "",
  inline = false,
}: {
  content: string;
  className?: string;
  inline?: boolean;
}) {
  const normalized = normalizeLatexSource(content);

  if (!normalized.trim()) {
    return null;
  }

  return (
    <span className={`math-text-container ${inline ? "inline-math" : ""} ${className}`}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[
          rehypeKatex,
          [rehypeSanitize, katexSanitizeSchema],
        ]}
        components={
          inline
            ? {
                p: ({ children }) => <span className="math-inline-p">{children}</span>,
              }
            : undefined
        }
      >
        {normalized}
      </ReactMarkdown>
    </span>
  );
}

export default MathText;
