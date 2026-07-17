// Restore a human-edited App Store notes file after release-please rebuilds
// the release branch.
//
// release-please regenerates its release branch from main on every push to
// main, force-pushing a single generated commit — every foreign commit on the
// branch (the drafted notes file, and any human edit to it) is discarded each
// time main moves. The draft workflow re-drafts afterwards, but a redraft is
// fresh model output: without this step, a human's edited copy would be
// silently replaced and the release would ship the wrong notes.
//
// The discarded branch tips remain fetchable by SHA, and every prior workflow
// run on the branch recorded its tip as `head_sha` — the run history doubles
// as an archive of the branch's states. This script walks it, newest first,
// and restores the most recent human-authored copy of the notes file into the
// working tree (the workflow commits it, preserving the human author, which
// is also what keeps the draft script's never-overwrite guard holding on
// later runs). Decision rules:
//
//   - current HEAD authored by a human → do nothing. A person shaped the
//     branch state on purpose: an edit is kept by the draft guard, a deletion
//     is the documented "redraft, please" gesture.
//   - walking prior tips: file present, last touched by a human → restore
//     that copy and stop.
//   - file present but bot-authored → stop with nothing to restore; the
//     freshest human intent was to leave a bot draft in place.
//   - tip is a human commit that deleted the file → stop; don't resurrect a
//     deliberate deletion.
//   - anything else (pre-draft states) → keep walking.
//
// A failure here must never block drafting: any error degrades to
// restored=false with a warning, which is exactly the no-restore status quo.
//
// Usage (from store-notes.yml, before the draft step; needs actions: read):
//   GITHUB_TOKEN=... bun scripts/restore-store-notes.ts
// Outputs (GITHUB_OUTPUT): restored=true|false, author="Name <email>".

import { execFileSync } from "child_process";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "fs";

const NOTES_DIR = "apps/desktop/store-notes";
const BOT_AUTHOR = /\[bot\]@users\.noreply\.github\.com$/i;

const repo = process.env.GITHUB_REPOSITORY;
const branch = process.env.GITHUB_HEAD_REF;
const token = process.env.GITHUB_TOKEN;
const api = process.env.GITHUB_API_URL ?? "https://api.github.com";

function output(kv: Record<string, string>) {
  const out = process.env.GITHUB_OUTPUT;
  if (!out) return;
  for (const [k, v] of Object.entries(kv)) {
    appendFileSync(out, `${k}=${v.replace(/\r?\n/g, " ")}\n`);
  }
}

async function gh<T>(path: string): Promise<T | null> {
  const r = await fetch(`${api}${path}`, {
    headers: {
      authorization: `Bearer ${token}`,
      accept: "application/vnd.github+json",
      "x-github-api-version": "2022-11-28",
    },
  });
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`GET ${path} -> ${r.status}`);
  return (await r.json()) as T;
}

async function restore(): Promise<boolean> {
  if (!repo || !branch || !token) {
    throw new Error("missing GITHUB_REPOSITORY/GITHUB_HEAD_REF/GITHUB_TOKEN");
  }

  const version: string = JSON.parse(
    readFileSync(".release-please-manifest.json", "utf8")
  )["."];
  if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`bad version in .release-please-manifest.json: ${version}`);
  }
  const notesPath = `${NOTES_DIR}/${version}.md`;

  // A human-authored HEAD is a deliberate branch state — leave it alone.
  const headAuthor = execFileSync("git", ["log", "-1", "--format=%ae"], {
    encoding: "utf8",
  }).trim();
  if (!BOT_AUTHOR.test(headAuthor)) {
    console.log(`HEAD is human-authored (${headAuthor}); nothing to restore.`);
    return false;
  }

  // Prior branch tips, newest first, from the run history (any workflow).
  const runs = await gh<{
    workflow_runs: { head_sha: string; created_at: string }[];
  }>(
    `/repos/${repo}/actions/runs?event=pull_request&branch=${encodeURIComponent(branch)}&per_page=100`
  );
  const seen = new Set<string>();
  const tips = (runs?.workflow_runs ?? [])
    .sort((a, b) => b.created_at.localeCompare(a.created_at))
    .map((r) => r.head_sha)
    .filter((sha) => !seen.has(sha) && (seen.add(sha), true))
    .slice(0, 30);

  for (const sha of tips) {
    const file = await gh<{ content?: string }>(
      `/repos/${repo}/contents/${notesPath}?ref=${sha}`
    );
    if (file?.content) {
      // Who last touched the file as of this tip?
      const commits = await gh<
        { commit: { author: { name: string; email: string } } }[]
      >(
        `/repos/${repo}/commits?path=${encodeURIComponent(notesPath)}&sha=${sha}&per_page=1`
      );
      const author = commits?.[0]?.commit?.author;
      if (!author) continue;
      if (BOT_AUTHOR.test(author.email)) {
        console.log(
          `newest surviving copy (${sha.slice(0, 7)}) is bot-authored; nothing to restore.`
        );
        return false;
      }
      mkdirSync(NOTES_DIR, { recursive: true });
      writeFileSync(notesPath, Buffer.from(file.content, "base64"));
      output({ author: `${author.name} <${author.email}>` });
      console.log(`restored ${notesPath} from ${sha.slice(0, 7)} (${author.email})`);
      return true;
    }
    // Missing at this tip: a human deletion commit is the documented
    // "redraft, please" gesture — don't walk past it.
    const commit = await gh<{
      commit: { author: { email: string } };
      files?: { filename: string; status: string }[];
    }>(`/repos/${repo}/commits/${sha}`);
    const deleted = commit?.files?.some(
      (f) => f.filename === notesPath && f.status === "removed"
    );
    if (deleted && commit && !BOT_AUTHOR.test(commit.commit.author.email)) {
      console.log(`deliberately deleted at ${sha.slice(0, 7)}; leaving it deleted.`);
      return false;
    }
  }
  console.log("no prior human-authored copy found.");
  return false;
}

try {
  output({ restored: String(await restore()) });
} catch (err) {
  // Degrade to the no-restore status quo rather than failing the job.
  console.log(`::warning::store-notes restore skipped: ${err}`);
  output({ restored: "false" });
}
