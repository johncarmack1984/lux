use crate::{
    buffer::{LuxBuffer, BUFFER_SIZE},
    channel::{
        // LuxChannel,
        LuxChannelData,
        LuxChannels,
    },
    colors::LuxLabelColor,
    devices::enttec_open_dmx_usb::EnttecOpenDMX,
};
use libftd2xx::FtStatus;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    sync::{Arc, Mutex},
};
use tauri::Manager;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxState {
    pub buffer: LuxBuffer,
    pub channels: LuxChannels,
}

impl LuxState {
    pub fn set_buffer(
        &mut self,
        buffer: LuxBuffer,
        app: tauri::AppHandle,
    ) -> Result<LuxBuffer, FtStatus> {
        let mut temp_buffer = [0; 513];

        temp_buffer[1..BUFFER_SIZE + 1].copy_from_slice(&buffer.get());

        let mut interface = match EnttecOpenDMX::new() {
            Ok(interface) => interface,
            Err(e) => return Err(e),
        };

        interface.open()?;

        interface.set_buffer(temp_buffer);

        interface.render()?;
        interface.close()?;

        self.buffer.set(buffer.get());

        app.emit("buffer_set", buffer).unwrap();

        Ok(self.buffer.clone())
    }

    pub fn set_channel_value(
        &mut self,
        channel_number: usize,
        value: u8,
        app: tauri::AppHandle,
    ) -> Result<LuxBuffer, FtStatus> {
        let mut buffer = self.buffer.get();
        buffer[channel_number - 1] = value;
        self.set_buffer(LuxBuffer::from(buffer), app)
    }

    pub fn _set_channel_label(
        &mut self,
        channel_number: usize,
        label: String,
        app: tauri::AppHandle,
    ) -> Result<String, String> {
        let mut channel = self.channels.get()[channel_number - 1].get();
        channel.label = label;
        self.channels.get()[channel_number - 1].set(channel);
        app.emit("channel_data_set", self.channels.clone()).unwrap();

        Ok(self.channels.get()[channel_number - 1].get().label.clone())
    }

    pub fn set_channels(
        &mut self,
        channels: LuxChannels,
        app: tauri::AppHandle,
    ) -> Result<LuxChannels, String> {
        self.channels.set(&channels.get());
        app.emit("channel_data_set", self.channels.clone()).unwrap();

        Ok(self.channels.clone())
    }

    pub fn mutex(&self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self.clone()))
    }
}

impl Display for LuxState {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.buffer)
    }
}

impl Default for LuxState {
    fn default() -> Self {
        let buffer: LuxBuffer = LuxBuffer::default();

        let channels: LuxChannels = LuxChannels::from([
            LuxChannelData {
                disabled: false,
                channel_number: 1,
                label: "Red".to_owned(),
                label_color: LuxLabelColor::Red,
            },
            LuxChannelData {
                disabled: false,
                channel_number: 2,
                label: "Green".to_owned(),
                label_color: LuxLabelColor::Green,
            },
            LuxChannelData {
                disabled: false,
                channel_number: 3,
                label: "Blue".to_owned(),
                label_color: LuxLabelColor::Blue,
            },
            LuxChannelData {
                disabled: false,
                channel_number: 4,
                label: "Amber".to_owned(),
                label_color: LuxLabelColor::Amber,
            },
            LuxChannelData {
                disabled: false,
                channel_number: 5,
                label: "White".to_owned(),
                label_color: LuxLabelColor::White,
            },
            LuxChannelData {
                disabled: false,
                channel_number: 6,
                label: "Brightness".to_owned(),
                label_color: LuxLabelColor::Brightness,
            },
        ]);
        Self { buffer, channels }
    }
}
