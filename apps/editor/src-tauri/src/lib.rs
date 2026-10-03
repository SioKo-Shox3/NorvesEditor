// NorvesEditorのTauriエントリーポイント。Bridge接続とエンジンプロセスの寿命をRustが管理し、
// UIにはTauri command/eventを通して公開する。

// BridgeのIPC名を一か所で管理する。
mod protocol_names;

mod asset_manifest;
// バックエンドの警告を stderr とアプリのログディレクトリのファイルへ出す。
mod backend_log;
mod bridge_state;
mod dto;
mod edit_service;
// エンジン設定(実行ファイルのパス)の保存と、Rust 側で開くファイル選択ダイアログ。
mod engine_settings;
mod error;
mod events_map;
pub mod mcp;
mod mcp_settings;
pub mod mcp_token;
// Windows 限定: 起動したエンジンをエディタの寿命に縛る Job Object。
#[cfg(windows)]
mod job_object;
mod process;
// J3: the LOAD-BEARING process runtime (spawn / READY / monitor / kill).
mod process_runtime;
mod workspace;

use bridge_state::BridgeState;
use edit_service::EditService;
use engine_settings::EngineSettingsState;
use mcp::runtime::McpRuntime;
use mcp::McpAuthorization;
use process_runtime::ProcessState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Manager;
use workspace::WorkspaceState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 接続・エンジンプロセス・ワークスペース・設定をアプリの状態として共有する。
    let app = tauri::Builder::default()
        .manage(BridgeState::default())
        .manage(ProcessState::default())
        .manage(WorkspaceState::default())
        .manage(EngineSettingsState::default())
        .setup(|app| {
            // 配布版にもWARN以上を残せるよう、AppHandle生成後にログ出力先を決める。
            backend_log::init(app.path().app_log_dir().ok());
            let config_dir = app.path().app_config_dir().map_err(std::io::Error::other)?;
            let authorization = McpAuthorization::default();
            let bridge = app.state::<BridgeState>();
            app.manage(EditService::new_with_app_and_authorization(
                bridge.edit_facade(),
                app.handle().clone(),
                authorization.clone(),
            ));
            let mcp_runtime = McpRuntime::new(config_dir, authorization);
            app.manage(mcp_runtime.clone());
            tauri::async_runtime::spawn(async move {
                mcp_runtime.initialize().await;
            });
            Ok(())
        })
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
            bridge_state::edit_undo,
            bridge_state::edit_redo,
            bridge_state::edit_get_history,
            edit_service::edit_retry,
            edit_service::edit_discard,
            bridge_state::focus_viewport,
            process_runtime::launch_engine,
            process_runtime::stop_engine,
            engine_settings::get_engine_settings,
            engine_settings::pick_engine_path,
            engine_settings::clear_engine_path,
            engine_settings::set_engine_args,
            mcp::runtime::get_mcp_settings,
            mcp::runtime::set_mcp_settings,
            mcp::runtime::set_mcp_write_access,
            mcp::runtime::get_mcp_token,
            mcp::runtime::regenerate_mcp_token,
            workspace::workspace_open,
            workspace::workspace_get,
            workspace::workspace_close,
            asset_manifest::asset_read_manifest,
        ])
        // Build (not `run`) so we can install the app-exit hook below.
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    let exit_started = Arc::new(AtomicBool::new(false));
    let exit_completed = Arc::new(AtomicBool::new(false));
    // 終了要求をいったん延期し、MCP・編集列・Bridge・エンジンの非同期停止後に終了する。
    app.run(move |app_handle, event| match event {
        tauri::RunEvent::ExitRequested { code, api, .. } => {
            if exit_completed.load(Ordering::Acquire) {
                return;
            }
            api.prevent_exit();
            if !exit_started.swap(true, Ordering::AcqRel) {
                let app_handle = app_handle.clone();
                let exit_completed = Arc::clone(&exit_completed);
                tauri::async_runtime::spawn(async move {
                    app_handle.state::<McpRuntime>().shutdown().await;
                    app_handle.state::<EditService>().shutdown().await;
                    bridge_state::shutdown_on_exit(app_handle.state::<BridgeState>().inner()).await;
                    process_runtime::shutdown_on_exit(&app_handle).await;
                    exit_completed.store(true, Ordering::Release);
                    app_handle.exit(code.unwrap_or_default());
                });
            }
        }
        tauri::RunEvent::Exit => process_runtime::kill_engine_on_exit(app_handle),
        _ => {}
    });
}
