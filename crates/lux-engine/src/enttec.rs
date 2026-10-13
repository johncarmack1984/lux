//! Enttec Open DMX USB output — direct USB control of the FT232R the host
//! bit-bangs DMX through, with no kernel serial driver, no libftdi, and no
//! proprietary D2XX blob (nusb is pure Rust, so the GPL release binary stays
//! self-contained and the static musl build keeps working). The desktop drives
//! the same hardware through libftd2xx behind its own `DmxSink`; this sink is
//! what lets a headless node own the dongle on an appliance.
//!
//! Unlike the sACN sink (fire-and-forget UDP), the Open DMX has no processor:
//! the host must generate every frame, and a 513-byte frame at 250 kbaud takes
//! ~23 ms to drain through the chip's buffer — far too long to block the
//! node's async loop per render. So the sink owns a transmit thread that
//! repeats the latest look continuously (~40 Hz, paced by the chip itself
//! draining), and [`DmxSink::render`] just swaps the frame being repeated.
//! Continuous repetition is also what the DMX wire wants: fixtures vary in how
//! they treat a quiet line, and the repeating frame makes the question moot.
//! The thread reopens the device with backoff when it vanishes (unplugged,
//! briefly claimed elsewhere), so a replug heals without a restart.
//!
//! The control requests mirror libftdi's (`ftdi.h`): vendor requests to
//! recipient Device, channel index 1 (`INTERFACE_A`, which libftdi also uses
//! for single-channel chips like the FT232R), bulk OUT endpoint 0x02.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use nusb::transfer::{Control, ControlType, Recipient};
use nusb::Interface;

use crate::DmxSink;

/// FT232R as the Enttec Open DMX USB enumerates it.
const VENDOR_ID: u16 = 0x0403;
const PRODUCT_ID: u16 = 0x6001;
/// The FT232R's bulk OUT (write) endpoint.
const ENDPOINT_OUT: u8 = 0x02;
/// libftdi's channel index for a single-channel chip (`INTERFACE_A`).
const CHANNEL_INDEX: u16 = 1;

/// FTDI vendor requests (libftdi `ftdi.h` names, sans the `SIO_` prefix).
const REQUEST_RESET: u8 = 0x00;
const REQUEST_SET_FLOW_CTRL: u8 = 0x02;
const REQUEST_SET_BAUD_RATE: u8 = 0x03;
const REQUEST_SET_DATA: u8 = 0x04;
/// `REQUEST_RESET` values: full reset, purge RX, purge TX.
const RESET_SIO: u16 = 0;
const RESET_PURGE_RX: u16 = 1;
const RESET_PURGE_TX: u16 = 2;

/// DMX512: 250 kbaud, and a frame is one start code plus 512 slots.
const DMX_BAUD: u32 = 250_000;
const FRAME_LEN: usize = 513;

const USB_TIMEOUT: Duration = Duration::from_secs(1);
/// How long the transmit thread waits before looking for the device again.
const REOPEN_DELAY: Duration = Duration::from_secs(1);

/// `SET_DATA` value for DMX line settings (8 data bits, no parity, 2 stop
/// bits), with bit 14 driving the line break that frames every DMX packet.
fn line_value(break_on: bool) -> u16 {
    let base = 8 | (2 << 11);
    if break_on {
        base | (1 << 14)
    } else {
        base
    }
}

/// `SET_BAUD_RATE` (value, index) for an exact integer divisor of the FT232R's
/// 3 MHz base clock. DMX's 250 kbaud divides exactly (divisor 12), so the
/// fractional encoding and the 2/3 MBaud special cases never come into play —
/// anything inexact is refused rather than approximated.
fn baud_params(baud: u32) -> Option<(u16, u16)> {
    if baud == 0 || 3_000_000 % baud != 0 {
        return None;
    }
    let divisor = 3_000_000 / baud;
    if divisor < 2 {
        // 0 and 1 are the chip's 3/2 MBaud escape codes, not divisors.
        return None;
    }
    let value = u16::try_from(divisor & 0xFFFF).ok()?;
    let index = u16::try_from(divisor >> 16).ok()?;
    Some((value, index))
}

