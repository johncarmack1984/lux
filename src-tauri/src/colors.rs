use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumProperty};

#[derive(Debug, Deserialize, Serialize, Clone, AsRefStr, EnumIter, EnumProperty)]
pub enum LuxLabelColor {
    Red,
    Green,
    Blue,
    Amber,
    White,
    Brightness,
}
