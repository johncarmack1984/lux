// Submit a shipped release to both App Stores (iOS + macOS): wait for the
// uploaded builds to finish Apple-side processing, create (or reuse) the
// store versions, attach the builds, set the human-approved "What's New"
// from apps/desktop/store-notes/<version>.md, and file review submissions
// with release-on-approval. Everything is idempotent enough to re-dispatch:
// existing editable versions are reused, and a platform with a review
// submission already open is skipped.
//
// TestFlight distribution is not gated on this — release.yml uploads builds
// on every release; this workflow is the deliberate "put it on the store"
// step (workflow: appstore.yml).
//
// Usage: ASC_KEY_PATH=key.p8 ASC_KEY_ID=... ASC_ISSUER_ID=... \
//        bun scripts/appstore-submit.ts --version 1.2.3

import { SignJWT, importPKCS8 } from "jose";
import { existsSync, readFileSync } from "fs";
import { join } from "path";

const API = "https://api.appstoreconnect.apple.com";
const PLATFORMS = ["IOS", "MAC_OS"] as const;
type Platform = (typeof PLATFORMS)[number];
// appStoreState values in which a version record can still be edited and
// (re)submitted; anything else for the same versionString is a hard stop.
const EDITABLE_STATES = new Set([
  "PREPARE_FOR_SUBMISSION",
  "DEVELOPER_REJECTED",
  "REJECTED",
  "METADATA_REJECTED",
  "INVALID_BINARY",
]);
const OPEN_SUBMISSION_STATES = new Set([
  "READY_FOR_REVIEW",
  "WAITING_FOR_REVIEW",
  "IN_REVIEW",
  "UNRESOLVED_ISSUES",
]);
const BUILD_POLL_INTERVAL_MS = 60_000;
const BUILD_POLL_TIMEOUT_MS = 35 * 60_000;

const version = process.argv[process.argv.indexOf("--version") + 1];
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  throw new Error(`--version must be X.Y.Z, got: ${version}`);
}

const notesPath = join("apps/desktop/store-notes", `${version}.md`);
if (!existsSync(notesPath)) {
  throw new Error(
    `${notesPath} does not exist — write (or approve the drafted) store notes before submitting`
  );
}
const whatsNew = readFileSync(notesPath, "utf8").trim();

const bundleId: string = JSON.parse(
  readFileSync("apps/desktop/src-tauri/tauri.conf.json", "utf8")
).identifier;

// The secret stores the .p8 as raw PEM; tolerate base64 for robustness.
let p8 = readFileSync(process.env.ASC_KEY_PATH!, "utf8");
if (!p8.includes("BEGIN PRIVATE KEY")) {
  p8 = Buffer.from(p8, "base64").toString("utf8");
}
const keyId = process.env.ASC_KEY_ID!;
const issuerId = process.env.ASC_ISSUER_ID!;
const signingKey = await importPKCS8(p8, "ES256");

async function asc<T = any>(
  path: string,
  init?: { method?: string; body?: unknown }
): Promise<{ status: number; json: T }> {
  const token = await new SignJWT({ aud: "appstoreconnect-v1" })
    .setProtectedHeader({ alg: "ES256", kid: keyId, typ: "JWT" })
    .setIssuer(issuerId)
    .setIssuedAt()
    .setExpirationTime("15m")
    .sign(signingKey);
  const res = await fetch(`${API}${path}`, {
    method: init?.method ?? "GET",
    headers: {
      Authorization: `Bearer ${token}`,
      ...(init?.body ? { "Content-Type": "application/json" } : {}),
    },
    body: init?.body ? JSON.stringify(init.body) : undefined,
  });
  const text = await res.text();
  return { status: res.status, json: text ? JSON.parse(text) : ({} as T) };
}

function expect<T>(r: { status: number; json: any }, what: string): T {
  if (r.status >= 400) {
    const detail = r.json?.errors?.map((e: any) => e.detail).join("; ");
    throw new Error(`${what} failed (${r.status}): ${detail ?? JSON.stringify(r.json)}`);
  }
  return r.json as T;
}

// --- app record ---------------------------------------------------------

const apps = expect<any>(
  await asc(`/v1/apps?filter[bundleId]=${bundleId}`),
  "app lookup"
);
const appId: string = apps.data?.[0]?.id;
if (!appId) throw new Error(`no App Store app record for ${bundleId}`);
console.log(`app ${appId} (${bundleId}), submitting ${version}`);

// --- wait for both builds to process -------------------------------------

// Builds carry no platform attribute — join through preReleaseVersion, which
// must stay listed in `fields[builds]`: a sparse fieldset of only attributes
// omits the relationship, so the join would silently resolve nothing and the
// poll would spin until it timed out.
async function processedBuilds(): Promise<Map<Platform, string>> {
  const r = expect<any>(
    await asc(
      `/v1/builds?filter[app]=${appId}&filter[version]=${version}` +
        `&fields[builds]=version,processingState,preReleaseVersion&include=preReleaseVersion` +
        `&fields[preReleaseVersions]=platform`
    ),
    "build list"
  );
  const trains = new Map<string, Platform>(
    (r.included ?? []).map((pv: any) => [pv.id, pv.attributes.platform])
  );
  const byPlatform = new Map<Platform, string>();
  for (const b of r.data ?? []) {
    const platform = trains.get(b.relationships?.preReleaseVersion?.data?.id);
    if (platform && b.attributes.processingState === "VALID") {
      byPlatform.set(platform, b.id);
    }
  }
  return byPlatform;
}

