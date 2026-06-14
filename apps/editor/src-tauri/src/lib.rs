// NorvesEditor Tauri entry point. P5 wires the real editor-client backend: the
// Rust side OWNS the Bridge connection + lifecycle and exposes it to the UI as
// Tauri commands, relaying engine events to the frontend. No frontend UI wiring
// (P6) and no app panels (P4) here.

// P3: IPC name constants. Now referenced by the command fns + event relay.
mod protocol_names;

mod bridge_state;
mod dto;
mod error;
mod events_map;

use bridge_state::BridgeState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // The backend owns the connection state for the whole app lifetime.
        .manage(BridgeState::default())
        .invoke_handler(tauri::generate_handler![
            bridge_state::bridge_connect,
            bridge_state::bridge_disconnect,
            bridge_state::bridge_reconnect,
            bridge_state::get_status,
            bridge_state::runtime_play,
            bridge_state::runtime_pause,
            bridge_state::runtime_stop,
            bridge_state::focus_viewport,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
