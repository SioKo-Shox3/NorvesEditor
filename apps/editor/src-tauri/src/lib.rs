// NorvesEditor Tauri entry point. P1 skeleton: builds a single window with the
// default plugins only. No custom commands, bridge wiring, or DTOs yet (added
// in later phases).

// P3: IPC name constants. #![allow(dead_code)] is declared inside the module
// file itself; P5 will reference the consts and that allow will be removed.
mod protocol_names;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
