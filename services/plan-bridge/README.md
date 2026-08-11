# lux-plan-bridge

The Planning Center bridge. A church's administrator authorizes lux against their Planning Center organization once, in a browser; this service keeps custody of the resulting refresh token and is the only thing that ever holds the OAuth client secret. Every plan the app reads comes through here.

## Why it is a service and not app code

Planning Center's own guidance is to register **one** OAuth application and reuse its client id and secret for every church that connects. One secret, therefore, for the whole integration — and a secret shipped inside an installed app is not a secret. Custody here also means the connection survives a reinstall, a new laptop, and the volunteer who set it up leaving.

## Routes

Served on a Function URL, fronted by the `auth.lux.johncarmack.com` CloudFront distribution that already carries the web Sign in with Apple routes (`infra/apple-auth-web.tf`). The domain is not cosmetic: Planning Center matches a registered redirect URI byte for byte, and a raw `*.lambda-url.on.aws` host is not one.

| Route | Auth | What |
|---|---|---|
| `POST /pco/connect` | Cognito bearer | Mint the consent URL for this account |
| `GET /pco/callback` | single-use `state` | Planning Center's redirect; exchanges the code, stores the tokens, renders a page |
| `GET /pco/status` | Cognito bearer | Is this account connected, and to which church |
| `GET /pco/service-types` | Cognito bearer | What a setup could follow (retired ones filtered out) |
| `POST /pco/plan` | Cognito bearer | The next plan, resolved against the cue map in the body |
| `POST /pco/disconnect` | Cognito bearer | Revoke the authorization at Planning Center, then delete the tokens |

`/pco/plan` is a `POST` for what is plainly a read because the request carries the `CueMap`: the map lives on the setup and travels by sync, so the bridge holds no copy, and sending it up means one engine (`lux-cue`, server-side) decides what a plan means. A laptop and a phone looking at the same plan cannot then disagree.

## Guarantees

**Read-only against Planning Center, structurally.** `lux-pco`'s client builds `GET` requests and nothing else; the only `POST`s in that crate are the OAuth token and revocation endpoints, which talk to the OAuth server rather than the API — one asks for a credential, the other gives it back. The `services` scope has no read-only variant, so this cannot be enforced by the grant — it is enforced by there being no method that writes. lux never advances someone else's service.

**Identity is the verified bearer's `sub`, never a request field.** A church can only read the connection it authorized.

**Planning Center's failures are translated, not forwarded.** Their 401 becomes "reconnect", their 429 becomes "busy, try again", and their response body never reaches a lux surface — it is their wording about their system, and an operator reading it in a lux dialog would not know which of two products was complaining.

**Disconnecting hands the credential back, and account deletion is a disconnect.** `/pco/disconnect` revokes the refresh token at Planning Center *before* deleting the stored row — deleting alone would only stop lux from being able to spend the token, while the grant stayed live in the church's Planning Center settings for up to ninety days. The app calls this same route while deleting an account, so a deleted account never leaves a live credential for another company's data behind it. The revocation is best-effort by type (`tokens::Revoked` has no error arm): an account that never connected, an unreadable OAuth secret, and an unreachable Planning Center are each logged, and the row is deleted regardless — because a deletion that refuses to finish is worse than a cleanup that has to be retried, and revoking a token twice is a no-op at their end.

**Nothing here is in the DMX path.** Losing Planning Center costs the plan list, never the lights.

## Storage

Two item kinds in the `lux-sync` table, in their own partitions, pinned by the role's `dynamodb:LeadingKeys` condition so this service cannot reach a user's setups:

- `pk = PCO#<sub>, sk = CONN` — the connection: org, access token and expiry, refresh token.
- `pk = PCOSTATE#<state>, sk = STATE` — an in-flight connect attempt. Read-and-delete, and self-expiring via the table's `ttl`.

## Credentials

`/lux/bridge/prod/pco-oauth` in Secrets Manager, holding `{"client_id": …, "client_secret": …}`. Hand-created, never a Terraform resource — the house pattern for true secrets, and here a hard requirement, since Planning Center shows the client secret exactly once at registration. Loaded lazily and cached, so the stack applies and the function serves before the secret exists; only the connect routes need it.

## Verifying it

The unit tests cover routing, the token-refresh decision, the three revocation outcomes (connected, never connected, upstream refused), HTML escaping, and the failure translation, all without a network. Against the live Planning Center OAuth server:

```sh
eval "$(aws secretsmanager get-secret-value --profile newearth-admin \
  --secret-id /lux/bridge/prod/pco-oauth --query SecretString --output text \
  | jq -r '"export LUX_PCO_CLIENT_ID=\(.client_id) LUX_PCO_CLIENT_SECRET=\(.client_secret)"')"
cargo test -p lux-pco --test live -- --ignored --nocapture
```

Those are `#[ignore]`d, so they never run in the PR gate.

**What live tests cannot prove:** Planning Center redirects `/oauth/authorize` to its login page *before* validating `client_id` or `redirect_uri`, so no unauthenticated request can confirm that the callback URIs are registered. That is only checked after a human signs in — see the interactive step in `.claude/specs/plan-bridge-oauth-runbook.md`.
