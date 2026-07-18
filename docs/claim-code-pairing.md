# Design: claim-code pairing for lux-node (headless device grant)

Status: DRAFT for review · 2026-07-18

## Problem

`lux-node login <email>` prompts for the account password with `rpassword`
(`apps/node/src/install.rs:316`, reached from `main.rs:76-95`) and runs SRP
against Cognito (`apps/node/src/auth.rs:43-99`). On a headless appliance there
is no TTY (`password prompt: No such device or address`), no keyboard, and no
place to type. Worse, Sign in with Apple users (#199) sign in with an Apple
identity token and a discarded random Cognito password — they *cannot* type a
password into a box, ever. The current flow is a dead end for them, not just an
inconvenience. This is the #1 blocker for the theater-tech appliance: the
target user unboxes a lux-node, plugs in ethernet + DMX, and must be able to
claim it from the lux app without ever shelling into the box.

## Goals

- Pair a factory-fresh, headless lux-node to a lux account using only the lux app.
- Outbound-only from the box (HTTPS/WSS egress; no inbound ports, no LAN server).
- Works identically for password users and Sign in with Apple users.
- No change to the post-pairing runtime: the node keeps running with the
  owner's Cognito identity, same ctl topic space (`lux/ctl/user/<sub>/…`),
  same `lux-sync-auth` authorizer, same sACN path.
- Long-lived sessions — an appliance must not demand re-pairing every 30 days.
- Zero-touch setup binding: the approve step also chooses which setup the node
  drives (today that happens interactively inside `lux-node install`).
- Revocable from the app ("remove device").

## Non-goals (v1)

- Per-device IoT certificates / fleet provisioning (v2, if devices ever need
  identities distinct from their owner).
- Factory-printed QR claim stickers (v2, for manufactured hardware — the
  design leaves an explicit slot).
- Multi-owner / org accounts.

## Current state (verified in source + live infra, 2026-07-18)

- **Node**: CLI `install | login <email> | run [--config]`. `login` = SRP
  (`aws_cognito_srp`, `auth.rs:43-99`); persists
  `{email, refreshToken}` at `$XDG_CONFIG_HOME/lux-node/session.json`, 0600
  (`config.rs:59-119`); the unit sets `XDG_CONFIG_HOME=/var/lib/lux-node` so
  the real path is `/var/lib/lux-node/lux-node/session.json`, owned by the
  `lux-node` system user. `run` mints a fresh ID token per connection attempt
  via `REFRESH_TOKEN_AUTH` (`node.rs:34-75`), captures rotated refresh tokens
  (`node.rs:55-57`), and reconnects hourly (authorizer refresh = 3600 s).
- **Cognito**: pool `us-west-1_jV7esPwmi`, public client
  `2t2l4fn537ttb3vul39olt3of3` (`lux-app`), flows `USER_SRP_AUTH` +
  `REFRESH_TOKEN_AUTH` + `ADMIN_USER_PASSWORD_AUTH`; refresh validity 30 d;
  no hosted UI / OAuth (`infra/accounts.tf:10-60`).
- **#199 (merged, ships dark)**: `services/apple-auth` = lambda
  `lux-apple-auth`, public function URL, routes `/auth/apple[|/link|/revoke]`,
  **and the pool's three CUSTOM_AUTH triggers** (Define/Create/Verify routed by
  `triggerSource`, `services/apple-auth/src/{main,triggers}.rs`;
  `lambda_config` wired in `infra/accounts.tf`). Verify currently accepts one
  answer kind: a linked Apple identity token. SIWA users are **native pool
  users** (`admin_create_user` + `admin_set_user_password`, CONFIRMED,
  `cognito.rs:60-110`), and sign-in mints tokens via
  `admin_initiate_auth(CUSTOM_AUTH)` — the exact pattern the device grant
  needs, already proven in-repo.
- **IoT**: authorizer `lux-sync-auth` → `lux-iot-authorizer`; verifies the
  Cognito **ID** token from `x-lux-token` via `lux_auth::Verifier`; emits
  Connect/Subscribe/Receive/Publish/RetainPublish scoped to
  `lux/ctl/user/<sub>/*`; principal = sub; fails closed.
- **Sync**: `lux-sync-api` (public function URL, bearer ID token verified
  in-lambda) over DynamoDB `lux-sync` (`pk`/`sk` single-table; `USER#<sub>`
  partitions; TTL currently disabled). Node calls `GET /setups` with its ID
  token (`apps/node/src/setups.rs:9-33`).
- **Presence**: retained `PresenceCard {v, session, setupId, name}` on
  `lux/ctl/user/<sub>/presence/<session>`; retained empty Last Will clears it
  (`node.rs:100-105, 233-253`).