/// Pack fixture channels into a wire frame: slot 0 is the DMX start code
/// (0x00), slots 1..=512 the channel data, missing channels dark.
fn pack(channels: &[u8]) -> [u8; FRAME_LEN] {
    let mut frame = [0u8; FRAME_LEN];
    let n = channels.len().min(FRAME_LEN - 1);
    frame[1..=n].copy_from_slice(&channels[..n]);
    frame
}

/// Lock a mutex, recovering the data from a poisoned lock: the shared frame is
/// plain bytes, valid under any interleaving, and output must not stop because
/// some other thread panicked while holding it.
fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// What the sink shares with its transmit thread.
struct Shared {
    /// The frame the thread repeats; `render` swaps it.
    frame: Mutex<[u8; FRAME_LEN]>,
    /// Whether a device is currently open — `render`'s error signal, so the
    /// node's rate-limited send reporting covers "dongle unplugged".
    connected: AtomicBool,
    /// Shutdown flag, paired with `wake` so backoff sleeps end promptly.
    stop: AtomicBool,
    pause: Mutex<()>,
    wake: Condvar,
}

/// An Enttec Open DMX USB sender owning one FT232R (the first on the bus).
pub struct EnttecSink {
    shared: Arc<Shared>,
    transmitter: Option<JoinHandle<()>>,
}

impl EnttecSink {
    /// Start the sink. Succeeds even with no dongle attached: the transmit
    /// thread keeps looking for one, and `render` reports the absence.
    pub fn new() -> Result<Self, String> {
        let shared = Arc::new(Shared {
            frame: Mutex::new([0u8; FRAME_LEN]),
            connected: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            pause: Mutex::new(()),
            wake: Condvar::new(),
        });
        let for_thread = Arc::clone(&shared);
        let transmitter = std::thread::Builder::new()
            .name("enttec-tx".to_owned())
            .spawn(move || transmit_loop(&for_thread))
            .map_err(|e| format!("could not start the enttec transmit thread: {e}"))?;
        Ok(Self {
            shared,
            transmitter: Some(transmitter),
        })
    }
}

impl DmxSink for EnttecSink {
    fn render(&self, channels: &[u8]) -> Result<(), String> {
        // Store the look even while disconnected, so the moment the device
        // (re)appears the thread transmits current truth, not stale bytes.
        *lock_or_recover(&self.shared.frame) = pack(channels);
        if self.shared.connected.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("the Enttec Open DMX USB is not connected".to_owned())
        }
    }
}

impl Drop for EnttecSink {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.wake.notify_all();
        if let Some(transmitter) = self.transmitter.take() {
            // The thread finishes at most one frame (~25 ms) before seeing the
            // flag; a failed join only means it already panicked and died.
            let _ = transmitter.join();
        }
    }
}

/// The transmit thread: open the device, repeat the latest frame until a send
/// fails or shutdown is flagged, reopen with backoff. The chip draining its
/// buffer paces the loop at the DMX wire rate, so there is no timer here.
fn transmit_loop(shared: &Shared) {
    let mut missing_reported = false;
    while !shared.stop.load(Ordering::Acquire) {
        let interface = match open_device() {
            Ok(interface) => {
                log::info!("enttec: FT232R opened and configured for DMX");
                missing_reported = false;
                interface
            }
            Err(e) => {
                if !missing_reported {
                    log::warn!("enttec: {e}; retrying every {REOPEN_DELAY:?}");
                    missing_reported = true;
                }
                wait_for(shared, REOPEN_DELAY);
                continue;
            }
        };
        shared.connected.store(true, Ordering::Release);
        while !shared.stop.load(Ordering::Acquire) {
            let frame = *lock_or_recover(&shared.frame);
            if let Err(e) = send_frame(&interface, &frame) {
                log::warn!("enttec: frame send failed ({e}); reopening the device");
                break;
            }
        }
        shared.connected.store(false, Ordering::Release);
    }
}

