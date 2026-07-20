# Shared control — inviting a contact to run your lights

Status: backend landed (phase 1) · 2026-07-19

## Problem

Two people are in a room with one lighting rig. One of them owns it: their app holds the setup, their machine talks to the fixtures. The other should be able to pick up a phone and move a fader. Today the only ways to do that are to hand over an account password or to stand behind them — the first is a real security answer to nothing, and the second is not a feature.

The ask is narrow and worth stating precisely, because it rules out most of the obvious designs: **one contact, one setup, full desk, revocable, and the owner's machine stays the only thing that ever writes DMX.**

## Shape

A **grant** is a triple — `(owner, contact, setup)`. It is not a shared account, not an org, not a role on the owner's account. The contact signs in as themselves with their own credentials; the grant only widens what that existing identity is allowed to do inside the owner's control-topic space.

Three consequences fall out of that and drive everything below:

- **Nothing about the owner's account is exposed.** A grant names one setup. The contact cannot see the owner's other setups, cannot sync, cannot edit the patch, and cannot read anything outside the two topics the grant lists.
- **The owner's applier stays the sole DMX authority.** A guest is one more attributed publisher into the owner's control space; frames carry `src`, and the applier merges them per-slot last-takes-precedence exactly as it already does for the owner's own devices. Nothing in this feature can drive a fixture directly.
- **Guests never sync.** The guest's surface is drawn from the retained `config` topic, not from a copy of the setup. There is no local persistence of someone else's setup on a guest device, so there is nothing to leak, stale, or clean up when a grant ends.

## Invites are claim codes, not emails

The owner mints a short-lived single-use code and sends it over a channel they already trust — iMessage, Signal, saying it out loud. The app hands it to the OS share sheet where the platform makes that easy, and always offers a clipboard copy.

This avoids building any mail infrastructure at all: no SES, no deliverability surface, no bounce handling, no address-verification flow, and no "did it land in spam". The sender is a human the recipient already knows, which is a stronger provenance signal than a From header. It also makes Hide My Email contacts work by construction, because nothing is ever matched on an address.

A code is a bearer credential and is treated like one: 10 characters from a confusion-free alphabet (~2^46), stored only as its SHA-256, single-use, and expiring in 48 hours. Claiming requires a signed-in account, so guessing is not anonymous — an attacker needs a real Cognito user, and every attempt is attributable.

The format is `LUX-XXXXX-XXXXX`. The alphabet drops vowels (so no code spells anything) and every lookalike pair (`0/O`, `1/I/L`, `S/5`). Since `L` and `U` are not in the alphabet, the `LUX` prefix can be stripped unambiguously on the way back in, and claiming accepts the code however a messaging app or a human mangled it: any case, with or without the dashes, with stray spaces.

## Data

Four item families in the `lux-sync` table, all in their own partitions so that sync's `USER#<sub>` query never sees them and the account wipe never races them — the pattern the Apple links and pairing records already use.

```
pk = INVITE#<sha256(code)>   sk = INVITE
  ownerSub, ownerLabel, setupId, setupName, label?
  createdAt, expiresAt, ttl

pk = GRANT#<owner_sub>       sk = INVITE#<codeRef>            (owner's pending list)
  codeRef, setupId, setupName, label?, createdAt, expiresAt, ttl

pk = GRANT#<owner_sub>       sk = CONTACT#<contact_sub>#SETUP#<setup_id>
  contactSub, contactLabel, setupId, setupName, label?, createdAt

pk = SHARED#<contact_sub>    sk = OWNER#<owner_sub>#SETUP#<setup_id>
  ownerSub, ownerLabel, setupId, setupName, createdAt
```

The last two are one grant written twice. That redundancy is deliberate and load-bearing: the owner needs a list keyed by *them* to manage, and the authorizer needs a list keyed by the *connecting user* to answer a connect in one lookup with no scan and no read of anyone else's partition. Both halves are written and deleted in a single transaction with `attribute_not_exists` guards, because either half alone is a bug with teeth — an orphaned `SHARED#` row is access the owner cannot see or revoke, and an orphaned `GRANT#` row is a revoke button that does nothing.

