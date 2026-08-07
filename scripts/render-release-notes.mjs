import { execFileSync } from "node:child_process";
import { log } from "node:console";
import { readFileSync, writeFileSync } from "node:fs";
import process from "node:process";

// This script renders one release-note artifact from repository state. Its
// inputs are explicit paths plus the current commit; it does not publish or
// mutate release metadata.
const [templatePath, outputPath] = process.argv.slice(2);

// Fail before reading any project files when the caller omitted either path.
// Keeping the CLI contract strict makes CI failures immediately actionable.
if (!templatePath || !outputPath) {
  throw new Error("Usage: node scripts/render-release-notes.mjs <template> <output>");
}

// Release metadata is JSON, so parse errors are allowed to surface rather than
// being converted into a plausible but incomplete release note.
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

// Version parity is checked across the package, lockfile, Core workspace, and
// Tauri config before a note can be rendered.
const packageVersion = readJson("package.json").version;
const packageLock = readJson("package-lock.json");
const tauriConfigVersion = readJson("src-tauri/tauri.conf.json").version;

// Cargo TOML is read with a narrow section parser because this script only
// needs package-version validation and should not rewrite or reformat TOML.
const readCargoPackageVersion = (path, section) => {
  const content = readFileSync(path, "utf8");
  const sectionPattern = new RegExp(
    `\\[${section.replace(".", "\\.")}\\]([\\s\\S]*?)(?=\\n\\[|$)`,
  );
  const sectionContent = content.match(sectionPattern)?.[1];
  const version = sectionContent?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

  if (!version) {
    throw new Error(`Could not find ${section}.version in ${path}`);
  }

  return version;
};

// Include both the lockfile root and package manifests: a stale lockfile must
// fail the same parity check as a stale package or Tauri version.
const versions = {
  "package.json": packageVersion,
  "package-lock.json": packageLock.version,
  "package-lock.json packages root": packageLock.packages?.[""]?.version,
  "core/Cargo.toml": readCargoPackageVersion("core/Cargo.toml", "workspace.package"),
  "src-tauri/Cargo.toml": readCargoPackageVersion("src-tauri/Cargo.toml", "package"),
  "src-tauri/tauri.conf.json": tauriConfigVersion,
};

// A release note is meaningful only when every version holder agrees on one
// concrete value; undefined values are rejected explicitly as well.
const versionValues = new Set(Object.values(versions));
if (versionValues.size !== 1 || versionValues.has(undefined)) {
  throw new Error(`Version mismatch: ${JSON.stringify(versions)}`);
}

// The latest commit message is the release update content. Reading it through
// git preserves multiline messages exactly instead of inventing a summary.
const commitMessage = execFileSync("git", ["log", "-1", "--pretty=%B"], {
  encoding: "utf8",
}).trim();

if (!commitMessage) {
  throw new Error("The latest commit has no commit message");
}

// Dates are rendered in the product's Asia/Shanghai release timezone so the
// generated note is stable for the intended release calendar.
const dateParts = new Intl.DateTimeFormat("en-US", {
  timeZone: "Asia/Shanghai",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
}).formatToParts(new Date());
const date = ["year", "month", "day"]
  .map((name) => dateParts.find((part) => part.type === name)?.value)
  .join("-");

// The template is the only caller-controlled content file. Exactly one marker
// is required so a missing or duplicated insertion point cannot go unnoticed.
const template = readFileSync(templatePath, "utf8");
const updateMarker = "{{UPDATE_CONTENT}}";
const markerCount = template.split(updateMarker).length - 1;

if (markerCount !== 1) {
  throw new Error(`${templatePath} must contain exactly one ${updateMarker} marker`);
}

// Replacement is intentionally limited to the documented placeholders; the
// result is written to the explicit output path and reported to CI/stdout.
const rendered = template
  .replaceAll("{{VERSION}}", packageVersion)
  .replaceAll("{{DATE}}", date)
  .replace(updateMarker, commitMessage);

writeFileSync(outputPath, rendered);
log(`Rendered ${outputPath} for v${packageVersion}`);
