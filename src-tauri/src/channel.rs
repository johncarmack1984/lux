use std::sync::{Arc, Mutex};

use crate::colors::LuxLabelColor;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Channel {
    pub id: uuid::Uuid,
    pub disabled: bool,
    pub channel_number: usize,
    pub label: String,
    pub label_color: LuxLabelColor,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxChannel(Arc<Mutex<Channel>>);

impl LuxChannel {
    pub fn new(data: Channel) -> Self {
        LuxChannel(Arc::new(Mutex::new(data)))
    }

    pub fn get(&self) -> LuxChannel {
        let data = self;
        (*data).clone()
    }

    // pub fn _set(&self, data: LuxChannelData) {
    //     let mut locked_data = self.lock().unwrap();
    //     *locked_data = data;
    // }

    pub fn toogle_disabled(&mut self, disabled: bool) {
        let mut locked_data = self.0.lock().unwrap();
        locked_data.disabled = disabled;
    }
}

impl Default for LuxChannel {
    fn default() -> Self {
        LuxChannel(Arc::new(Mutex::new(Channel {
            id: uuid::Uuid::new_v4(),
            disabled: false,
            channel_number: 0,
            label: String::new(),
            label_color: LuxLabelColor::Brightness,
        })))
    }
}

impl From<Channel> for LuxChannel {
    fn from(value: Channel) -> Self {
        LuxChannel(Arc::new(Mutex::new(value)))
    }
}