Claiming consumes the invite inside that same transaction. The conditional delete is what makes a code single-use: exactly one concurrent claimer can win it.

`INVITE#` rows carry `ttl` and self-expire; grants do not.

## Routes (`services/sync-api`)

| Route | Who | Does |
|---|---|---|
| `POST /shares/invite` | owner | mint a code for one of their own setups |
| `POST /shares/claim` | contact | redeem a code, gaining the grant |
| `GET /shares` | either | grants given, grants received, invites outstanding |
| `DELETE /shares/granted/{contactSub}/{setupId}` | owner | revoke |
| `DELETE /shares/received/{ownerSub}/{setupId}` | contact | leave |
| `DELETE /shares/invite/{codeRef}` | owner | withdraw an unclaimed code |

They live on the sync API because that is where identity-gated per-account state already lives; the auth service is about *becoming* a user, not about what users may do to each other.

Revoke and leave are the same operation named from opposite ends, and both are always available. Leaving is not a lesser right than revoking: neither party needs the other's cooperation to end the arrangement.

**The path never carries the caller's own identity.** It names the *other* party; the caller's side always comes from the verified token. Both DynamoDB keys a delete touches therefore embed the caller, so there is no request a caller can construct that names a grant they are not part of. The key derivation *is* the authorization check.

After any grant change, both affected accounts get an opaque `{"changed":"shares"}` nudge on their own topic. Same contract as every other nudge: never parsed, any frame means pull.

## Authorizer (`services/iot-authorizer`)

The one security-critical change. After the JWT verifies, the authorizer queries `SHARED#<sub>` and appends **one policy document per grant**:

- **publish** `…/setup/<id>/frame` and `…/presence/<contactSub>`
- **retain-publish** `…/presence/<contactSub>` only
- **subscribe/receive** `…/setup/<id>/state` and `…/setup/<id>/config`

The asymmetry is the design. A guest publishes live frames and its own presence card, and reads what the applier publishes. It cannot publish `state` or `config`: the owner's applier is the sole authority on both, and a guest that could retain either could lie to every other surface — including after it left.

Guest presence cards go to one exact topic per guest, `…/presence/<contactSub>` — deliberately not per session, and with no wildcard at all. The authorizer runs during the WebSocket handshake, before a session id exists, so a per-session topic could only be authorized as `presence/<contactSub>-*`; and a wildcard over a topic namespace is an unbounded retained-write primitive. A guest could retain messages on arbitrarily many topics, the owner would replay every one on each reconnect, and revoking the grant would remove the guest's ability to clear them — leaving a mess that outlives the grant and that revocation makes permanent rather than fixing. Per-session granularity buys nothing anyway, since a connection's one Last Will stays on the guest's own topic and cards in the owner's space are explicit publish/clear either way. The cost is that a guest signed in on two devices shows one card.

Nothing unvalidated is spliced into an ARN. `PUT /setups/*` is a legal request, so a setup can genuinely be named `*` — and IAM wildcards match `/`, so that id reaching a policy would silently widen a grant across the owner's entire setup space. The invite route rejects ids outside `[A-Za-z0-9_-]`, and the authorizer refuses to emit a document for one regardless of what the write path allowed.

**Why one document per grant, and why the cap is 9.** AWS IoT accepts at most 10 policy documents from a custom authorizer, each at most 2048 characters. A grant's six ARNs (UUID subs, UUID setup ids) run about 1 KB, so two grants will not reliably share a document, and an oversized document is rejected *wholesale* — which would take the caller's own access down with it. One grant per document, with the caller's own space taking the first, gives a hard ceiling of 9. That number lives in `lux_wire::shares::MAX_GRANTS_PER_CONTACT` so the claim route and the authorizer cannot disagree about it.

The cap is enforced at **claim**, not at invite: the contact is not known until they redeem, and the limit is a property of the contact's connection, not the owner's generosity. Claiming past it fails loudly with a typed error. The authorizer independently truncates and logs an error if it ever sees more, which should be unreachable.

**Failure behaviour.** A grant lookup that errors yields no grants and logs an error — it does not deny the connection. This is closed with respect to shares (a failed read can never fabricate access) while keeping a DynamoDB blip from signing every user out of their own sync.

