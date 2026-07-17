// Draft the App Store "What's New" for the release the release-please PR is
// about to cut, from that release's CHANGELOG section. Runs on the release PR
// (workflow: store-notes.yml); the human edits the committed file in the PR,
// and appstore.yml refuses to submit a version whose file is missing.
//
// The draft is exactly that — a draft. It never overwrites a human edit: the
// file is only (re)written while its last committed author is the workflow
// bot (or it doesn't exist yet). Delete the file in the PR to force a fresh
// draft. The authorship check reads git history, so the workflow checks out
// with full depth; scripts/restore-store-notes.ts runs first and resurrects a
// human edit that a release-please branch rebuild discarded (the workflow
// skips drafting entirely when it restores one).
//
// Usage: ANTHROPIC_API_KEY=... bun scripts/draft-store-notes.ts

import Anthropic from "@anthropic-ai/sdk";
import { execFileSync } from "child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "fs";
import { join } from "path";

const NOTES_DIR = "apps/desktop/store-notes";
const BOT_EMAIL = "github-actions[bot]@users.noreply.github.com";

const version: string = JSON.parse(
  readFileSync(".release-please-manifest.json", "utf8")
)["."];
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  throw new Error(`bad version in .release-please-manifest.json: ${version}`);
}

const notesPath = join(NOTES_DIR, `${version}.md`);

// Never clobber a human's edit.
if (existsSync(notesPath)) {
  const lastAuthor = execFileSync(
    "git",
    ["log", "-1", "--format=%ae", "--", notesPath],
    { encoding: "utf8" }
  ).trim();
  if (lastAuthor && lastAuthor !== BOT_EMAIL) {
    console.log(`${notesPath} was last edited by ${lastAuthor}; leaving it alone.`);
    process.exit(0);
  }
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

// The most recent shipped notes file anchors the voice.
const previous = existsSync(NOTES_DIR)
  ? readdirSync(NOTES_DIR)
      .filter((f) => f.endsWith(".md") && f !== `${version}.md`)
      .sort((a, b) =>
        b.localeCompare(a, undefined, { numeric: true, sensitivity: "base" })
      )[0]
  : undefined;
const example = previous
  ? readFileSync(join(NOTES_DIR, previous), "utf8").trim()
  : undefined;

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
