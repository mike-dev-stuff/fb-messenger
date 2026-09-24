fn main() {
    // Declaring the command here autogenerates its `allow-update-badge`
    // permission. Without it the ACL rejects the call from messenger.com
    // with "not allowed. Plugin not found" and the dock badge never updates.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&["update_badge", "debug_report"]),
        ),
    )
    .expect("failed to run tauri-build");
}
