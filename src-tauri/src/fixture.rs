#[tauri::command]
pub fn slider(channel: usize, value: u8) -> u8 {
    let mut interface = lux::LuxDMX::new().unwrap();
    interface.open().unwrap();
    interface.set_channel(1, 255);
    interface.set_channel(2, 255);
    interface.set_channel(3, 255);
    interface.set_channel(4, 255);
    interface.set_channel(5, 255);
    interface.set_channel(channel, value);
    interface.render().unwrap();
    interface.close().unwrap();
    value
}
