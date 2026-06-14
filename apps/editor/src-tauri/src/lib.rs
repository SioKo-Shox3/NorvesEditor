// NorvesEditor Tauri entry point. P1 skeleton: builds a single window with the
// default plugins only. No custom commands, bridge wiring, or DTOs yet (added
// in later phases).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
