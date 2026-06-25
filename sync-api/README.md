# lux-sync-api

Cloud sync for the lux desktop app, on AWS Lambda (`provided.al2023`), behind a Function URL. The app pushes and pulls its setups here so they follow a signed-in user across devices. Config (the live setups) syncs; live DMX levels never do — output stays local-authoritative.

## How it works

The app calls the Function URL with `Authorization: Bearer <Cognito ID token>`. The handler:

1. Verifies the JWT against the Cognito user pool's JWKS (`auth.rs`) — RS256 signature, issuer, audience (= app client id), expiry, and `token_use == "id"`.
2. Derives the caller's `sub` from the verified token. **This is the only identity trusted** — the request never names a user, so cross-tenant access is impossible by construction.
3. Reads/writes only that user's partition of the `lux-sync` DynamoDB table (`store.rs`).

No AWS credentials ever reach the client; it holds a short-lived JWT. Same single-account, nothing-always-on, no-long-lived-keys shape as the IoT remote-control path.

## Routes

| Method | Path | Body / query | Effect |
|---|---|---|---|
| `GET` | `/setups` | — | List the caller's setups (including tombstones; the client filters). |
| `PUT` | `/setups/{id}` | `{ name, universe, fixtures, baseUpdatedAt? }` | Create/update one setup. Optimistic concurrency: with `baseUpdatedAt` the write lands only if the stored `updatedAt` still matches; without it, only if the setup doesn't yet exist. `409` on conflict. |
| `DELETE` | `/setups/{id}?baseUpdatedAt=N` | — | Soft-delete (write a `deleted` tombstone) so the delete propagates to other devices. `409` on conflict. |

`updatedAt` (epoch millis) is assigned **server-side** on every write — never the client clock — and is the last-writer-wins authority. Fixtures are stored as an opaque JSON string, so this service stays agnostic to the app's fixture schema.

## Data model (DynamoDB `lux-sync`, single table)

```
PK = USER#<sub>   SK = SETUP#<setupId>   { name, universe, fixtures, rev, updatedAt, deleted }
```

One item per setup. A `Query` on the PK returns the user's whole account in one call.

## Build & deploy

Built and deployed with [cargo-lambda]; the runtime expects a `bootstrap` executable. Infra (Cognito pool + client, the DynamoDB table, this function's scoped IAM role, the Function URL, and its env vars) is Terraform in `../infra/accounts.tf`.

```sh
cargo lambda build --release --x86-64
cargo lambda deploy lux-sync-api
```

The IAM role and env vars are owned by Terraform (`terraform -chdir=../infra apply`); deploy ships only the code (config is `ignore_changes` on the Terraform side, mirroring the bot).

[cargo-lambda]: https://www.cargo-lambda.info
