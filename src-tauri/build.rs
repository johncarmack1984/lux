fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .codegen(tauri_build::CodegenContext::new())
            .app_manifest(tauri_build::AppManifest::new().commands(&[
                "update_channel_value",
                "update_channel_metadata",
                "set_buffer",
                "sync_channels",
                "sync_state",
                "get_initial_state",
                "delete_channels",
                "insert_channel",
            ])),
    )
    .expect("failed to run tauri-build");
}
