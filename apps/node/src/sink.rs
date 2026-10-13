//! The node's DMX output: one of the engine's sinks, chosen by
//! [`crate::config::OutputKind`]. An enum rather than a `Box<dyn DmxSink>`
//! because shutdown differs by transport — only sACN has stream termination —
//! and a closed set keeps that visible at the match.

use lux_engine::enttec::EnttecSink;
use lux_engine::sacn::SacnSink;
use lux_engine::DmxSink;

/// The output this node renders to.
pub enum Output {
    Sacn(SacnSink),
    Enttec(EnttecSink),
}

impl Output {
    /// Push one frame of channel data to the hardware.
    pub fn render(&self, channels: &[u8]) -> Result<(), String> {
        match self {
            Output::Sacn(sink) => sink.render(channels),
            Output::Enttec(sink) => sink.render(channels),
        }
    }

    /// Shutdown courtesy, carrying the final look.
    pub fn terminate(&self, channels: &[u8]) {
        match self {
            // E1.31-2018 §6.2.6: stream-terminated packets stop receivers
            // from timing the quiet source out to black.
            Output::Sacn(sink) => sink.terminate(channels),
            // The transmit thread repeats the last look until the sink drops,
            // and fixtures hold their levels over a quiet DMX line — there is
            // no goodbye to send.
            Output::Enttec(_) => {}
        }
    }
}
