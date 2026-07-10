# Changelog

## [0.17.0](https://github.com/johncarmack1984/lux/compare/v0.16.0...v0.17.0) (2026-07-10)


### Features

* **ios:** enable multicast networking entitlement ([#137](https://github.com/johncarmack1984/lux/issues/137)) ([925f9c4](https://github.com/johncarmack1984/lux/commit/925f9c48078f9d65cbd622b167edfc8c1003b202))

## [0.16.0](https://github.com/johncarmack1984/lux/compare/v0.15.0...v0.16.0) (2026-07-08)


### Features

* reading light preset in the fixture color picker ([#135](https://github.com/johncarmack1984/lux/issues/135)) ([49760ea](https://github.com/johncarmack1984/lux/commit/49760ea9d131ebf86322e423481afa9019063beb))

## [0.15.0](https://github.com/johncarmack1984/lux/compare/v0.14.0...v0.15.0) (2026-07-08)


### Features

* ship Mac App Store builds to TestFlight on release ([#132](https://github.com/johncarmack1984/lux/issues/132)) ([5d1fd43](https://github.com/johncarmack1984/lux/commit/5d1fd43921c5d0e4ef0acc1e3e8bba91e8be0513))

## [0.14.0](https://github.com/johncarmack1984/lux/compare/v0.13.1...v0.14.0) (2026-07-08)


### Features

* show the app version in the footer ([#129](https://github.com/johncarmack1984/lux/issues/129)) ([f49ad17](https://github.com/johncarmack1984/lux/commit/f49ad170d680f1107373155a8f1f59e4a3878ae7))

## [0.13.1](https://github.com/johncarmack1984/lux/compare/v0.13.0...v0.13.1) (2026-07-04)


### Bug Fixes

* **ci:** guard the site CloudFront alias lookup against null aliases ([#113](https://github.com/johncarmack1984/lux/issues/113)) ([99eb934](https://github.com/johncarmack1984/lux/commit/99eb93465a24c55ec0ee984066eef57af182420c))
* **desktop:** reflect account and patch state on iOS ([#114](https://github.com/johncarmack1984/lux/issues/114)) ([0f47a5f](https://github.com/johncarmack1984/lux/commit/0f47a5f092df9e33324fcb37167c7ccbe9607cee))
* **desktop:** reflect DMX, sync, and channel state on iOS ([#116](https://github.com/johncarmack1984/lux/issues/116)) ([0e24fab](https://github.com/johncarmack1984/lux/commit/0e24fab8679d98d019adafcc2703a2dcb98f5658))
* **desktop:** register the data-protection keychain store on iOS ([#112](https://github.com/johncarmack1984/lux/issues/112)) ([cd8ebd1](https://github.com/johncarmack1984/lux/commit/cd8ebd18ed09955dadde57656d9750b3db69f4e9))
* **sync-api:** install a JWT crypto backend so verification works ([#118](https://github.com/johncarmack1984/lux/issues/118)) ([c003af9](https://github.com/johncarmack1984/lux/commit/c003af9a17a3fc32e607d21568dde42ce7eb3a66))

## [0.13.0](https://github.com/johncarmack1984/lux/compare/v0.12.2...v0.13.0) (2026-07-04)


### Features

* in-app account deletion ([#106](https://github.com/johncarmack1984/lux/issues/106)) ([8158859](https://github.com/johncarmack1984/lux/commit/8158859353a1a1595f04418347b03146920bc118))
* product site at lux.johncarmack.com + shared @lux/ui package ([#109](https://github.com/johncarmack1984/lux/issues/109)) ([68f71a3](https://github.com/johncarmack1984/lux/commit/68f71a31db45b55fa87f9621db2c94bf8f59288e))

## [0.12.2](https://github.com/johncarmack1984/lux/compare/v0.12.1...v0.12.2) (2026-07-03)


### Bug Fixes

* **desktop:** persist the live universe across restarts ([#99](https://github.com/johncarmack1984/lux/issues/99)) ([a0c8e60](https://github.com/johncarmack1984/lux/commit/a0c8e6096e2165278b1c069fdd73e934019b762b))

## [0.12.1](https://github.com/johncarmack1984/lux/compare/v0.12.0...v0.12.1) (2026-07-03)


### Bug Fixes

* **bot:** rename the crate to lux-discord-bot to match its Lambda function ([#95](https://github.com/johncarmack1984/lux/issues/95)) ([a22fcf6](https://github.com/johncarmack1984/lux/commit/a22fcf60154c5b105addbd21824b759efd443ab9))

## [0.12.0](https://github.com/johncarmack1984/lux/compare/v0.11.0...v0.12.0) (2026-07-03)


### Features

* environment values as generated data — endpoints.prod.json, no env files ([#93](https://github.com/johncarmack1984/lux/issues/93)) ([bd9bd00](https://github.com/johncarmack1984/lux/commit/bd9bd00c6e80f61c7d81b7e3ae316618d77f37ec))

## [0.11.0](https://github.com/johncarmack1984/lux/compare/v0.10.0...v0.11.0) (2026-07-03)


### Features

* **sync:** shared wire contract + realtime change nudges over IoT WebSocket ([#89](https://github.com/johncarmack1984/lux/issues/89)) ([68b882b](https://github.com/johncarmack1984/lux/commit/68b882bfb4e1472c6965b243ad1c2af25a14eeec))

## [0.10.0](https://github.com/johncarmack1984/lux/compare/v0.9.0...v0.10.0) (2026-06-30)


### Features

* in-app DMX output picker in the manage-setups popover ([#79](https://github.com/johncarmack1984/lux/issues/79)) ([1990588](https://github.com/johncarmack1984/lux/commit/1990588d2b6fcc79f3326b1aacc04de1f13fb8d2))

## [0.9.0](https://github.com/johncarmack1984/lux/compare/v0.8.0...v0.9.0) (2026-06-26)


### Features

* sync-status indicator, pull-on-focus, and offline-retry backoff ([#73](https://github.com/johncarmack1984/lux/issues/73)) ([d57885f](https://github.com/johncarmack1984/lux/commit/d57885f6da772ba1366b515ad177cbd6581bd137))

## [0.8.0](https://github.com/johncarmack1984/lux/compare/v0.7.0...v0.8.0) (2026-06-25)


### Features

* accounts and cloud sync of setups ([#62](https://github.com/johncarmack1984/lux/issues/62)) ([11040e2](https://github.com/johncarmack1984/lux/commit/11040e288fc2b1da5086e212ebb74282d7acad41))


### Bug Fixes

* bake Cognito config into the build so accounts work in releases ([#65](https://github.com/johncarmack1984/lux/issues/65)) ([168ee1c](https://github.com/johncarmack1984/lux/commit/168ee1cca31430687cbd1588c535c0ebc7a4d6bf))

## [0.7.0](https://github.com/johncarmack1984/lux/compare/v0.6.1...v0.7.0) (2026-06-25)


### Features

* add named setups, each a fixture patch bound to a DMX universe ([#60](https://github.com/johncarmack1984/lux/issues/60)) ([8208e40](https://github.com/johncarmack1984/lux/commit/8208e40eec0e8e6669a781e45fe0be10365b8f9e))

## [0.6.1](https://github.com/johncarmack1984/lux/compare/v0.6.0...v0.6.1) (2026-06-25)


### Bug Fixes

* keep src-tauri/Cargo.lock in sync with the released version ([#58](https://github.com/johncarmack1984/lux/issues/58)) ([b3d3569](https://github.com/johncarmack1984/lux/commit/b3d356957a254344672bcb861b7132988b11030c))

## [0.6.0](https://github.com/johncarmack1984/lux/compare/v0.5.0...v0.6.0) (2026-06-25)


### Features

* add fixtures with patching, presets, and role-aware color mixing ([#57](https://github.com/johncarmack1984/lux/issues/57)) ([f3ea72f](https://github.com/johncarmack1984/lux/commit/f3ea72f6a0d3c1a721d6f1160b99dc03cb3950ad))
* drive the full 512-channel DMX universe ([#55](https://github.com/johncarmack1984/lux/issues/55)) ([3040b8a](https://github.com/johncarmack1984/lux/commit/3040b8a1c0532a8dcb991ca90ab9d4957493fb8a))

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
