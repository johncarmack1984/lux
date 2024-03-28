use serde::{Deserialize, Serialize};

pub const BUFFER_SIZE: usize = 6;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LuxBuffer(pub [u8; BUFFER_SIZE]);
