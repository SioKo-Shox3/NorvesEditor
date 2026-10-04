//! 本番アダプタと受入試験で共有する、許可・編集列・操作記録の構成。

use std::{path::PathBuf, sync::Arc};

use crate::{
    bridge_state::BridgeState,
    edit_service::{EditEventSink, EditService},
};

use super::{operations::OperationStore, runtime::McpRuntime, McpAuthorization};

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
        let runtime = McpRuntime::with_services(config_dir, authorization, reads);
        Self { edits, runtime }
    }
}
