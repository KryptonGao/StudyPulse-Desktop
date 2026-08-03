import { execFileSync } from "node:child_process";
import { log } from "node:console";
import { readFileSync, writeFileSync } from "node:fs";
import process from "node:process";

const [templatePath, outputPath] = process.argv.slice(2);

if (!templatePath || !outputPath) {
  throw new Error("Usage: node scripts/render-release-notes.mjs <template> <output>");
}

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

const packageVersion = readJson("package.json").version;
const packageLock = readJson("package-lock.json");
const tauriConfigVersion = readJson("src-tauri/tauri.conf.json").version;

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

const versions = {
  "package.json": packageVersion,
  "package-lock.json": packageLock.version,
  "package-lock.json packages root": packageLock.packages?.[""]?.version,
  "core/Cargo.toml": readCargoPackageVersion("core/Cargo.toml", "workspace.package"),
  "src-tauri/Cargo.toml": readCargoPackageVersion("src-tauri/Cargo.toml", "package"),
  "src-tauri/tauri.conf.json": tauriConfigVersion,
};

const versionValues = new Set(Object.values(versions));
if (versionValues.size !== 1 || versionValues.has(undefined)) {
  throw new Error(`Version mismatch: ${JSON.stringify(versions)}`);
}

const commitMessage = execFileSync("git", ["log", "-1", "--pretty=%B"], {
  encoding: "utf8",
}).trim();

if (!commitMessage) {
  throw new Error("The latest commit has no commit message");
}

const dateParts = new Intl.DateTimeFormat("en-US", {
  timeZone: "Asia/Shanghai",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
}).formatToParts(new Date());
const date = ["year", "month", "day"]
  .map((name) => dateParts.find((part) => part.type === name)?.value)
  .join("-");

const template = readFileSync(templatePath, "utf8");
const updateMarker = "{{UPDATE_CONTENT}}";
const markerCount = template.split(updateMarker).length - 1;

if (markerCount !== 1) {
  throw new Error(`${templatePath} must contain exactly one ${updateMarker} marker`);
}

const rendered = template
  .replaceAll("{{VERSION}}", packageVersion)
  .replaceAll("{{DATE}}", date)
  .replace(updateMarker, commitMessage);

writeFileSync(outputPath, rendered);
log(`Rendered ${outputPath} for v${packageVersion}`);
