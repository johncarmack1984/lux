# Architecture

Lux is a Tauri 2 app (Rust core + Vite/React UI) with serverless AWS satellites. Local control is authoritative; the cloud only moves config and nudges — live DMX output never depends on the network.

## Runtime shape

- **Desktop / iOS app** (`apps/desktop`): the Rust core owns the DMX universe and renders it to hardware (Enttec OpenDMX USB, sACN/E1.31, Art-Net) behind the `devices/` sink abstraction. The UI drives the core over [tauri-typed-ipc](https://github.com/johncarmack1984/tauri-typed-ipc).
- **sync-api** (`services/sync-api`): Lambda Function URL. Verifies the Cognito JWT in-handler; the caller's verified `sub` — never a request field — keys their DynamoDB partition, so cross-tenant access is impossible.
- **iot-authorizer** (`services/iot-authorizer`): IoT custom authorizer for the change-nudge channel; same JWT verification, returns a policy scoped to the verified user's own topic.
- **bot** (`services/bot`): Discord-interactions Lambda (ed25519-verified) publishing remote-control commands to AWS IoT; the app dials out over MQTT + mutual TLS and subscribes — no public ingress to the light host, nothing always-on.
- **infra** (`infra/`): Terraform, run by CI via OIDC — read-only plan on PRs, curated least-privilege apply on release merges.

## Type spines

Every wire is declared once and drift-guarded — a mismatch is a compile/CI failure, never a runtime surprise:

- UI ↔ Rust: ttipc-generated `src/bindings.ts`, committed and drift-gated by `src-tauri/tests/bindings.rs` (plain `cargo test` fails when stale).
- Desktop ↔ sync-api: `crates/lux-wire`, depended on by both sides; golden tests pin the exact JSON. No hand-maintained mirrors anywhere.
- Cognito verification shared by the identity-gated Lambdas: `crates/lux-auth`.

## Configuration (environment as data)

The code is environment-agnostic: every environment value — Cognito pool/client/region, the sync URL, the IoT nudge endpoint — lives in `apps/desktop/src-tauri/endpoints.prod.json`, machine-generated from Terraform outputs by `scripts/gen-endpoints`, committed, and embedded at compile time. It is drift-gated twice: infra PR plans regenerate and diff it, and the release apply re-checks it before any Lambda or artifact ships — so a stale value fails a check instead of shipping. A gitignored `endpoints.local.json` overrides any subset for dev stacks and carries machine-local config (remote-control device identity + mTLS material paths, the sACN interface override). There are no env files; the only hardcoded names in source are protocol identifiers owned by `lux-wire` (topic scheme, token header, authorizer name).

## Sync model (server-authoritative, nudged pull)

Setups — and the user's settings blob, which syncs whole as a single record — are edited locally (offline-first) and pushed with optimistic concurrency; the server assigns `updatedAt` (last-writer-wins) and setup deletes are soft tombstones. Pulls reconcile on sign-in, startup, window focus, and reconnect, with exponential-backoff retry while offline.

After each committed write the sync-api publishes a tiny opaque frame to the writer's own topic (`lux/sync/user/<sub>`). Each signed-in device keeps one open MQTT-over-WebSocket connection to IoT Core (`nudge.rs`), authorized per-user by the `lux-sync-auth` custom authorizer, and treats **any** frame as "pull now" — the frame is never parsed; the HTTP pull stays the authoritative sync. IoT Core is the connection holder, so the estate stays serverless while getting standing-connection push.