- **Constraint discovered**: `lux_auth::Verifier` requires `aud == <single
  app client id>` (`crates/lux-auth/src/lib.rs:88-93`). Env
  `COGNITO_APP_CLIENT_ID` feeds both the IoT authorizer and sync-api. Any
  second app client is invisible to the whole backend until the verifier
  accepts a *set* of client ids.

## The rendezvous problem

RFC 8628 (OAuth device grant) assumes the device can *display* a user code.
Our box has no display and no input. John's original sketch — publish the
short code in the box's presence card — founders on a bootstrap circle: an
unpaired box has no token, the authorizer rejects its CONNECT, and presence is
*retained*, which is precisely the class of publish that #189 taught us is
CONNECT-fatal without explicit policy. Opening a pre-auth pairing topic space
would grow the authorizer's unauthenticated surface for no gain. The presence
card instead becomes the *success signal*: the first retained card on the
owner's ctl space is how the app shows "Paired ✓".

Channels considered for carrying the claim from box to account:

| Channel | Verdict |
|---|---|
| Code in journal / `lux-node pair` output | Works, requires SSH — keep as CLI fallback, not the product flow |
| Pre-auth IoT pairing topics (presence-card sketch) | Bootstrap circle + authorizer surface growth; rejected above |
| mDNS/LAN discovery from the app | Venue WiFi is routinely VLAN'd/client-isolated from wired; avahi-class deps were deliberately removed in headless minimization; unreliable primary |
| Factory QR sticker (Particle-style claim token) | The right *manufactured hardware* answer; useless for DIY Pi installs; v2 slot |
| **Same-public-IP rendezvous via the backend** | **Chosen for v1** — no display, no LAN protocol, pure outbound HTTPS, works for DIY |

**Same-public-IP rendezvous:** the unpaired box registers over HTTPS; the
lambda records the request's source IP (function URLs expose it). When a
signed-in user opens "Add device", the app asks for pending devices *seen from
the same public IP as this request*. Phone on venue WiFi and box on venue
ethernet share the venue's NAT egress → the box appears; phone on cellular →
empty list + "join the venue's WiFi to add a device" hint. This is the
Plex/Spotify-Connect NAT-proximity trick: shared egress establishes proximity,
and the human confirms identity by matching the short code / MAC tail on the
box's label.

## Proposed flow

Three legs — **register** (box), **approve** (app), **redeem** (box) — RFC 8628
semantics on the auth service + Cognito CUSTOM_AUTH.

```
box                         auth service / lux-sync DDB           app (signed in)
 │  POST /auth/device/authorize     │                                  │
 │  {device meta}  (pub_ip↑)        │                                  │
 │ ◄── {device_code, user_code,     │                                  │
 │      interval, expires_in}       │                                  │
 │                                  │   GET /auth/device/pending       │
 │  (poll /auth/device/token        │ ◄── (bearer: user ID token)      │
 │   every `interval` s,            │  → same-egress pending devices:  │
 │   answer: authorization_pending) │    hostname, user_code, mac_tail │
 │                                  │                                  │
 │                                  │   POST /auth/device/approve      │
 │                                  │ ◄── {ref, setupId, universe}     │
 │                                  │   binds device → user sub        │
 │  POST /auth/device/token         │                                  │
 │ ◄── admin_initiate_auth          │                                  │
 │     (CUSTOM_AUTH, device client) │                                  │
 │     → {email, refreshToken,      │                                  │
 │        setupId, universe}        │                                  │
 │  writes session.json + binding,  │                                  │
 │  normal run → presence card      │  app sees presence → "Paired ✓"  │
```

### Box: first-boot state machine

`lux-node run` gains an *unpaired* state instead of dying when session.json is
absent:

1. Generate and persist a `device_id` (uuid) under `/var/lib/lux-node`.
2. `POST /auth/device/authorize` with `{device_id, hostname, mac_tail,
   version, arch}` → hold `device_code` (128-bit secret), log the `user_code`
   to the journal (SSH-fallback parity with the app display).
3. Poll `POST /auth/device/token {device_code}` at `interval` (5 s, honoring
   RFC 8628 `slow_down`) until `approved | expired | denied`. On expiry
   (15 min) re-register with fresh codes, forever, with jittered backoff — the
   service waits patiently at boot until someone claims it.
4. On approval the response carries `{email, refreshToken, setupId,
   universe}`. Write `session.json {email, refreshToken}` exactly as `login`
   does today, persist the setup binding (below), and fall through to the
   normal connect path. Nothing downstream changes.

