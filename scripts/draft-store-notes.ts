// Draft the App Store "What's New" for the release just cut, from its
// CHANGELOG section. Runs at ship time (release.yml's store-notes job, checked
// out at the release tag); the job attaches the drafted text to the GitHub
// Release body between <!-- store-notes --> markers, and appstore.yml
// extracts that section at submit time — edit it on the release page any
// time before then. This script only writes the local file; write-once
// against the release body is the workflow's job.
//
// The voice-anchor example comes from PREV_NOTES_FILE when the workflow
// provides one (extracted from the previous release's section), else from
// the newest notes file committed back when drafts landed on main.
//
// Usage: [PREV_NOTES_FILE=...] ANTHROPIC_API_KEY=... bun scripts/draft-store-notes.ts

import Anthropic from "@anthropic-ai/sdk";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "fs";
import { join } from "path";

const NOTES_DIR = "apps/desktop/store-notes";

const version: string = JSON.parse(
  readFileSync(".release-please-manifest.json", "utf8")
)["."];
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  throw new Error(`bad version in .release-please-manifest.json: ${version}`);
}

const notesPath = join(NOTES_DIR, `${version}.md`);

// Never redraft an existing file (committed notes from the drafts-on-main
// era, or a prior local draft): regenerating through the model produces
// *different* text, and existing text may carry human edits.
if (existsSync(notesPath)) {
  console.log(`${notesPath} already exists; leaving it alone.`);
  process.exit(0);
}

// The version's CHANGELOG section: from its `## [x.y.z]` heading to the next
// `## [` heading (or EOF).
const changelog = readFileSync("CHANGELOG.md", "utf8");
// Canonical regex escape (the version is already shape-validated above, but
// CodeQL rightly wants the escape complete rather than dot-only).
const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const section = changelog.match(
  new RegExp(`^## \\[${escaped}\\][\\s\\S]*?(?=^## \\[|$(?![\\s\\S]))`, "m")
)?.[0];
if (!section) {
  console.log(`CHANGELOG.md has no section for ${version} yet; nothing to draft.`);
  process.exit(0);
}

// The most recent shipped notes anchor the voice: the workflow-provided file
// (previous release's section) when present, else the newest committed file.
const prevNotesFile = process.env.PREV_NOTES_FILE;
const fromRelease =
  prevNotesFile && existsSync(prevNotesFile)
    ? readFileSync(prevNotesFile, "utf8").trim()
    : "";
const previous = existsSync(NOTES_DIR)
  ? readdirSync(NOTES_DIR)
      .filter((f) => f.endsWith(".md") && f !== `${version}.md`)
      .sort((a, b) =>
        b.localeCompare(a, undefined, { numeric: true, sensitivity: "base" })
      )[0]
  : undefined;
const example =
  fromRelease ||
  (previous ? readFileSync(join(NOTES_DIR, previous), "utf8").trim() : undefined);

const client = new Anthropic();
const response = await client.messages.create({
  model: "claude-opus-4-8",
  max_tokens: 1024,
  thinking: { type: "adaptive" },
  system: [
    "You write the App Store \"What's New\" text for lux, a DMX lighting controller used by lighting technicians and hobbyists.",
    "Rules:",
    "- Plain, factual sentences. No hype, no exclamation marks, no marketing adjectives, no emoji.",
    "- Describe only what a user sees or does differently. Skip internal changes: refactors, CI, dependency bumps, backend plumbing, developer tooling.",
    "- Short paragraphs separated by blank lines, most important change first. No headings, no bullet markers, no links, no PR numbers, no version numbers.",
    "- If nothing in the changelog is user-visible, output exactly: Maintenance and stability improvements.",
    "- Output the notes text only — nothing else.",
    ...(example ? ["", "The previous release's notes, as a voice reference:", "", example] : []),
  ].join("\n"),
  messages: [
    {
      role: "user",
      content: `Changelog for the release:\n\n${section}`,
    },
  ],
});

if (response.stop_reason !== "end_turn") {
  throw new Error(`unexpected stop_reason: ${response.stop_reason}`);
}
const text = response.content
  .filter((b): b is Anthropic.TextBlock => b.type === "text")
  .map((b) => b.text)
  .join("")
  .trim();
if (!text) throw new Error("empty draft");

mkdirSync(NOTES_DIR, { recursive: true });
writeFileSync(notesPath, text + "\n");
console.log(`drafted ${notesPath} (${text.length} chars)`);
