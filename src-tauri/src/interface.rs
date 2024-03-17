#[tauri::command]
pub fn get_lux_buffer() -> Vec<u8> {
    let mut interface = lux::LuxDMX::new().unwrap();
    interface.open().unwrap();
    interface.render().unwrap();
    let buffer = interface.get_buffer().to_vec();
    interface.close().unwrap();
    buffer
}
