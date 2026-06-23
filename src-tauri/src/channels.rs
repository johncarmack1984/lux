use crate::buffer::BUFFER_SIZE;
use crate::channel::{Channel, LuxChannel};
use crate::cmd::CmdEvent;
use crate::colors::LuxLabelColor;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::Runtime;

pub type Channels = [LuxChannel; BUFFER_SIZE];

#[derive(Debug, Deserialize, Serialize, Clone, Type)]
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
    pub fn get_all(&self) -> LuxChannels {
        let channels = self.channels.lock().unwrap().clone();
        LuxChannels::from(channels)
    }

    /// Write `new_metadata` into the (1-based) `channel_number` slot in place and
    /// return it. Split out from `set` so the persistence logic can be unit-tested
    /// without a Tauri `AppHandle`.
    pub fn put(
        &self,
        channel_number: usize,
        new_metadata: LuxChannel,
    ) -> Result<LuxChannel, String> {
        let index = channel_number
            .checked_sub(1)
            .filter(|i| *i < BUFFER_SIZE)
            .ok_or_else(|| {
                format!("channel {channel_number} out of range (expected 1..={BUFFER_SIZE})")
            })?;
        self.channels.lock().unwrap()[index] = new_metadata.clone();
        Ok(new_metadata)
    }

    pub fn set<R: Runtime>(
        &mut self,
        channel_number: usize,
        new_metadata: LuxChannel,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxChannel, String> {
        let channel = self.put(channel_number, new_metadata)?;
        CmdEvent::ChannelDataSet {
            channels: self.channels.lock().unwrap().clone(),
        }
        .emit(&app)
        .map_err(|e| format!("Failed to emit channel_data_set event: {}", e))?;
        Ok(channel)
    }

    pub fn set_all(&mut self, channels: Channels) -> Result<LuxChannels, String> {
        log::trace!("set_all");
        let mut locked_channels = self.channels.lock().unwrap();
        *locked_channels = channels.clone();
        Ok(locked_channels.into())
    }

    pub fn set_channels<R: Runtime>(
        &mut self,
        channels: LuxChannels,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxChannels, String> {
        self.set_all(channels.into())?;

        CmdEvent::ChannelDataSet {
            channels: self.channels.lock().unwrap().clone(),
        }
        .emit(&app)
        .map_err(|e| format!("Failed to emit channel_data_set event: {}", e))?;
        Ok(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::Channel;
    use crate::colors::LuxLabelColor;

    fn sample(channel_number: u32, label: &str) -> LuxChannel {
        LuxChannel::from(Channel {
            id: uuid::Uuid::new_v4(),
            disabled: false,
            channel_number,
            label: label.to_owned(),
            label_color: LuxLabelColor::Red,
        })
    }

    #[test]
    fn put_persists_metadata_at_one_based_channel() {
        let channels = LuxChannels::default();

        let returned = channels.put(3, sample(42, "Cyan")).unwrap();
        assert_eq!(returned.get_channel_number(), 42);

        // 1-based channel 3 lives at array index 2, and must actually be written.
        let stored = channels.channels.lock().unwrap()[2].clone();
        assert_eq!(stored.get_channel_number(), 42);

        // neighbouring channels are untouched (default channel 4 keeps number 4).
        let neighbour = channels.channels.lock().unwrap()[3].clone();
        assert_eq!(neighbour.get_channel_number(), 4);
    }

    #[test]
    fn put_rejects_out_of_range_channels() {
        let channels = LuxChannels::default();
        assert!(channels.put(0, sample(1, "x")).is_err());
        assert!(channels.put(BUFFER_SIZE + 1, sample(1, "x")).is_err());
    }
}
