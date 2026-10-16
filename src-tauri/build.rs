// Declara los comandos del puente para generar sus permisos (allow-<comando>): las
// capabilities de capabilities/*.json deciden qué origen puede invocar cada uno.
fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "ping",
            "desktop_info",
            "printers_list",
            "printers_save",
            "printers_remove",
            "printers_discover",
            "printers_print",
            "printers_status",
            "notify_incoming",
            "notify_clear",
            "window_bring_to_front",
            "window_set_kiosk",
            "power_keep_awake",
            "update_check",
            "update_install",
            "support_start_remote",
            "support_copy_diagnostics",
            "session_remember",
            "session_recall",
            "settings_set_autostart",
            "page_ready",
            "setup_validate",
            "setup_save",
            "shell_retry",
            "shell_open_clock_settings",
            "shell_state",
            "shell_quit",
        ])),
    )
    .expect("tauri build");
}
