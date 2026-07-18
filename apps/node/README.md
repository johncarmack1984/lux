# lux-node

Headless lux for an always-on Linux box: it signs into your lux account, holds the same realtime channel as the apps, applies remote-control frames addressed to one setup, and renders them to sACN. No display, no GTK — a single static binary and a systemd unit. Your phone anywhere drives the lights at home while this runs.

It transmits at E1.31 priority 90 (surfaces send 100), so touching a fader on any device on the LAN overrides the node until you let go.

## Install

```bash
curl -fsSL -o lux-node https://github.com/johncarmack1984/lux/releases/latest/download/lux-node-x86_64-linux
chmod +x lux-node
sudo ./lux-node install
```

`install` does everything and is safe to re-run (it upgrades the binary and fixes whatever is missing): copies itself to `/usr/local/bin`, creates the `lux-node` system user and dirs, writes the systemd unit, signs in as the service identity, then lists the account's setups so you pick by name (universe comes from the record; a UUID prompt only appears if the sync API is unreachable — and only when no config exists yet), enables the service, and masks sleep/suspend — pass `--keep-sleep` to skip that last part. Watch it with `journalctl -u lux-node -f`.

The setup id is the id of the setup this node applies — visible in the app's sync record, or ask any signed-in device. Optional keys in `/etc/lux-node/config.json`: `"interface"` (IPv4 of the NIC to egress multicast from, for multi-homed hosts) and `"priority"` (default 90).

(No release with the asset yet, or hacking on a checkout? The **node-build** workflow's `lux-node-x86_64-linux` artifact and `cargo build --release --target x86_64-unknown-linux-musl -p lux-node` produce the same binary.)

On an Intel Mac mini, also enable auto power-on after a power failure (the setpci register varies by generation — verify for the model before poking):

```bash
sudo setpci -s 0:1f.0 0xa4.b=0
```

## What it does on the wire

Outbound-only WSS to AWS IoT Core through the same JWT authorizer as the apps — no ports opened at the house. It subscribes your ctl space, applies `frame`s for its setup, seeds its buffer from the retained `state` echo on connect (restart persistence via the broker), re-renders every second (sACN receivers drop quiet sources), announces a retained presence card (cleared by its Last Will), and publishes its own state echo so every surface shows the rig's truth.
