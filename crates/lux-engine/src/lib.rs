//! The Tauri-free DMX core shared by the desktop app and the headless node.
//!
//! Everything here is plain functions and plain types: the [`DmxSink`] trait
//! and its sACN/E1.31 sender ([`sacn`]), the universe overlay semantics
//! ([`universe`]), the remote-control routing and gating decisions ([`ctl`]),
//! and the TLS/JWT glue every user-channel connection needs ([`tls`],
//! [`auth`]). The desktop app wraps these in Tauri state and event plumbing;
//! `lux-node` wraps them in a systemd-friendly binary. Nothing in this crate
//! may depend on Tauri — that boundary is what lets lux run headless on a
//! Linux box with no GTK/glib in the binary.

pub mod auth;
pub mod ctl;
pub mod sacn;
pub mod tls;
pub mod universe;

/// A DMX output: take channel bytes (slot 1 first) and push them to hardware.
pub trait DmxSink: Send + Sync {
    fn render(&self, channels: &[u8]) -> Result<(), String>;
}
