use crate::buffer::BUFFER_SIZE;
use crate::channel::{Channel, LuxChannel};
use crate::colors::LuxLabelColor;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::Emitter;

pub type Channels = [LuxChannel; BUFFER_SIZE];

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LuxChannels {
    pub channels: Arc<Mutex<Channels>>,
}

impl From<LuxChannels> for Channels {
    fn from(value: LuxChannels) -> Self {
        value.channels.lock().unwrap().to_owned()
    }
}

impl From<&LuxChannels> for Channels {
    fn from(value: &LuxChannels) -> Self {
        value.channels.lock().unwrap().to_owned()
    }
}

impl From<[LuxChannel; BUFFER_SIZE]> for LuxChannels {
    fn from(value: [LuxChannel; BUFFER_SIZE]) -> Self {
        let channels = array_init::array_init(|i| LuxChannel::from(value[i].clone()));
        LuxChannels {
            channels: Arc::new(Mutex::new(channels)),
        }
    }
}

impl From<[Channel; BUFFER_SIZE]> for LuxChannels {
    fn from(value: [Channel; BUFFER_SIZE]) -> Self {
        let channels = array_init::array_init(|i| LuxChannel::from(value[i].clone()));
        LuxChannels {
            channels: Arc::new(Mutex::new(channels)),
        }
    }
}

impl From<&LuxChannels> for LuxChannels {
    fn from(value: &LuxChannels) -> Self {
        let channels = value.channels.lock().unwrap().to_owned();
        LuxChannels {
            channels: Arc::new(Mutex::new(channels)),
        }
    }
}

impl Default for LuxChannels {
    fn default() -> Self {
        let channels: LuxChannels = LuxChannels::from([
            Channel {
                id: uuid::Uuid::new_v4(),
                disabled: false,
                channel_number: 1,
                label: "Red".to_owned(),
                label_color: LuxLabelColor::Red,
            },
            Channel {
                id: uuid::Uuid::new_v4(),
                disabled: false,
                channel_number: 2,
                label: "Green".to_owned(),
                label_color: LuxLabelColor::Green,
            },
            Channel {
                id: uuid::Uuid::new_v4(),
                disabled: false,
                channel_number: 3,
                label: "Blue".to_owned(),
                label_color: LuxLabelColor::Blue,
            },
            Channel {
                id: uuid::Uuid::new_v4(),
                disabled: false,
                channel_number: 4,
                label: "Amber".to_owned(),
                label_color: LuxLabelColor::Amber,
            },
            Channel {
                id: uuid::Uuid::new_v4(),
                disabled: false,
                channel_number: 5,
                label: "White".to_owned(),
                label_color: LuxLabelColor::White,
            },
            Channel {
                id: uuid::Uuid::new_v4(),
                disabled: false,
                channel_number: 6,
                label: "Brightness".to_owned(),
                label_color: LuxLabelColor::Brightness,
            },
        ]);

        LuxChannels::from(channels)
    }
}

impl Display for LuxChannels {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.channels.lock().unwrap())
    }
}

impl From<MutexGuard<'_, [LuxChannel; BUFFER_SIZE]>> for LuxChannels {
    fn from(value: MutexGuard<'_, [LuxChannel; BUFFER_SIZE]>) -> Self {
        LuxChannels {
            channels: Arc::new(Mutex::new(value.to_owned())),
        }
    }
}

impl From<Arc<Mutex<[LuxChannel; BUFFER_SIZE]>>> for LuxChannels {
    fn from(value: Arc<Mutex<[LuxChannel; BUFFER_SIZE]>>) -> Self {
        LuxChannels { channels: value }
    }
}

impl LuxChannels {
    pub fn get(&self, channel_number: usize) -> Option<LuxChannel> {
        self.channels.lock().unwrap().get(channel_number).cloned()
    }

    pub fn get_all(&self) -> LuxChannels {
        let channels = self.channels.lock().unwrap().clone();
        LuxChannels::from(channels)
    }

    pub fn set(
        &mut self,
        channel_number: usize,
        new_metadata: LuxChannel,
        app: tauri::AppHandle,
    ) -> Result<LuxChannel, String> {
        let channel = self
            .get(channel_number)
            .insert(LuxChannel::from(new_metadata))
            .to_owned();
        app.emit("channel_data_set", self.channels.clone()).unwrap();
        Ok(channel)
    }

    pub fn set_all(&mut self, channels: Channels) -> Result<LuxChannels, String> {
        log::trace!("set_all");
        let mut locked_channels = self.channels.lock().unwrap();
        *locked_channels = channels.clone();
        Ok(locked_channels.into())
    }

    pub fn set_channels(
        &mut self,
        channels: LuxChannels,
        app: tauri::AppHandle,
    ) -> Result<LuxChannels, String> {
        self.set_all(channels.into())?;

        app.emit("channel_data_set", self.channels.clone()).unwrap();
        Ok(self.clone())
    }
}
