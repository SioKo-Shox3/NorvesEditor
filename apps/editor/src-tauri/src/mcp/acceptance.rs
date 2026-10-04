//! 受入試験から本番サービスを操作する薄いアダプタ。画面通知の代わりにDTOを取り出す。

use std::{path::PathBuf, sync::Arc};

use serde_json::Value;

use super::{operations::OperationStore, runtime::McpRuntime, service::McpServices};
use crate::{
    bridge_state::{self, BridgeState},
    edit_service::HistoryDirection,
};

pub struct HeadlessEditor {
    bridge: BridgeState,
    services: McpServices,
    operations: OperationStore,
}

impl HeadlessEditor {
    pub fn new(directory: PathBuf) -> Self {
        let bridge = BridgeState::default();
        let operations = OperationStore::new(Some(directory.join("logs")));
        let services =
            McpServices::new(&bridge, directory.join("config"), operations.clone(), None);
        Self {
            bridge,
            services,
            operations,
        }
    }

    pub fn runtime(&self) -> &McpRuntime {
        &self.services.runtime
    }

    pub async fn connect(&self, port: u16) -> Result<(), String> {
        bridge_state::connect_service(Arc::new(|_, _| {}), &self.bridge, port)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// 本番と同じ切断入口でrelay・接続世代・履歴を失効させる。
    pub async fn disconnect(&self) {
        bridge_state::disconnect_quietly(&self.bridge).await;
    }

    pub fn history(&self) -> Value {
        serde_json::to_value(self.services.edits.history_summary()).expect("履歴DTOの直列化")
    }

    pub fn operations(&self) -> Value {
        serde_json::to_value(self.operations.snapshot()).expect("操作記録DTOの直列化")
    }

    /// 画面と同じ履歴先頭・改訂付き入口で取り消す。
    pub async fn undo_from_ui(&self) -> Result<Value, String> {
        let (head, revision) = self.services.edits.history_cursor(HistoryDirection::Undo);
        self.services
            .edits
            .enqueue_undo_from_ui(head, revision)
            .map_err(|error| error.to_string())?
            .result()
            .await
            .map_err(|error| error.to_string())
    }

    /// HTTP、編集actor、Bridgeのrelay/dispatcherと画像workerを順に停止・joinする。
    pub async fn shutdown(&self) {
        self.services.runtime.shutdown().await;
        self.services.edits.shutdown().await;
        bridge_state::shutdown_on_exit(&self.bridge).await;
    }
}