**Setup binding:** `/etc/lux-node/config.json` is root-owned and the service
runs as `lux-node` (`ProtectSystem=full`), so the paired binding lands in the
state dir instead: `$XDG_CONFIG_HOME/lux-node/node.json` `{setupId,
universe}`. Precedence in `run`: `--config` file if present (today's
installs keep working), else state-dir binding, else unpaired-wait. The
approve screen picks the setup (and universe, default 1) — replacing the
interactive pick list inside `install` (`install.rs:190-247`) for appliances.

New CLI: `lux-node pair` (optional, humans with a shell) — prints
`user_code` + QR, blocks until paired. `login` stays for dev.

### App: approve leg

Settings → Devices → **Add device**:
- Lists `/auth/device/pending` (same-egress, unexpired, unclaimed): hostname,
  `user_code` (e.g. `LUX-7QK2`), MAC tail, version, first-seen.
- User taps the box, confirms code/MAC against the box's label (or journal /
  `lux-node pair` output), picks the setup it should drive, taps **Approve**.
- The screen then watches the user's ctl space for the node's retained
  presence card and flips to "Paired ✓"; it becomes the device list (rename,
  health-from-presence, **Remove device**).

### Token minting: extend #199's CUSTOM_AUTH, add a device app client

No new triggers — extend what #199 shipped:

- **New public app client `lux-node-device`**: `ALLOW_CUSTOM_AUTH` +
  `ALLOW_REFRESH_TOKEN_AUTH` only; `RefreshTokenValidity` = **3650 days**
  (appliance-grade; the interactive `lux-app` client keeps 30 d). A separate
  client id also labels device sessions in every token (`aud`), enabling
  device-scoped authorizer policy later.
- **Verify trigger** (`services/apple-auth/src/triggers.rs`): discriminate the
  challenge answer — today it is an Apple identity token; add a second kind
  keyed on the calling `clientId` (present in the trigger event): for
  `lux-node-device`, the answer is the `device_code`; valid iff the hashed
  code exists in `lux-sync` with status=`approved`, unexpired, and
  `bound_sub == event.userName`'s sub. Mark `redeemed` (single-use)
  atomically with a conditional update.
- **`/auth/device/token`**: on `approved`, calls
  `admin_initiate_auth(CUSTOM_AUTH)` for the bound user on the device client —
  the same call and IAM the service already makes for Apple sign-in
  (`cognito.rs`) — answers with the device_code, returns `{email,
  refreshToken, setupId, universe}`. The box never sees Cognito mechanics.
- **`lux_auth::Verifier` multi-aud** (prerequisite): accept a set of client
  ids (`COGNITO_APP_CLIENT_IDS`, comma-separated, superset env). Consumers:
  `lux-iot-authorizer` and `lux-sync-api` — without this, every token the node
  mints on the device client is rejected at the door.

The node ends up holding a **normal Cognito refresh token for the owner's own
user** — REFRESH_TOKEN_AUTH → ID token → `x-lux-token` → `lux-sync-auth`
policy → ctl topics → sACN, all untouched.

Since SIWA users are native pool users, this flow is identical for them —
the resolved answer to what was this design's biggest open question.

### Where the routes live

On `services/apple-auth`, which is already the de-facto auth service: it owns
the pool triggers, the `admin_initiate_auth` IAM, a public function URL, the
`lux-sync` table client, and the Apple link partitions. Proposal: rename the
deployable to **`lux-auth-api`** (`services/auth`) as part of this work —
`/auth/apple/*` and `/auth/device/*` under one roof, one endpoints entry
(`authUrl`) added to `endpoints.prod.json` when it ships. (If the rename is
too much churn for one PR, land device routes in `services/apple-auth` and
rename later — decision for review.)

`/auth/device/pending` and `/auth/device/approve` authenticate the user's
bearer ID token with the same `lux_auth::Verifier` the sync API uses.

### Data model (`lux-sync` table, new item family)

```
pk = PAIR#<sha256(device_code)>   sk = A
  user_code    "LUX-7QK2"         status  pending|approved|redeemed|denied
  device_id    uuid               hostname, mac_tail, version, arch
  pub_ip       "203.0.113.7"      (function-URL sourceIp)
  bound_sub    <cognito sub>      approved_by + approved_at (audit)
  setup_id     <uuid>  universe   (set on approve)
  created_at / expires_at (15 min)   ttl <epoch>

pk = PAIRIP#<pub_ip>              sk = <created_at>#<device_id>   (pending-list key)
  user_code, hostname, mac_tail, version, pair_ref → PAIR# item

pk = DEVICE#<sub>                 sk = <device_id>                (post-pair registry)
  name, created_at, last_seen, revoked?
```

