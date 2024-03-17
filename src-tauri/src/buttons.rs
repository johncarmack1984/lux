use enttecopendmx;

#[tauri::command]
pub fn full_bright() {
    let mut interface = enttecopendmx::EnttecOpenDMX::new().unwrap();
    interface.open().unwrap();
    interface.set_channel(1, 255);
    interface.set_channel(2, 255);
    interface.set_channel(3, 255);
    interface.set_channel(4, 255);
    interface.set_channel(5, 255);
    interface.set_channel(6, 255);
    interface.render().unwrap();
    interface.close().unwrap();
}

#[tauri::command]
pub fn rgb_chase() {
    use std::thread;
    use std::time::Duration;
    const SLEEPTIME: u64 = 100;
    let mut interface = enttecopendmx::EnttecOpenDMX::new().unwrap();
    interface.open().unwrap();
    interface.set_channel(6, 255);
    loop {
        for i in 1..4 {
            interface.set_channel(i as usize, 255 as u8);
            // interface.buffer[1] = interface.buffer[1] + 10;
            interface.render().unwrap();
            interface.set_channel(i as usize, 0 as u8);
            thread::sleep(Duration::from_millis(SLEEPTIME));
        }
    }
}

#[tauri::command]
pub fn blackout() {
    let mut interface = enttecopendmx::EnttecOpenDMX::new().unwrap();
    interface.open().unwrap();
    interface.blackout();
    interface.render().unwrap();
    interface.close().unwrap();
}
