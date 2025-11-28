use libftd2xx::DeviceInfo;
use libftd2xx::DeviceType;
use libftd2xx::FtStatus;
use libftd2xx::{DeviceStatus, Ftdi, FtdiCommon, StopBits};
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const BUF_SIZE: usize = 513;
const BAUDRATE: u32 = 250000;
const BITS_8: libftd2xx::BitsPerWord = libftd2xx::BitsPerWord::Bits8;
const STOP_BITS_2: libftd2xx::StopBits = StopBits::Bits2;
const PARITY_NONE: libftd2xx::Parity = libftd2xx::Parity::No;
const READ_TIMEOUT: Duration = Duration::from_millis(1000);
const WRITE_TIMEOUT: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone)]
struct Buffer([u8; BUF_SIZE]);

impl Default for Buffer {
    fn default() -> Self {
        Buffer([0; BUF_SIZE])
    }
}

impl Serialize for Buffer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_tuple(BUF_SIZE)?;
        for i in 0..BUF_SIZE {
            seq.serialize_element(&self.0[i])?;
        }
        seq.end()
    }
}

/// This struct represents an Enttec Open DMX Interface. To create a new instance use the `new()` method or construct it "by hand".
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EnttecOpenDMX {
    /// FTDI device
    ftdi: Option<Arc<Mutex<Ftdi>>>,
    /// buffer which can be written
    buffer: Buffer,
    /// defaults to "EnttecOpenDMX"
    name: String,
    /// initial device status
    status: DeviceStatus,
    /// initial device info
    info: DeviceInfo,
}

impl EnttecOpenDMX {
    /// Creates a new instance representing the Open DMX Interface, it uses the auto discovery provided by the [libftd2xx] crate.
    ///
    /// To select a specific device check the documentation of the [libftd2xx] crate and then create the struct.
    ///
    /// [libftd2xx]: https://crates.io/crates/libftd2xx
    pub fn new() -> Result<EnttecOpenDMX, String> {
        let device_name = String::from("EnttecOpenDMX");
        log::trace!("Creating new EnttecOpenDMX instance");

        let ft = match Ftdi::new() {
            Ok(ftdi) => {
                log::trace!("Ftdi::new() succeeded");
                Some(Arc::new(Mutex::new(ftdi)))
            }
            Err(e) => {
                // Log the error and proceed without a device connection
                log::error!("Could not connect to EnttecOpenDMX: {:?}", e);
                None
            }
        };

        if let Some(ref ft) = ft {
            let mut ftdi = ft.lock().unwrap();
            let device_info = ftdi.device_info().map_err(|e| format!("{:?}", e))?;
            let ftdi_status = ftdi.status().map_err(|e| format!("{:?}", e))?;

            Ok(EnttecOpenDMX {
                ftdi: Some(ft.clone()),
                buffer: Buffer([0; BUF_SIZE]),
                name: device_name,
                status: ftdi_status,
                info: device_info,
            })
        } else {
            Ok(EnttecOpenDMX {
                ftdi: None,
                buffer: Buffer([0; BUF_SIZE]),
                name: device_name,
                status: DeviceStatus {
                    ammount_in_rx_queue: 0,
                    ammount_in_tx_queue: 0,
                    event_status: 0,
                },
                info: DeviceInfo {
                    port_open: false,
                    speed: None,
                    device_type: DeviceType::Unknown,
                    vendor_id: 0,
                    product_id: 0,
                    serial_number: String::from(""),
                    description: String::from(""),
                },
            })
        }
    }

    /// Opens the connection with the Interface.
    pub fn open(&mut self) -> Result<(), FtStatus> {
        if let Some(ft) = self.ftdi.as_ref() {
            let mut ftdi = ft.lock().unwrap();
            ftdi.reset()?;
            ftdi.set_baud_rate(BAUDRATE)?;
            ftdi.set_data_characteristics(BITS_8, STOP_BITS_2, PARITY_NONE)?;
            ftdi.set_timeouts(READ_TIMEOUT, WRITE_TIMEOUT)?;
            ftdi.set_flow_control_none()?;
            ftdi.clear_rts()?;
            ftdi.purge_rx()?;
            ftdi.purge_tx()?;
            Ok(())
        } else {
            Err(FtStatus::DEVICE_NOT_FOUND) // Or any other appropriate error
        }
    }

    /// Allows to set the value of a specific DMX channel. For the channel only values lower than 513 are allowed or the code will `panic!`
    pub fn _set_channel(&mut self, channel: usize, value: u8) {
        if channel < 513 {
            self.buffer.0[channel] = value;
        } else {
            panic!("invalid channel: {}", channel);
        }
    }

    /// Allows too set the whole state of the universe at once.
    #[allow(dead_code)]
    pub fn set_buffer(&mut self, buf: [u8; BUF_SIZE]) {
        self.buffer = Buffer(buf);
    }

    /// Allows to set the whole state of the universe at once to 0.
    pub fn _blackout(&mut self) {
        self.buffer = Buffer([0; BUF_SIZE]);
    }

    /// Renders the current buffer
    pub fn render(&mut self) -> Result<(), FtStatus> {
        if let Some(ft) = self.ftdi.as_ref() {
            let mut ftdi = ft.lock().unwrap();
            ftdi.set_break_on()?;
            ftdi.set_break_off()?;
            ftdi.write_all(&self.buffer.0).unwrap();
            Ok(())
        } else {
            Err(FtStatus::DEVICE_NOT_FOUND) // Or any other appropriate error
        }
    }

    /// Closes an open connection.
    #[allow(dead_code)]
    pub fn close(&mut self) -> Result<(), FtStatus> {
        if let Some(ft) = self.ftdi.as_ref() {
            let mut ftdi = ft.lock().unwrap();
            ftdi.close()?;
            Ok(())
        } else {
            Err(FtStatus::DEVICE_NOT_FOUND) // Or any other appropriate error
        }
    }
}

impl Default for EnttecOpenDMX {
    fn default() -> Self {
        EnttecOpenDMX::new().unwrap()
    }
}
