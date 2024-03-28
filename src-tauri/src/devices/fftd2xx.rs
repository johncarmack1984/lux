// use libftd2xx::Ftdi;

// use serde::{Deserialize, Serialize};

// #[derive(Debug, Serialize)]
// #[serde(remote = "Ftdi")]
// pub struct FtdiDef {
//     // #[serde(getter = "Ftdi::handle")]
//     // handle: *mut ::std::os::raw::c_void,
// }

// // Provide a conversion to construct the remote type.
// impl From<FtdiDef> for Ftdi {
//     fn from(def: FtdiDef) -> Ftdi {
//         Ftdi::from(def)
//     }
// }

// #[derive(Debug, Serialize)]
// #[serde(remote = "DeviceInfo")]
// pub struct DeviceInfoDef {}

// impl From<DeviceInfoDef> for DeviceInfo {
//     fn from(def: DeviceInfoDef) -> DeviceInfo {
//         DeviceInfo::from(def)
//     }
// }

// #[derive(Debug, Serialize)]
// #[serde(remote = "DeviceStatus")]
// pub struct DeviceStatusDef {
//     ammount_in_rx_queue: u32,
//     ammount_in_tx_queue: u32,
//     event_status: u32,
// }

// impl From<DeviceStatusDef> for DeviceStatus {
//     fn from(def: DeviceStatusDef) -> DeviceStatus {
//         DeviceStatus::from(def)
//     }
// }
