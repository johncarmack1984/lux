# lux dev tasks. Run `just` (or `just --list`) to see everything.
#
# The iOS recipes drive a physical device through devicectl: Xcode 26's Run
# button crashes on this tauri-generated project, so the build → install →
# launch loop lives here instead. The device is auto-detected from the first
# one `devicectl` lists; override with `just device=<id> ios-device` or by
# setting LUX_IOS_DEVICE.

bundle_id := "com.johncarmack.lux"
app := "apps/desktop/src-tauri/gen/apple/build/lux_iOS.xcarchive/Products/Applications/lux.app"
device := env_var_or_default("LUX_IOS_DEVICE", ```
    xcrun devicectl list devices 2>/dev/null \
      | grep -Eo '[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}' \
      | head -1
    ```)

# List the available recipes.
default:
    @just --list

# Front-end gate: production build, typecheck, lint (matches the PR gate).
check:
    cd apps/desktop && bun run build && bun run typecheck && bun run lint

# Run the desktop app in dev.
dev:
    cd apps/desktop && bun run tauri dev

# Rust workspace tests (includes the IPC bindings drift guard).
test:
    cargo test --workspace --locked

# Regenerate the committed IPC bindings (src/bindings.ts).
bindings:
    cd apps/desktop/src-tauri && REGEN_BINDINGS=1 cargo test

# Build the signed iOS app for a physical device (arm64).
ios-build:
    cd apps/desktop && bun run tauri ios build --debug --target aarch64 --ci

# Install the last iOS device build onto the device.
ios-install:
    xcrun devicectl device install app --device {{device}} {{app}}

# (Re)launch the app on the device.
ios-launch:
    xcrun devicectl device process launch --terminate-existing --device {{device}} {{bundle_id}}

# The full on-device loop: build, install, launch.
ios-device: ios-build ios-install ios-launch

# Pull the app's on-device log to ./lux-device.log.
ios-log:
    xcrun devicectl device copy from --device {{device}} \
      --domain-type appDataContainer --domain-identifier {{bundle_id}} \
      --source "Library/Application Support/{{bundle_id}}/logs/lux.log" \
      --destination ./lux-device.log
    @echo "wrote ./lux-device.log"

# Build for the booted simulator (no signing), install, and launch.
ios-sim:
    cd apps/desktop && bun run tauri ios build --debug --target aarch64-sim --no-sign --ci
    xcrun simctl install booted apps/desktop/src-tauri/gen/apple/build/arm64-sim/lux.app
    xcrun simctl launch booted {{bundle_id}}
