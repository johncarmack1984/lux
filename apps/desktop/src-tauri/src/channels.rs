use crate::lock::LockPolicy;
use crate::buffer::UNIVERSE_SIZE;
use crate::channel::{Channel, LuxChannel};
use crate::cmd::CmdEvent;
use crate::colors::LuxLabelColor;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, Mutex};
use tauri::Runtime;

pub type Channels = Vec<LuxChannel>;

#[derive(Debug, Deserialize, Serialize, Clone, Type)]
pub struct LuxChannels {
    pub channels: Arc<Mutex<Channels>>,
}

impl From<LuxChannels> for Channels {
    fn from(value: LuxChannels) -> Self {
        value.channels.lock_or_recover().clone()
    }
}

impl From<&LuxChannels> for Channels {
    fn from(value: &LuxChannels) -> Self {
        value.channels.lock_or_recover().clone()
    }
}

impl From<Vec<LuxChannel>> for LuxChannels {
    fn from(value: Vec<LuxChannel>) -> Self {
        LuxChannels {
            channels: Arc::new(Mutex::new(value)),
        }
    }
}

impl From<Vec<Channel>> for LuxChannels {
    fn from(value: Vec<Channel>) -> Self {
        let channels = value.into_iter().map(LuxChannel::from).collect();
        LuxChannels {
            channels: Arc::new(Mutex::new(channels)),
        }
    }
}

impl From<&LuxChannels> for LuxChannels {
    fn from(value: &LuxChannels) -> Self {
        let channels = value.channels.lock_or_recover().clone();
        LuxChannels {
            channels: Arc::new(Mutex::new(channels)),
        }
    }
}

impl Default for LuxChannels {
    fn default() -> Self {
        // Slots 1..=6 are the labelled RGBAW fixture; 7..=512 are raw universe
        // channels with generic "CH N" labels.
        let rgbaw = [
            ("Red", LuxLabelColor::Red),
            ("Green", LuxLabelColor::Green),
            ("Blue", LuxLabelColor::Blue),
            ("Amber", LuxLabelColor::Amber),
            ("White", LuxLabelColor::White),
            ("Brightness", LuxLabelColor::Brightness),
        ];
        let channels: Vec<Channel> = (1..=UNIVERSE_SIZE as u32)
            .map(|channel_number| {
                let (label, label_color) = rgbaw
                    .get((channel_number - 1) as usize)
                    .map(|(label, color)| ((*label).to_owned(), *color))
                    .unwrap_or_else(|| (format!("CH {channel_number}"), LuxLabelColor::Generic));
                Channel {
                    id: uuid::Uuid::new_v4(),
                    disabled: false,
                    channel_number,
                    label,
                    label_color,
                }
            })
            .collect();
        LuxChannels::from(channels)
    }
}

impl Display for LuxChannels {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.channels.lock_or_recover())
    }
}

impl LuxChannels {
    pub fn get_all(&self) -> LuxChannels {
        let channels = self.channels.lock_or_recover().clone();
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
        let mut channels = self.channels.lock_or_recover();
        let len = channels.len();
        let index = channel_number
            .checked_sub(1)
            .filter(|i| *i < len)
            .ok_or_else(|| format!("channel {channel_number} out of range (expected 1..={len})"))?;
        channels[index] = new_metadata.clone();
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
            channels: self.channels.lock_or_recover().clone(),
        }
        .emit(&app)
        .map_err(|e| format!("Failed to emit channel_data_set event: {}", e))?;
        Ok(channel)
    }

    pub fn set_all(&mut self, channels: Channels) -> Result<LuxChannels, String> {
        log::trace!("set_all");
        *self.channels.lock_or_recover() = channels.clone();
        Ok(LuxChannels::from(channels))
    }

    pub fn set_channels<R: Runtime>(
        &mut self,
        channels: LuxChannels,
        app: tauri::AppHandle<R>,
    ) -> Result<LuxChannels, String> {
        self.set_all(channels.into())?;

        CmdEvent::ChannelDataSet {
            channels: self.channels.lock_or_recover().clone(),
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
    fn default_spans_the_full_universe() {
        let channels = LuxChannels::default();
        let locked = channels.channels.lock_or_recover();
        assert_eq!(locked.len(), UNIVERSE_SIZE);
        // Slot 1 is the labelled Red channel, slot 7 is a generic raw channel.
        assert_eq!(locked[0].get_channel_number(), 1);
        assert_eq!(locked[6].get_channel_number(), 7);
    }

    #[test]
    fn put_persists_metadata_at_one_based_channel() {
        let channels = LuxChannels::default();

        let returned = channels.put(3, sample(42, "Cyan")).unwrap();
        assert_eq!(returned.get_channel_number(), 42);

        // 1-based channel 3 lives at array index 2, and must actually be written.
        let stored = channels.channels.lock_or_recover()[2].clone();
        assert_eq!(stored.get_channel_number(), 42);

        // neighbouring channels are untouched (default channel 4 keeps number 4).
        let neighbour = channels.channels.lock_or_recover()[3].clone();
        assert_eq!(neighbour.get_channel_number(), 4);
    }

    #[test]
    fn put_rejects_out_of_range_channels() {
        let channels = LuxChannels::default();
        assert!(channels.put(0, sample(1, "x")).is_err());
        assert!(channels.put(UNIVERSE_SIZE + 1, sample(1, "x")).is_err());
    }
}
