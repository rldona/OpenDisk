// Declares the app's commands so each one needs an explicit permission in
// `capabilities/default.json` (the command allowlist).
fn main() {
    let commands = &[
        "app_info",
        "disk_info",
        "fda_status",
        "open_fda_settings",
        "start_scan",
        "cancel_scan",
        "get_tree",
        "get_findings",
        "create_plan",
        "execute_plan",
        "undo_run",
        "list_snapshots",
        "reveal_path",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("failed to run tauri-build");
}