**Revocation latency.** A policy lives for the connection's refresh window, one hour. A revoked guest keeps the access they already hold until their next re-auth. This is the documented cost of connect-time authorization; if it ever matters, forcing a disconnect on revoke is the sharpening.

## The `config` topic

`lux_wire::ctl::Config` finally gives the reserved `…/setup/<id>/config` topic its schema: a setup compiled down to what it takes to *render a surface for it* — id, name, universe, patched channels (`{n, name, role}`), fixture summaries.

It is deliberately not a `SetupRecord`. That shape is the authoring model — opaque fixture JSON, revisions, tombstones — and it belongs to the account that owns it. This one is the rendering model: flat, small, and parseable by an embedded node with no JSON DOM. `role` is a plain string rather than an enum so that adding a role never strands an older reader, and the payload is versioned from day one because App Store review lag guarantees the two ends run different app versions.

The same payload later feeds product-box render nodes, which is why it is carved this way now rather than shaped around what a phone happens to need.

## Account deletion

Deleting an account ends every grant it is part of, in both directions, and the cleanup runs **before** the partition wipe. The ordering is load-bearing: once the caller's half of a grant is gone there is nothing left to find the other half from, and every contact would keep a row pointing at a deleted account — plus, until the authorizer's cache expired, publish rights into a dead topic space.

As an owner: every contact's mirror row is removed, outstanding invite codes are burned, and the retained `config` frames are cleared. As a contact: the mirrored row in each owner's partition is removed. Everyone affected gets a nudge.

The config sweep runs over *every* setup on the account rather than the currently-granted ones: a setup that was shared and then revoked still has a retained config, and driving the sweep from live grants alone would walk straight past it.

**This step is fallible and the wipe is gated on it.** A stale mirror is not a cosmetic leftover — the authorizer reads `SHARED#<sub>` and nothing else, so a surviving row is a live grant into a topic space whose owner account no longer exists, with the owner's row and the owner's Cognito user both gone. There is no revoking that. So a failed read or delete fails the whole request; deletion is idempotent end to end, and the client's retry re-reads a shorter list and finishes.

Clearing retained config is done **server-side**, not by the app before it signs out. Two reasons. The contact direction has no app-side option at all — a departing guest has no publish rights in the owner's space, correctly — so one direction is forced server-side and one mechanism beats two. And an app that is killed mid-deletion (or an iOS app suspended between steps) would otherwise leave a retained frame carrying setup and channel names addressed to an account that no longer exists. Deleting an account has to mean the data actually leaves.

The delete-account confirm shows both directions before it happens, alongside the setup and paired-device counts.

## Accepted limits

- **Presence can go stale.** A connection has exactly one Last Will and it targets the guest's own presence topic, so a guest's card in the owner's space is explicit-publish/explicit-clear. An ungraceful drop can leave a card up until the guest reconnects or signs out. Cards carry a timestamp so surfaces can render staleness.
- **Guest publish rate** rides the existing ~25 Hz client-side coalescing. Among invited contacts that is a courtesy, not a defence.
- **Grant labels are denormalized.** The contact's and owner's email are recorded on the grant when it is created; renaming a setup or changing an email later does not rewrite existing rows. The live name reaches the guest through `Config`, which is what their surface actually draws from.
- **Simultaneous claims can overshoot the cap.** They can all read the grant count before any of them commits, so the overshoot is bounded by how many live codes one contact holds at once, not by one. The authorizer is the enforcing line either way and fails closed by truncating; the visible effect is that grants past the ninth silently do nothing.
- **Deletion clears retained `config`, but not retained `state` or presence cards.** Those predate this feature and are not part of a grant, and clearing them would need `iot:Publish` on the `state` and `presence` topics — which would cost the property that makes the current grant safe: that the sync API cannot publish a frame or a state, and so cannot drive anybody's lights. (`iot:RetainPublish` alone does not suffice; IoT requires `iot:Publish` for any publish, retained or not.) Worth revisiting on its own terms — a presence card's `name` is the machine hostname, which is often a person's name — but not by quietly widening this role.
