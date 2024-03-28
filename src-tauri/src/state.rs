use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
// use tauri::{App, Manager};

use crate::{
    buffer::BUFFER_SIZE, channels::LuxChannel, colors::LuxLabelColor,
    devices::enttec_open_dmx_usb::EnttecOpenDMX,
};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxState {
    pub buffer: [u8; BUFFER_SIZE],
    pub channels: [LuxChannel; BUFFER_SIZE],
}

impl LuxState {
    #[allow(dead_code)]
    pub fn set_buffer(&mut self, buffer: [u8; BUFFER_SIZE]) {
        let mut temp_buffer = [0; 513];

        temp_buffer[1..BUFFER_SIZE + 1].copy_from_slice(&buffer);
        let mut interface = EnttecOpenDMX::new().unwrap();
        interface.open().unwrap();
        interface.set_buffer(temp_buffer);
        interface.render().unwrap();
        interface.close().unwrap();
        self.buffer = buffer;
    }

    #[allow(dead_code)]
    pub fn set_channel(&mut self, channel_number: usize, value: u8) -> Result<(), &'static str> {
        if channel_number <= BUFFER_SIZE {
            let mut buffer = self.buffer.clone();
            buffer[channel_number - 1] = value;
            self.set_buffer(buffer);
            Ok(())
        } else {
            Err("Channel number exceeds DMX universe")
        }
    }

    #[allow(dead_code)]
    pub fn full_bright(&mut self) -> Result<(), String> {
        let buffer = [255; BUFFER_SIZE];
        self.set_buffer(buffer);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn blackout(&mut self) -> Result<(), String> {
        let buffer = [0; BUFFER_SIZE];
        self.set_buffer(buffer);
        Ok(())
    }
}

impl Default for LuxState {
    fn default() -> Self {
        let buffer: [u8; BUFFER_SIZE] = [121, 255, 255, 0, 0, 85];

        let channels: [LuxChannel; BUFFER_SIZE] = [
            LuxChannel {
                disabled: false,
                channel_number: 1,
                label: "Red".to_owned(),
                label_color: LuxLabelColor::Red,
            },
            LuxChannel {
                disabled: false,
                channel_number: 2,
                label: "Green".to_owned(),
                label_color: LuxLabelColor::Green,
            },
            LuxChannel {
                disabled: false,
                channel_number: 3,
                label: "Blue".to_owned(),
                label_color: LuxLabelColor::Blue,
            },
            LuxChannel {
                disabled: false,
                channel_number: 4,
                label: "Amber".to_owned(),
                label_color: LuxLabelColor::Amber,
            },
            LuxChannel {
                disabled: false,
                channel_number: 5,
                label: "White".to_owned(),
                label_color: LuxLabelColor::White,
            },
            LuxChannel {
                disabled: false,
                channel_number: 6,
                label: "Brightness".to_owned(),
                label_color: LuxLabelColor::Brightness,
            },
        ];

        Self { buffer, channels }
    }
}

#[allow(dead_code)]
pub fn lux_state() -> Arc<Mutex<LuxState>> {
    Arc::new(Mutex::new(LuxState::default()))
}
