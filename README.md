# Lux

A desktop app for driving DMX stage lighting, built as a native
[Tauri](https://tauri.app) app: a Rust core that talks to the hardware and a
Next.js + shadcn/ui front end to drive it. Lux controls an
[Enttec OpenDMX USB](https://www.enttec.com/product/dmx-usb-interfaces/open-dmx-usb/)
interface and exposes the same controls three ways — the desktop UI, an HTTP API,
and a Discord bot.

The Rust side keeps a DMX channel buffer continuously synced to the hardware
(`src-tauri/src/{buffer,channels,sync}.rs`) behind a device abstraction
(`src-tauri/src/devices/`); the UI talks to it over **taurpc**, so the
Rust↔TypeScript command layer is type-safe end to end.

## Stack

Rust · Tauri 2 · taurpc (type-safe IPC) · Next.js 16 · React 19 · shadcn/ui ·
Tailwind v4 · Enttec OpenDMX USB (DMX512 over serial)

## Run it

```bash
cargo tauri dev
```

## Features

- ✅ Drives channels 1–6 of an Enttec OpenDMX USB (one RGBAW fixture)
- ✅ Accepts buffer commands over HTTP
- ✅ Controllable from a Discord bot
- ✅ Type-safe Rust↔TS commands via taurpc

### Planned

- [ ] User accounts synced via Turso
- [ ] Custom fixtures using the full 512-channel universe
- [ ] Binary distribution: macOS → iOS → Raspberry Pi → Windows → Android
- [ ] Client/server modes for shared home use
- [ ] CLI for remote control

## Screenshots

### v0.0.3
![Lux v0.0.3](.github/lux-window-v0.0.3.png?raw=true)

### v0.0.2
![Lux v0.0.2](.github/lux-window-v0.0.2.png?raw=true)

### v0.0.1
![Lux v0.0.1](.github/lux-window-v0.0.1.png?raw=true)
