fn main() {
    const COMMANDS: &[&str] = &[
        "config_load",
        "config_save",
        "history_load",
        "history_append",
        "history_clear",
        "completion_query",
        "terminal_start",
        "terminal_write",
        "terminal_resize",
        "terminal_ack_output",
        "terminal_close",
    ];

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to build Twominal");
}
