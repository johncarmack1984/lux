# lux-node

Headless lux for an always-on Linux box: it signs into your lux account, holds the same realtime channel as the apps, applies remote-control frames addressed to one setup, and renders them to sACN. No display, no GTK — a single static binary and a systemd unit. Your phone anywhere drives the lights at home while this runs.

It transmits at E1.31 priority 90 (surfaces send 100), so touching a fader on any device on the LAN overrides the node until you let go.

## Get the binary

Run the **node-build** workflow (Actions → node-build → Run workflow) and download the `lux-node-x86_64-linux` artifact, or build from a checkout:

```bash
cargo build --release --target x86_64-unknown-linux-musl -p lux-node
```

## Install (Ubuntu)

```bash
sudo install -m 755 lux-node /usr/local/bin/lux-node
sudo useradd --system --home /var/lib/lux-node --shell /usr/sbin/nologin lux-node || true
sudo mkdir -p /etc/lux-node
sudo tee /etc/lux-node/config.json > /dev/null <<'EOF'
{ "setupId": "<your setup uuid>", "universe": 1 }
EOF
sudo install -m 644 lux-node.service /etc/systemd/system/lux-node.service

# Sign in once as the service user (stores the refresh token, 0600):
sudo mkdir -p /var/lib/lux-node && sudo chown lux-node /var/lib/lux-node
sudo -u lux-node XDG_CONFIG_HOME=/var/lib/lux-node lux-node login you@example.com

sudo systemctl daemon-reload
sudo systemctl enable --now lux-node
journalctl -u lux-node -f
```

`setupId` is the id of the setup this node applies — visible in the app's sync record, or ask any signed-in device. Optional config keys: `"interface"` (IPv4 of the NIC to egress multicast from, for multi-homed hosts) and `"priority"` (default 90).

## An always-on box should stay on

```bash
sudo systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target
```

On an Intel Mac mini, also enable auto power-on after a power failure (the setpci register varies by generation — verify for the model before poking):

```bash
sudo setpci -s 0:1f.0 0xa4.b=0
```

## What it does on the wire

Outbound-only WSS to AWS IoT Core through the same JWT authorizer as the apps — no ports opened at the house. It subscribes your ctl space, applies `frame`s for its setup, seeds its buffer from the retained `state` echo on connect (restart persistence via the broker), re-renders every second (sACN receivers drop quiet sources), announces a retained presence card (cleared by its Last Will), and publishes its own state echo so every surface shows the rig's truth.
