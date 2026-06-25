#!/usr/bin/env bun
// Fail if any installed @tauri-apps/* JS package's major.minor differs from its
// Rust crate counterpart in the workspace Cargo.lock.
//
// This is the same match `tauri build` enforces — but that check only runs
// inside a full build, which only happens on a release tag (release.yml). So a
// JS<->crate minor drift (e.g. a regenerated lockfile floating a plugin crate
// past its JS pin) stays invisible until the release build fails. Running it on
// PRs catches the drift in seconds. See lux's "match every @tauri-apps JS
// package's minor to its Rust crate's minor" gotcha (PRs #32, #42, #69).
//
// Mapping: @tauri-apps/api -> `tauri`; @tauri-apps/plugin-X -> `tauri-plugin-X`.
// Everything else under @tauri-apps (the CLI and its platform binaries) has no
// runtime crate to compare against and is skipped.

import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const desktopDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(desktopDir, "..", "..");
const tauriModules = join(desktopDir, "node_modules", "@tauri-apps");

// Resolved crate versions from Cargo.lock ([[package]] blocks of name + version).
const crateVersion = new Map();
for (const block of readFileSync(join(repoRoot, "Cargo.lock"), "utf8").split("[[package]]")) {
  const name = block.match(/\nname = "([^"]+)"/)?.[1];
  const version = block.match(/\nversion = "([^"]+)"/)?.[1];
  if (name && version) crateVersion.set(name, version);
}

const minor = (v) => v.split(".").slice(0, 2).join(".");
const crateFor = (pkg) =>
  pkg === "api" ? "tauri" : pkg.startsWith("plugin-") ? `tauri-${pkg}` : null;

const rows = [];
for (const pkg of readdirSync(tauriModules)) {
  const crate = crateFor(pkg);
  if (!crate) continue;
  const crateVer = crateVersion.get(crate);
  if (!crateVer) continue; // crate not in the dependency tree — nothing to compare
  const jsVer = JSON.parse(
    readFileSync(join(tauriModules, pkg, "package.json"), "utf8"),
  ).version;
  rows.push({ crate, crateVer, js: `@tauri-apps/${pkg}`, jsVer, ok: minor(jsVer) === minor(crateVer) });
}

rows.sort((a, b) => a.crate.localeCompare(b.crate));
const pad = (s, n) => String(s).padEnd(n);
for (const r of rows) {
  console.log(`${r.ok ? "✓" : "✗"} ${pad(r.crate, 26)} 🦀 ${pad(r.crateVer, 9)} ${pad(r.js, 31)} ⱼₛ ${r.jsVer}`);
}

const bad = rows.filter((r) => !r.ok);
if (bad.length) {
  console.error(
    `\n✗ ${bad.length} Tauri JS<->crate major/minor mismatch(es). Bump each JS pin in ` +
      `apps/desktop/package.json so its minor matches the crate, then \`bun install\`:\n` +
      bad.map((r) => `    ${r.js} (${r.jsVer}) -> ~${minor(r.crateVer)}  to match ${r.crate} ${r.crateVer}`).join("\n"),
  );
  process.exit(1);
}
console.log(`\n✓ all ${rows.length} @tauri-apps packages match their Rust crate minors`);