- `device_code` never stored in cleartext; the box holds the only copy.
- Enable DynamoDB TTL on `ttl` (one infra flag, currently disabled) so PAIR#/
  PAIRIP# litter self-cleans; DEVICE# items carry no `ttl`.
- IAM: the auth service role's `dynamodb:LeadingKeys` pin (today
  `APPLE#*`/`APPLELINK#*`) gains `PAIR#*`, `PAIRIP#*`, `DEVICE#*`.

### Revocation

"Remove device" → mark `DEVICE#<sub>/<device_id>` revoked. Enforcement at the
authorizer: for tokens whose `aud` is the device client, `lux-iot-authorizer`
does one `GetItem` on `DEVICE#<sub>/<device_id>` per CONNECT (device_id rides
as a custom claim minted by a pre-token-generation trigger, or — simpler —
the authorizer denies when the *user* has marked that device revoked, keyed by
sub + client id if we accept one-device-per-account granularity in v1).
CONNECTs happen hourly, not per frame, so cost is negligible; a revoked box
degrades to no-remote (sACN keeps last local state → blackout at the venue's
discretion) within ≤1 h or next reconnect. Tighten in v2 with Cognito
refresh-token rotation on the device client. **Granularity decision for
review** (claim-based per-device vs per-account) — claim-based is the clean
answer and the pre-token trigger is small.

## Security analysis

- `device_code`: 128-bit random, SHA-256 at rest, single-use (conditional
  update), 15-min TTL, constant-time compare in the verify trigger. Token
  polls rate-limited (`slow_down`, RFC 8628 §3.5).
- `user_code` is **display-only confirmation** in v1 — never typed, so the
  classic §6.1 brute-force surface doesn't exist. The enumeration surface is
  `/auth/device/pending`, which requires a valid signed-in user **and** only
  returns devices sharing the caller's NAT egress; rate-limit it anyway.
- Residual risk: someone behind the same venue egress racing the owner to
  approve a just-booted box. Mitigations: approve screen shows hostname + MAC
  tail (physically on the box); a wrong claim is evident (owner never sees a
  presence card), auditable (`approved_by`), and revocable; the pairing window
  only exists while the box has no session. Fully closed in v2 by
  possession-proof QR claim stickers on manufactured units. This matches the
  accepted posture of mainstream NAT-proximity pairing (Chromecast-class) for
  a stage-lighting appliance.
- Pairing rides plain HTTPS to an existing public function URL; nothing
  touches IoT pre-auth — the authorizer's unauthenticated surface is unchanged.
- The trigger extension cannot cross accounts for Apple users: the existing
  verify path stays Apple-token-bound; the device path is
  client-id-discriminated and checks `bound_sub` — same fail-closed shape as
  #199's trigger (`triggers.rs:79-116`).
- Box at rest stores only `{email, refreshToken}` (0600, dedicated system
  user) — the exposure class is identical to today, now on a segregated
  client whose sessions can be revoked without touching phones.

## Phasing

1. **Foundations** — `lux_auth::Verifier` multi-aud + env plumb-through
   (authorizer, sync-api); `lux-node-device` app client; `lux-sync` TTL flag;
   LeadingKeys IAM. Each independently shippable and inert.
2. **Backend** — device routes on the auth service + verify-trigger
   extension. Fully testable with `curl` alone (register → approve with a
   phone-minted bearer → redeem) before any client work.
3. **Node** — unpaired state machine in `run`, state-dir setup binding,
   `pair` subcommand, journal parity. Testable end-to-end on the mini: delete
   session.json, watch it pair. **This retires rpassword from the appliance
   path.**
4. **App** — Add-device screen (pending → approve+setup-pick →
   presence-confirm), device list with remove.
5. **v2 slots** — QR claim stickers / provisioned claim tokens for
   manufactured units; refresh-token rotation; per-device authorizer policy.

## Open questions for review

1. Service naming: extend `services/apple-auth` in place vs rename to
   `services/auth` / `lux-auth-api` in the same PR (recommend rename — the
   scope is now plainly "auth", and it ships dark anyway).
2. Revocation granularity v1: per-device claim via pre-token-generation
   trigger, or per-account flag (cheaper, coarser)?
3. Ten-year refresh on the device client vs enabling Cognito refresh-token
   rotation from day one (rotation = write-back churn on session.json, which
   `node.rs:55-57` already handles).
4. `user_code` format: Crockford base32, no vowels, `LUX-` + 4 chars —
   sufficient for display-only; bump to 6 if it ever becomes type-in
   (e.g. a manual-entry fallback for cellular/VLAN-isolated venues — v1?).
5. Does the approve leg also want to *name* the device (writes `DEVICE#`
   name, later editable), or default to hostname?