/// Find, claim, and configure the first FT232R on the bus for DMX framing.
fn open_device() -> Result<Interface, String> {
    let info = nusb::list_devices()
        .map_err(|e| format!("USB enumeration failed: {e}"))?
        .find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
        .ok_or("no Enttec Open DMX USB (FTDI 0403:6001) on the bus")?;
    let device = info
        .open()
        .map_err(|e| format!("could not open the FT232R: {e} (udev permissions?)"))?;
    // Detaches ftdi_sio on Linux; a plain claim elsewhere.
    let interface = device
        .detach_and_claim_interface(0)
        .map_err(|e| format!("could not claim the FT232R: {e}"))?;

    let (baud_value, baud_index) =
        baud_params(DMX_BAUD).ok_or("250 kbaud does not divide the FTDI base clock")?;
    control(&interface, REQUEST_RESET, RESET_SIO, CHANNEL_INDEX)?;
    control(&interface, REQUEST_SET_BAUD_RATE, baud_value, baud_index)?;
    control(
        &interface,
        REQUEST_SET_DATA,
        line_value(false),
        CHANNEL_INDEX,
    )?;
    control(&interface, REQUEST_SET_FLOW_CTRL, 0, CHANNEL_INDEX)?;
    control(&interface, REQUEST_RESET, RESET_PURGE_RX, CHANNEL_INDEX)?;
    control(&interface, REQUEST_RESET, RESET_PURGE_TX, CHANNEL_INDEX)?;
    Ok(interface)
}

/// One DMX packet: the line break + mark-after-break that frame every packet
/// (each control round trip spans at least one full-speed USB frame, which
/// comfortably exceeds the 88 µs/8 µs minima — the same no-sleep shape the
/// desktop sink has run in production), then the 513 bytes.
fn send_frame(interface: &Interface, frame: &[u8; FRAME_LEN]) -> Result<(), String> {
    control(interface, REQUEST_SET_DATA, line_value(true), CHANNEL_INDEX)?;
    control(
        interface,
        REQUEST_SET_DATA,
        line_value(false),
        CHANNEL_INDEX,
    )?;
    let written = pollster::block_on(interface.bulk_out(ENDPOINT_OUT, frame.to_vec()))
        .into_result()
        .map_err(|e| format!("bulk write: {e}"))?;
    if written.actual_length() != FRAME_LEN {
        return Err(format!(
            "short frame write: {} of {FRAME_LEN} bytes",
            written.actual_length()
        ));
    }
    Ok(())
}

/// One FTDI vendor control request.
fn control(interface: &Interface, request: u8, value: u16, index: u16) -> Result<(), String> {
    interface
        .control_out_blocking(
            Control {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request,
                value,
                index,
            },
            &[],
            USB_TIMEOUT,
        )
        .map_err(|e| format!("control request {request:#04x}: {e}"))?;
    Ok(())
}

/// Sleep that [`EnttecSink::drop`] can interrupt, so shutdown never waits out
/// a reopen backoff.
fn wait_for(shared: &Shared, timeout: Duration) {
    let guard = lock_or_recover(&shared.pause);
    if !shared.stop.load(Ordering::Acquire) {
        // The result is irrelevant: both a timeout and a wake re-check `stop`.
        let _ = shared.wake.wait_timeout(guard, timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dmx_line_settings_are_8n2_with_break_on_bit_14() {
        assert_eq!(line_value(false), 0x1008);
        assert_eq!(line_value(true), 0x5008);
    }

    #[test]
    fn dmx_baud_divides_the_base_clock_exactly() {
        assert_eq!(baud_params(250_000), Some((12, 0)));
    }

    #[test]
    fn inexact_and_escape_code_bauds_are_refused() {
        assert_eq!(baud_params(0), None);
        assert_eq!(baud_params(115_200), None, "not an exact divisor");
        assert_eq!(baud_params(3_000_000), None, "divisor 1 is an escape code");
    }

    #[test]
    fn pack_prepends_the_start_code_and_truncates() {
        let frame = pack(&[7, 8, 9]);
        assert_eq!(frame[0], 0, "DMX null start code");
        assert_eq!(&frame[1..4], &[7, 8, 9]);
        assert!(frame[4..].iter().all(|&v| v == 0));

        let oversized = vec![255u8; 600];
        let frame = pack(&oversized);
        assert_eq!(frame.len(), FRAME_LEN);
        assert!(frame[1..].iter().all(|&v| v == 255));
    }
}
