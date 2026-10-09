# lux-node

Headless lux for an always-on box — a Linux machine (x86_64, or a Raspberry Pi on a 64-bit OS) or a Mac: it signs into your lux account, holds the same realtime channel as the apps, applies remote-control frames addressed to one setup, and renders them to sACN. No display, no GTK — a single binary run as a system service, a systemd unit on Linux and a launchd daemon on macOS. Your phone anywhere drives the lights at home while this runs.

It transmits at E1.31 priority 90 (surfaces send 100), so touching a fader on any device on the LAN overrides the node until you let go.

## Install

On Linux:

```bash
curl -fsSL -o lux-node "https://github.com/johncarmack1984/lux/releases/latest/download/lux-node-$(uname -m)-linux"
chmod +x lux-node
sudo ./lux-node install --pair
```

Assets exist for `x86_64` and `aarch64` (Raspberry Pi 3/4/5/Zero 2 W on a 64-bit OS), named to match `uname -m`.

On macOS (one universal binary for Apple silicon and Intel):

```bash
curl -fsSL -o lux-node "https://github.com/johncarmack1984/lux/releases/latest/download/lux-node-macos"
chmod +x lux-node
sudo ./lux-node install --pair
```

Download it with curl, not a browser: a browser marks the file as quarantined, and Gatekeeper won't run a quarantined binary that isn't notarized.

`--pair` claims the box from the lux app instead of a password. Install prints a short code and waits; on a phone or computer on the same network, open the lux app, go to Settings > Devices, find the box under Add a device, check that the code matches, pick the setup it should drive, and approve. Pairing mints a device session that lasts 10 years. Without `--pair`, install signs in with an email and password instead, and that session lasts 30 days, after which the node can't reconnect until it signs in again; a Sign in with Apple account has no password to sign in with at all.

`install` does everything and is safe to re-run (it upgrades the binary — even while the service is running, restarting it onto the new build — and fixes whatever is missing): copies itself to `/usr/local/bin`, creates the service account and dirs, writes the service definition, pairs or signs in as the service identity, then, unless pairing already chose one, lists the account's setups so you pick by name (universe comes from the record; a UUID prompt only appears if the sync API is unreachable — and only when no binding exists yet), starts the service, and keeps the box awake — pass `--keep-sleep` to skip that last part.

- Linux: a `lux-node` system user, state in `/var/lib/lux-node`, and the unit at `/etc/systemd/system/lux-node.service`. Keeping awake masks sleep/suspend. Watch it with `journalctl -u lux-node -f`.
- macOS: a hidden `_luxnode` role account, state in `/Library/Application Support/lux-node`, and the daemon at `/Library/LaunchDaemons/com.johncarmack.lux-node.plist`, logging to `/Library/Logs/lux-node/lux-node.log` (watch it with `tail -f`). Keeping awake is a `caffeinate -s` assertion held for the node's process: the Mac doesn't sleep on AC power while the node runs, sleeps as usual on battery, and the assertion ends with the node. It runs as a daemon rather than an agent because macOS exempts launchd daemons from Local Network privacy, which sACN multicast falls under. With FileVault on, nothing starts after a restart until someone unlocks the disk at the login window.

The node's binding (the setup it applies and the universe it sends on) lives in `node.json` in the state dir after pairing, or in `/etc/lux-node/config.json` after a password install; `/etc/lux-node/config.json` wins when both exist. Optional keys in either: `"interface"` (IPv4 of the NIC to egress multicast from, for multi-homed hosts) and `"priority"` (default 90). Pairing again rewrites `node.json`.

(No release with the asset yet, or hacking on a checkout? The **node-build** workflow's `lux-node-x86_64-linux` / `lux-node-aarch64-linux` / `lux-node-macos` artifacts produce the same binaries, as do `cargo build --release --target <arch>-unknown-linux-musl -p lux-node` on Linux and `cargo build --release -p lux-node` on a Mac, for that Mac's architecture.)

On an Intel Mac mini running Linux, also enable auto power-on after a power failure (the setpci register varies by generation — verify for the model before poking):

```bash
sudo setpci -s 0:1f.0 0xa4.b=0
```

A desktop Mac running macOS does the same with `sudo pmset -a autorestart 1`; with FileVault on, it comes back to the unlock screen.

## Stop or remove

On Linux, `sudo systemctl disable --now lux-node` stops the node and keeps it from starting at boot.

On macOS, `sudo launchctl bootout system/com.johncarmack.lux-node` stops it; it starts again at boot unless `/Library/LaunchDaemons/com.johncarmack.lux-node.plist` is deleted too.

## What it does on the wire

Outbound-only WSS to AWS IoT Core through the same JWT authorizer as the apps — no ports opened at the house. It subscribes its setup's `frame` and `state` topics by name, because AWS IoT never delivers a retained message through a wildcard. On every connect it takes in the setup's retained `state` echo before applying `frame`s, which restores the look after a restart; frames that arrive first are held and applied on top. It re-renders every second once it holds a look, through reconnects too, since sACN receivers drop quiet sources. It announces a retained presence card (cleared by its Last Will) and publishes its own state echo after each applied change, so every surface shows the rig's truth. AWS IoT stores one retained message per topic per second and drops the rest, so echoes go out live with at most one retained per 1.1 s, plus a trailing retained one so the stored copy catches up to the final look. It never echoes a state it seeded from.
