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
mod process;
// J3: the LOAD-BEARING process runtime (spawn / READY / monitor / kill).
mod process_runtime;

use bridge_state::BridgeState;
use process_runtime::ProcessState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // The backend owns the connection state for the whole app lifetime.
        .manage(BridgeState::default())
        // J3: the (at most one) running engine process, separate from the
        // connection state.
        .manage(ProcessState::default())
        .invoke_handler(tauri::generate_handler![
            bridge_state::bridge_connect,
            bridge_state::bridge_disconnect,
            bridge_state::bridge_reconnect,
            bridge_state::get_status,
            bridge_state::scene_get_tree,
            bridge_state::object_get_snapshot,
            bridge_state::schema_get_snapshot,
            bridge_state::runtime_play,
            bridge_state::runtime_pause,
            bridge_state::runtime_stop,
            bridge_state::focus_viewport,
            process_runtime::launch_engine,
            process_runtime::stop_engine,
        ])
        // Build (not `run`) so we can install the app-exit hook below.
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // J3: on app exit, best-effort kill a running engine so it is not orphaned.
    // `kill_on_drop(true)` is only a safety net (not guaranteed on abort), so the
    // explicit kill here is preferred. Both ExitRequested and Exit are handled so
    // the kill fires whether the exit is user-initiated or programmatic.
    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
            process_runtime::kill_engine_on_exit(app_handle);
        }
        _ => {}
    });
}
