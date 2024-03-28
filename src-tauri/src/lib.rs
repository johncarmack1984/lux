use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use strum::{AsRefStr, EnumIter, EnumProperty, IntoEnumIterator};
use tauri_plugin_log::LogTarget;

// mod devices;
// use enttecopendmx::EnttecOpenDMX;
// use crate::devices::enttec_open_dmx_usb::EnttecOpenDMX;

mod devices;
use crate::devices::enttec_open_dmx_usb::EnttecOpenDMX;

pub const BUFFER_SIZE: usize = 6;

#[derive(Debug, Deserialize, Serialize, Clone, AsRefStr, EnumIter, EnumProperty)]
pub enum LuxLabelColor {
    Red,
    Green,
    Blue,
    Amber,
    White,
    Brightness,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxChannel {
    pub disabled: bool,
    pub channel_number: usize,
    pub label: String,
    pub label_color: LuxLabelColor,
    pub value: u8,
}

impl LuxChannel {
    pub fn set_disabled(&mut self, disabled: bool) {
        self.disabled = disabled;
    }

    pub fn set_channel_number(&mut self, channel_number: usize) -> Result<(), &'static str> {
        if channel_number <= BUFFER_SIZE {
            self.channel_number = channel_number;
            Ok(())
        } else {
            Err("Index exceeds DMX universe")
        }
    }

    pub fn set_label(&mut self, label: &str) {
        self.label = label.to_owned().to_string();
    }

    pub fn set_label_color(&mut self, label_color: LuxLabelColor) {
        self.label_color = label_color;
    }

    pub fn set_value(&mut self, value: u8) -> Result<(), &'static str> {
        self.value = value;
        Ok(())
    }
}

impl Default for LuxChannel {
    fn default() -> Self {
        Self {
            disabled: false,
            channel_number: 0,
            label: String::new(),
            label_color: LuxLabelColor::Brightness,
            value: 0,
        }
    }
}

// #[derive(Debug, Clone)]
// struct Devices([EnttecOpenDMX; 1]);

// impl Devices {
//     fn new() -> Self {
//         Self([EnttecOpenDMX::new().unwrap()])
//     }
// }

// impl Default for Devices {
//     fn default() -> Self {
//         Self::new()
//     }
// }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxState {
    pub buffer: [u8; BUFFER_SIZE],
    pub channels: [LuxChannel; BUFFER_SIZE],
    // #[serde(skip)]
    // pub device: EnttecOpenDMX,
}

impl LuxState {
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

    pub fn set_channel(&mut self, channel_number: usize, value: u8) -> Result<(), &'static str> {
        if channel_number <= BUFFER_SIZE {
            self.channels[channel_number - 1].value = value;
            let mut buffer = self.buffer.clone();
            buffer[channel_number - 1] = value;
            self.set_buffer(buffer);
            Ok(())
        } else {
            Err("Channel number exceeds DMX universe")
        }
    }

    pub fn full_bright(&mut self) -> Result<(), String> {
        let buffer = [255; BUFFER_SIZE];
        self.set_buffer(buffer);
        let channels_vec: Vec<LuxChannel> = self
            .channels
            .iter()
            .map(|c| {
                let mut new_c = c.clone();
                new_c.value = 255;
                new_c
            })
            .collect();
        self.channels = channels_vec.try_into().unwrap();
        Ok(())
    }

    pub fn blackout(&mut self) -> Result<(), String> {
        let buffer = [0; BUFFER_SIZE];
        self.set_buffer(buffer);
        let channels_vec: Vec<LuxChannel> = self
            .channels
            .iter()
            .map(|c| {
                let mut new_c = c.clone();
                new_c.value = 0;
                new_c
            })
            .collect();
        self.channels = channels_vec.try_into().unwrap();
        Ok(())
    }
}

impl Default for LuxState {
    fn default() -> Self {
        let buffer = [0; BUFFER_SIZE];
        let label_colors: Vec<LuxLabelColor> = LuxLabelColor::iter().collect();
        let mut channels = Vec::with_capacity(BUFFER_SIZE);
        for i in 1..BUFFER_SIZE + 1 {
            let mut channel = LuxChannel::default();
            channel.set_channel_number(i).unwrap();
            let label_color = LuxLabelColor::iter()
                .nth((i - 1) % label_colors.len())
                .unwrap();
            channel.set_label(label_color.as_ref());
            channel.set_label_color(label_color);
            channels.push(channel.to_owned());
        }
        let channels: Result<[LuxChannel; 6], _> = channels.try_into();
        // let device = EnttecOpenDMX::new().unwrap();
        Self {
            buffer,
            channels: channels.unwrap(),
            // device,
        }
    }
}

pub fn lux_state() -> Arc<Mutex<LuxState>> {
    Arc::new(Mutex::new(LuxState::default()))
}

pub fn logger() -> tauri_plugin_log::Builder {
    tauri_plugin_log::Builder::default()
        .targets([
            LogTarget::LogDir,
            LogTarget::Stdout,
            LogTarget::Webview,
            LogTarget::Stderr,
        ])
        .level(log::LevelFilter::Info)
}
