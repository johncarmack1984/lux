use crate::buffer::BUFFER_SIZE;
use crate::colors::LuxLabelColor;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxChannelData {
    pub id: uuid::Uuid,
    pub disabled: bool,
    pub channel_number: usize,
    pub label: String,
    pub label_color: LuxLabelColor,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxChannel(Arc<Mutex<LuxChannelData>>);

impl LuxChannel {
    pub fn new(data: LuxChannelData) -> Self {
        LuxChannel(Arc::new(Mutex::new(data)))
    }

    pub fn get(&self) -> LuxChannelData {
        let data = self.0.lock().unwrap();
        (*data).clone()
    }

    pub fn _set(&self, data: LuxChannelData) {
        let mut locked_data = self.0.lock().unwrap();
        *locked_data = data;
    }

    pub fn _disable(&self, disabled: bool) {
        let mut data = self.0.lock().unwrap();
        data.disabled = disabled;
    }
}

impl Default for LuxChannel {
    fn default() -> Self {
        LuxChannel::new(LuxChannelData {
            id: uuid::Uuid::new_v4(),
            disabled: false,
            channel_number: 0,
            label: String::new(),
            label_color: LuxLabelColor::Brightness,
        })
    }
}

impl From<LuxChannelData> for LuxChannel {
    fn from(value: LuxChannelData) -> Self {
        LuxChannel(Arc::new(Mutex::new(value)))
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxChannels(Arc<Mutex<[LuxChannel; BUFFER_SIZE]>>);

impl From<[LuxChannelData; BUFFER_SIZE]> for LuxChannels {
    fn from(value: [LuxChannelData; BUFFER_SIZE]) -> Self {
        let channels = array_init::array_init(|i| LuxChannel::from(value[i].clone()));
        LuxChannels(Arc::new(Mutex::new(channels)))
    }
}

impl From<[LuxChannel; BUFFER_SIZE]> for LuxChannels {
    fn from(value: [LuxChannel; BUFFER_SIZE]) -> Self {
        LuxChannels(Arc::new(Mutex::new(value)))
    }
}

impl LuxChannels {
    pub fn get(&self) -> [LuxChannel; BUFFER_SIZE] {
        let channels = self.0.lock().unwrap();
        channels.clone()
    }

    pub fn set(&mut self, channels: &[LuxChannel; BUFFER_SIZE]) {
        let mut locked_channels = self.0.lock().unwrap();
        *locked_channels = channels.clone();
    }

    pub fn get_by_id(&self, id: uuid::Uuid) -> Option<LuxChannel> {
        let channels = self.get();
        for channel in channels.iter() {
            if channel.get().id == id {
                return Some(channel.clone());
            }
        }
        None
    }

    pub fn _get_by_channel_number(&self, channel_number: usize) -> Option<LuxChannel> {
        let channels = self.get();
        for channel in channels.iter() {
            if channel.get().channel_number == channel_number {
                return Some(channel.clone());
            }
        }
        None
    }
}