const deadline = Date.now() + BUILD_POLL_TIMEOUT_MS;
let builds = await processedBuilds();
while (PLATFORMS.some((p) => !builds.has(p))) {
  if (Date.now() > deadline) {
    const missing = PLATFORMS.filter((p) => !builds.has(p)).join(", ");
    throw new Error(`timed out waiting for ${missing} ${version} build(s) to process`);
  }
  console.log(
    `waiting for builds to process (have: ${[...builds.keys()].join(", ") || "none"})`
  );
  await new Promise((r) => setTimeout(r, BUILD_POLL_INTERVAL_MS));
  builds = await processedBuilds();
}
console.log(`builds processed: ${[...builds.entries()].map(([p, id]) => `${p}=${id}`).join(" ")}`);

// --- per-platform: version, build, notes, submission ---------------------

for (const platform of PLATFORMS) {
  const buildId = builds.get(platform)!;

  const versions = expect<any>(
    await asc(
      `/v1/apps/${appId}/appStoreVersions?limit=50` +
        `&fields[appStoreVersions]=versionString,platform,appStoreState`
    ),
    "version list"
  );
  const existing = (versions.data ?? []).find(
    (v: any) =>
      v.attributes.platform === platform && v.attributes.versionString === version
  );

  let versionId: string;
  if (existing && EDITABLE_STATES.has(existing.attributes.appStoreState)) {
    versionId = existing.id;
    console.log(`${platform}: reusing version ${versionId} (${existing.attributes.appStoreState})`);
  } else if (existing) {
    throw new Error(
      `${platform} ${version} already exists in state ${existing.attributes.appStoreState}`
    );
  } else {
    const created = expect<any>(
      await asc("/v1/appStoreVersions", {
        method: "POST",
        body: {
          data: {
            type: "appStoreVersions",
            attributes: { platform, versionString: version, releaseType: "AFTER_APPROVAL" },
            relationships: { app: { data: { type: "apps", id: appId } } },
          },
        },
      }),
      `${platform} version create`
    );
    versionId = created.data.id;
    console.log(`${platform}: created version ${versionId}`);
  }

  expect(
    await asc(`/v1/appStoreVersions/${versionId}/relationships/build`, {
      method: "PATCH",
      body: { data: { type: "builds", id: buildId } },
    }),
    `${platform} build attach`
  );

  const locs = expect<any>(
    await asc(
      `/v1/appStoreVersions/${versionId}/appStoreVersionLocalizations` +
        `?fields[appStoreVersionLocalizations]=locale`
    ),
    `${platform} localization list`
  );
  const enUS = (locs.data ?? []).find((l: any) => l.attributes.locale === "en-US");
  if (!enUS) throw new Error(`${platform}: no en-US localization on ${versionId}`);
  expect(
    await asc(`/v1/appStoreVersionLocalizations/${enUS.id}`, {
      method: "PATCH",
      body: {
        data: {
          type: "appStoreVersionLocalizations",
          id: enUS.id,
          attributes: { whatsNew },
        },
      },
    }),
    `${platform} whatsNew`
  );

  const submissions = expect<any>(
    await asc(
      `/v1/reviewSubmissions?filter[app]=${appId}&filter[platform]=${platform}` +
        `&filter[state]=${[...OPEN_SUBMISSION_STATES].join(",")}`
    ),
    `${platform} submission list`
  );
  if ((submissions.data ?? []).length > 0) {
    console.log(`${platform}: a review submission is already open — skipping submit`);
    continue;
  }

  const rs = expect<any>(
    await asc("/v1/reviewSubmissions", {
      method: "POST",
      body: {
        data: {
          type: "reviewSubmissions",
          attributes: { platform },
          relationships: { app: { data: { type: "apps", id: appId } } },
        },
      },
    }),
    `${platform} reviewSubmission create`
  );
  expect(
    await asc("/v1/reviewSubmissionItems", {
      method: "POST",
      body: {
        data: {
          type: "reviewSubmissionItems",
          relationships: {
            reviewSubmission: { data: { type: "reviewSubmissions", id: rs.data.id } },
            appStoreVersion: { data: { type: "appStoreVersions", id: versionId } },
          },
        },
      },
    }),
    `${platform} reviewSubmissionItem`
  );
  const submitted = expect<any>(
    await asc(`/v1/reviewSubmissions/${rs.data.id}`, {
      method: "PATCH",
      body: {
        data: { type: "reviewSubmissions", id: rs.data.id, attributes: { submitted: true } },
      },
    }),
    `${platform} submit`
  );
  console.log(`${platform}: submitted — state ${submitted.data.attributes.state}`);
}

console.log(`done: ${version} submitted with release-on-approval`);
