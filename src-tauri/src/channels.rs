use crate::buffer::BUFFER_SIZE;
use serde::{Deserialize, Serialize};

use crate::colors::LuxLabelColor;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxChannel {
    pub disabled: bool,
    pub channel_number: usize,
    pub label: String,
    pub label_color: LuxLabelColor,
}

impl LuxChannel {
    pub fn _set_disabled(&mut self, disabled: bool) {
        self.disabled = disabled;
    }

    pub fn _set_channel_number(&mut self, channel_number: usize) -> Result<(), &'static str> {
        if channel_number <= BUFFER_SIZE {
            self.channel_number = channel_number;
            Ok(())
        } else {
            Err("Index exceeds DMX universe")
        }
    }

    pub fn _set_label(&mut self, label: &str) {
        self.label = label.to_owned().to_string();
    }

    pub fn _set_label_color(&mut self, label_color: LuxLabelColor) {
        self.label_color = label_color;
    }

    pub fn _value(&self, buffer: &[u8]) -> u8 {
        buffer[self.channel_number - 1]
    }

    pub fn _set_value(&mut self, buffer: &mut [u8], value: u8) -> Result<(), &'static str> {
        buffer[self.channel_number - 1] = value;
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
        }
    }
}
