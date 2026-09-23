// NorvesEditor Tauri entry point. P5 wires the real editor-client backend: the
// Rust side OWNS the Bridge connection + lifecycle and exposes it to the UI as
// Tauri commands, relaying engine events to the frontend. No frontend UI wiring
// (P6) and no app panels (P4) here.

// P3: IPC name constants. Now referenced by the command fns + event relay.
mod protocol_names;

mod asset_manifest;
mod bridge_state;
mod dto;
// エンジン設定(実行ファイルのパス)の保存と、Rust 側で開くファイル選択ダイアログ。
mod engine_settings;
mod error;
mod events_map;
// Windows 限定: 起動したエンジンをエディタの寿命に縛る Job Object。
#[cfg(windows)]
mod job_object;
mod process;
// J3: the LOAD-BEARING process runtime (spawn / READY / monitor / kill).
mod process_runtime;
mod workspace;

use bridge_state::BridgeState;
use engine_settings::EngineSettingsState;
use process_runtime::ProcessState;
use workspace::WorkspaceState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WARN 以上を stderr へ出す。既に初期化済み(テスト等)なら何もしない。
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::io::stderr)
        .try_init();
    let app = tauri::Builder::default()
        // The backend owns the connection state for the whole app lifetime.
        .manage(BridgeState::default())
        // J3: the (at most one) running engine process, separate from the
        // connection state.
        .manage(ProcessState::default())
        // Phase A: workspace root state is a pure editor concern, independent
        // from the Bridge connection and engine process lifecycle.
        .manage(WorkspaceState::default())
        // 設定ファイルの読み替え・書き戻しを直列にする。
        .manage(EngineSettingsState::default())
        .invoke_handler(tauri::generate_handler![
            bridge_state::bridge_connect,
            bridge_state::bridge_disconnect,
            bridge_state::bridge_reconnect,
            bridge_state::get_status,
            bridge_state::scene_get_tree,
            bridge_state::scene_create_object,
            bridge_state::scene_delete_object,
            bridge_state::scene_reparent_object,
            bridge_state::scene_duplicate_object,
            bridge_state::object_get_snapshot,
            bridge_state::object_set_property,
            bridge_state::component_add,
            bridge_state::component_remove,
            bridge_state::schema_get_snapshot,
            bridge_state::viewport_get_thumbnail,
            bridge_state::asset_resolve,
            bridge_state::asset_get_manifest,
            bridge_state::asset_reload_manifest,
            bridge_state::runtime_play,
            bridge_state::runtime_pause,
            bridge_state::runtime_stop,
            bridge_state::focus_viewport,
            process_runtime::launch_engine,
            process_runtime::stop_engine,
            engine_settings::get_engine_settings,
            engine_settings::pick_engine_path,
            engine_settings::clear_engine_path,
            engine_settings::set_engine_args,
            workspace::workspace_open,
            workspace::workspace_get,
            workspace::workspace_close,
            asset_manifest::asset_read_manifest,
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
