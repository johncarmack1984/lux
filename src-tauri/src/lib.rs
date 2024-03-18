use serde::Serialize;
use std::sync::{Arc, Mutex};
use strum::{AsRefStr, EnumIter, EnumProperty, IntoEnumIterator};
use tauri_plugin_log::LogTarget;

pub const BUFFER_SIZE: usize = 6;

#[derive(Debug, Serialize, Clone, AsRefStr, EnumIter, EnumProperty)]
pub enum LuxLabelColor {
    Red,
    Green,
    Blue,
    Amber,
    White,
    Brightness,
}

#[derive(Debug, Serialize, Clone)]
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

#[derive(Debug, Serialize, Clone)]
pub struct LuxState {
    pub buffer: [u8; BUFFER_SIZE],
    pub channels: [LuxChannel; BUFFER_SIZE],
}

impl LuxState {
    pub fn set_buffer(&mut self, buffer: [u8; BUFFER_SIZE]) {
        self.buffer = buffer;
    }

    pub fn set_channel(&mut self, channel_number: usize, value: u8) -> Result<(), &'static str> {
        if channel_number <= BUFFER_SIZE {
            self.channels[channel_number - 1].value = value;
            Ok(())
        } else {
            Err("Channel number exceeds DMX universe")
        }
    }

    pub fn set_channels(&mut self, channels: [LuxChannel; BUFFER_SIZE]) {
        self.channels = channels;
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
        Self {
            buffer,
            channels: channels.unwrap(),
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
