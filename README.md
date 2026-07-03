# Lux

A desktop app for driving DMX stage lighting, built as a native [Tauri](https://tauri.app) app: a Rust core that talks to the hardware and a Vite + TanStack Router + shadcn/ui front end to drive it. Lux runs a full 512-channel DMX universe with user-defined fixtures, patched into named setups (home / church / work) that sync to your account and work offline.

Output goes three ways: an [Enttec OpenDMX USB](https://www.enttec.com/product/dmx-usb-interfaces/open-dmx-usb/) interface, network DMX over sACN / Art-Net (nodes auto-discovered), and remote control from a Discord bot over AWS IoT.

The Rust side keeps the universe continuously synced to the hardware (`apps/desktop/src-tauri/src/{buffer,channels,sync}.rs`) behind a device abstraction (`apps/desktop/src-tauri/src/devices/`); the UI talks to it over [tauri-typed-ipc](https://github.com/johncarmack1984/tauri-typed-ipc) (a type-safe IPC crate I wrote), so the Rust↔TypeScript command layer is type-safe end to end. The desktop↔cloud wire is typed the same way: both sides share one contract crate (`crates/lux-wire`). More in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Demo

![Lux driving an RGBAW fixture over an Enttec OpenDMX USB interface](.github/lux-demo.gif)

*Setting fixture color live from the desktop UI: the RGBAW tube and the Enttec OpenDMX USB interface (bottom) respond in real time.*

## Stack

Rust · Tauri 2 · tauri-typed-ipc (type-safe IPC) · Vite · TanStack Router · React 19 · shadcn/ui ·
Tailwind v4 · Enttec OpenDMX USB (DMX512 over serial) · sACN / Art-Net (network DMX)

## Run it

```bash
cd apps/desktop
bun run tauri dev
```

## Features

- Full 512-channel universe: user-defined fixtures, patching, role-aware color mixing
- Enttec OpenDMX USB output, plus network DMX over sACN and Art-Net (nodes auto-discovered)
- Continuous DMX512 render/sync loop with correct break / mark-after-break framing
- Named setups, cloud-synced per account, offline-first — other devices pick up changes live over an open IoT WebSocket (nudged pull)
- Remote control from Discord over AWS IoT
- Type-safe Rust↔TS commands via tauri-typed-ipc
- Signed, notarized, self-updating macOS releases

## Screenshot

![The lux control grid](.github/lux-window-v0.0.3.png?raw=true)
