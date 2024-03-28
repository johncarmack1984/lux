fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .codegen(tauri_build::CodegenContext::new())
            .plugin(
                "app-menu",
                tauri_build::InlinedPlugin::new().commands(&["toggle", "popup"]),
            )
            .app_manifest(
                tauri_build::AppManifest::new().commands(&["log_operation", "perform_request"]),
            ),
    )
    .expect("failed to run tauri-build");
}
