fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .codegen(tauri_build::CodegenContext::new())
            .app_manifest(tauri_build::AppManifest::new().commands(&[
                "update_channel_value",
                "set_buffer",
                "sync_channels",
                "sync_state",
            ])),
    )
    .expect("failed to run tauri-build");
}
