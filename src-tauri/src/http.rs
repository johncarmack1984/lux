use tauri::{App, Manager};

use crate::buffer::{Buffer, LuxBuffer};

pub fn setup_http(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();
    #[cfg(desktop)]
    std::thread::spawn(|| {
        let server = httpd("localhost:3003");
        listen_http(server, app_handle);
    });
    Ok(())
}

pub fn httpd(addr: &str) -> tiny_http::Server {
    match tiny_http::Server::http(addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

#[derive(serde::Deserialize)]
struct IncomingPayload {
    buffer: Vec<u8>,
}

fn deserialize_buffer(incoming_buffer: Vec<u8>) -> Result<LuxBuffer, String> {
    match serde_json::from_slice::<IncomingPayload>(&incoming_buffer) {
        Ok(deserialized) => {
            log::trace!("Deserialized buffer: {:?}", deserialized.buffer);
            Ok(LuxBuffer::from(deserialized.buffer))
        }
        Err(e) => {
            let msg = format!("Failed to deserialize incoming buffer: {:?}", e);
            log::error!("{}", msg);
            Err(msg)
        }
    }
}

pub fn listen_http(server: tiny_http::Server, app: tauri::AppHandle) {
    loop {
        if let Ok(mut request) = server.recv() {
            log::trace!("incoming api request: {:?}", request);
            let mut incoming_buffer = Vec::new();
            request
                .as_reader()
                .read_to_end(&mut incoming_buffer)
                .unwrap();
            let deserialized_buffer: Result<LuxBuffer, String> =
                deserialize_buffer(incoming_buffer.clone());
            let buffer = deserialized_buffer.unwrap();
            app.emit("incoming_api_request", buffer.clone()).unwrap();
            let mut state = app.state::<LuxBuffer>().get();
            state.set(Buffer::from(buffer), app.clone()).unwrap();

            let response = tiny_http::Response::new(
                tiny_http::StatusCode(200),
                request.headers().to_vec(),
                std::io::Cursor::new(incoming_buffer),
                request.body_length(),
                None,
            );
            request.respond(response).unwrap()
        }
    }
}
