use std::str::FromStr;

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumProperty};

#[derive(Debug, Deserialize, Serialize, Copy, Clone, AsRefStr, EnumIter, EnumProperty)]
pub enum LuxLabelColor {
    Red,
    Green,
    Blue,
    Amber,
    White,
    Brightness,
}

impl FromStr for LuxLabelColor {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Red" => Ok(LuxLabelColor::Red),
            "Green" => Ok(LuxLabelColor::Green),
            "Blue" => Ok(LuxLabelColor::Blue),
            "Amber" => Ok(LuxLabelColor::Amber),
            "White" => Ok(LuxLabelColor::White),
            "Brightness" => Ok(LuxLabelColor::Brightness),
            _ => Err("Invalid color"),
        }
    }
}
