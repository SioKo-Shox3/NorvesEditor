//! 本番アダプタと受入試験で共有する、許可・編集列・操作記録の構成。

use std::{path::PathBuf, sync::Arc, time::SystemTime};

use crate::{
    bridge_state::BridgeState,
    edit_service::{EditEventSink, EditService},
};

use super::{
    capture::CaptureStore, operations::OperationStore, runtime::McpRuntime, McpAuthorization,
};

pub(crate) struct McpServices {
    pub(crate) edits: EditService,
    pub(crate) runtime: McpRuntime,
}

impl McpServices {
    pub(crate) fn new(
        bridge: &BridgeState,
        config_dir: PathBuf,
        operations: OperationStore,
        edit_events: Option<EditEventSink>,
        capture_dir: Option<PathBuf>,
    ) -> Self {
        let authorization = McpAuthorization::default();
        let edits =
            EditService::with_services(bridge.edit_facade(), authorization.clone(), edit_events);
        let reads = bridge
            .mcp_read_context()
            .with_history_source(edits.confirmation_history_source())
            .with_operations(operations)
            .with_authorization(authorization.clone())
            .with_write_service(Arc::new(edits.clone()));
        let reads = if let Some(directory) = capture_dir {
            let store = CaptureStore::new(directory);
            if let Err(error) = store.prepare() {
                tracing::warn!(%error, "撮影用ディレクトリの準備に失敗しました。");
            }
            if let Err(error) = store.cleanup(SystemTime::now()) {
                tracing::warn!(%error, "撮影用ディレクトリの片付けに失敗しました。");
            }
            reads.with_capture_store(store)
        } else {
            reads
        };
        let runtime = McpRuntime::with_services(config_dir, authorization, reads);
        Self { edits, runtime }
    }
}
