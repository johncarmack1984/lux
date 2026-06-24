# Changelog

## [0.5.0](https://github.com/johncarmack1984/lux/compare/v0.4.1...v0.5.0) (2026-06-24)


### Features

* add network DMX output (sACN/E1.31) with auto-detected device selection ([#53](https://github.com/johncarmack1984/lux/issues/53)) ([b9b0f2b](https://github.com/johncarmack1984/lux/commit/b9b0f2b7716087d1d32dc16bb13577e0e61441fb))

## [0.4.1](https://github.com/johncarmack1984/lux/compare/v0.4.0...v0.4.1) (2026-06-24)


### Bug Fixes

* **deps:** patch glib to clear RUSTSEC-2024-0429 ([#51](https://github.com/johncarmack1984/lux/issues/51)) ([766e8dc](https://github.com/johncarmack1984/lux/commit/766e8dc19300c092419187c029fc3e0c6c1e18b4))

## [0.4.0](https://github.com/johncarmack1984/lux/compare/v0.3.3...v0.4.0) (2026-06-24)


### Features

* migrate frontend to Vite + TanStack Router ([#49](https://github.com/johncarmack1984/lux/issues/49)) ([9091206](https://github.com/johncarmack1984/lux/commit/9091206f22d4d65a5665812a46f1c8aef1b7c20d))

## [0.3.3](https://github.com/johncarmack1984/lux/compare/v0.3.2...v0.3.3) (2026-06-23)


### Bug Fixes

* channel persistence, slider throttling, and dead-code cleanup ([#47](https://github.com/johncarmack1984/lux/issues/47)) ([c94a9a1](https://github.com/johncarmack1984/lux/commit/c94a9a15f323718436d2d73d542ab2ea293ec5eb))

## [0.3.2](https://github.com/johncarmack1984/lux/compare/v0.3.1...v0.3.2) (2026-06-18)


### Bug Fixes

* **deps:** align [@tauri-apps](https://github.com/tauri-apps) JS packages to tauri 2.11 ([#42](https://github.com/johncarmack1984/lux/issues/42)) ([bdbdc2e](https://github.com/johncarmack1984/lux/commit/bdbdc2ebfa55a4c4dc6fe612d3386d61c01c9ef3))

## [0.3.1](https://github.com/johncarmack1984/lux/compare/v0.3.0...v0.3.1) (2026-06-18)


### Bug Fixes

* **deps:** upgrade tauri 2.9.3 -&gt; 2.11.3 (GHSA-7gmj-67g7-phm9) ([#40](https://github.com/johncarmack1984/lux/issues/40)) ([ba3c2b7](https://github.com/johncarmack1984/lux/commit/ba3c2b7fcdcbf1c357ad2d8e167b191a65db3f7e))

## [0.3.0](https://github.com/johncarmack1984/lux/compare/v0.2.2...v0.3.0) (2026-06-18)


### Features

* sign and notarize macOS releases (Developer ID + notarization) ([#37](https://github.com/johncarmack1984/lux/issues/37)) ([e3e555b](https://github.com/johncarmack1984/lux/commit/e3e555b271cbee91da95f3eb4c46b7839f4a2b96))

## [0.2.2](https://github.com/johncarmack1984/lux/compare/v0.2.1...v0.2.2) (2026-06-18)


### Bug Fixes

* use a password-protected updater key from a JSON secret ([#35](https://github.com/johncarmack1984/lux/issues/35)) ([8c67a62](https://github.com/johncarmack1984/lux/commit/8c67a62c56e198276e2349ded4aff3ac8d7fca2a))

## [0.2.1](https://github.com/johncarmack1984/lux/compare/v0.2.0...v0.2.1) (2026-06-18)


### Bug Fixes

* pin updater/process JS plugins to match the Rust crates ([#32](https://github.com/johncarmack1984/lux/issues/32)) ([34c3aa3](https://github.com/johncarmack1984/lux/commit/34c3aa3c649849d9682b1bdfad9ba9569c46cf5b))

## [0.2.0](https://github.com/johncarmack1984/lux/compare/v0.1.1...v0.2.0) (2026-06-18)


### Features

* in-app auto-updater (tauri-plugin-updater) ([#30](https://github.com/johncarmack1984/lux/issues/30)) ([44762ef](https://github.com/johncarmack1984/lux/commit/44762efb1eb1f20018e8f366fbb47231b5e2e16a))

## [0.1.1](https://github.com/johncarmack1984/lux/compare/v0.1.0...v0.1.1) (2026-06-18)


### Bug Fixes

* revert the breaking cargo bump ([#21](https://github.com/johncarmack1984/lux/issues/21)) + guard tauri in dependabot ([c8c35ba](https://github.com/johncarmack1984/lux/commit/c8c35ba7f67b6e2c79829a2e940f93b730a6cbf4))
* type-only import so next build / tauri build resolves ([da6ae57](https://github.com/johncarmack1984/lux/commit/da6ae574db3f51b03e8ade90f1f0b68f7b4f2464))
* use type-only import for ChannelProps so next build resolves ([b977d82](https://github.com/johncarmack1984/lux/commit/b977d82344acfa855f0e0141961a31928ca471cd))

## [0.1.0](https://github.com/johncarmack1984/lux/compare/v0.0.3...v0.1.0) (2026-06-18)


### Continuous Integration

* add release pipeline (release-please + tauri-action) ([98f5d02](https://github.com/johncarmack1984/lux/commit/98f5d024d5aedb469740a097f42affaef790519a))
