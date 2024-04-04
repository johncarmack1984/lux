// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// #![allow(non_snake_case)]

// mod api;
// mod buffer;
// mod channel;
// mod colors;
// mod devices;
mod logger;
// mod state;
// mod sync;

use specta::{ts, Type};

#[derive(Type)]
pub struct TypeOne {
    pub a: String,
    pub b: GenericType<i32>,
    #[serde(rename = "cccccc")]
    pub c: MyEnum,
}

#[derive(Type)]
pub struct GenericType<A> {
    pub my_field: String,
    pub generic: A,
}

#[derive(Type)]
pub enum MyEnum {
    A,
    B,
    C,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    assert_eq!(
        ts::export::<TypeOne>(&Default::default()).unwrap(),
        "export type TypeOne = { a: string; b: GenericType<number>; cccccc: MyEnum }".to_string()
    );
    tauri::Builder::default()
        // .manage(state::LuxState::default().mutex())
        // .invoke_handler(api::router().into_handler())
        .plugin(logger::logger().build())
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
