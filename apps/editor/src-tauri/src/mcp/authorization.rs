//! MCP書き込み許可と、シーン部分木に対する範囲検査。

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    time::Duration,
};

use norves_bridge_editor_client::{parse_scene_tree_result, ObjectSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::McpWriteMode;
use crate::edit_service::{EditSource, HistoryDirection, HistoryRecord};

/// 1回の範囲解決で走査するノード数。
pub(crate) const MAX_SCOPE_NODES: usize = 2_000;
/// 1回の範囲解決で取得するsnapshot数。
pub(crate) const MAX_SCOPE_SNAPSHOTS: usize = 200;
/// scene treeとobject snapshotの合計転送量。
pub(crate) const MAX_SCOPE_BYTES: usize = 2 * 1024 * 1024;
/// 1回の範囲解決に許す時間。
pub(crate) const MAX_SCOPE_RESOLUTION_MILLIS: u64 = 5_000;
/// Settingsから受け付けるシーンIDの最大長。
pub(crate) const MAX_SCOPE_ROOT_ID_BYTES: usize = 256;

/// MCP書き込みの実行時設定。プロセス再起動後は必ずread-onlyへ戻る。
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct McpWriteSettings {
    pub(crate) mode: McpWriteMode,
    pub(crate) scene_root_id: Option<String>,
}

impl McpWriteSettings {
    pub(crate) fn validate(&self) -> Result<(), ScopeError> {
        if self.scene_root_id.as_ref().is_some_and(|id| {
            id.is_empty() || id.len() > MAX_SCOPE_ROOT_ID_BYTES || id.contains('\0')
        }) {
            return Err(ScopeError::InvalidScopeRoot);
        }
        Ok(())
    }
}

/// MCPが取り消す履歴まとまりの向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpHistoryDirection {
    Undo,
    Redo,
}

impl From<McpHistoryDirection> for HistoryDirection {
    fn from(value: McpHistoryDirection) -> Self {
        match value {
            McpHistoryDirection::Undo => Self::Undo,
            McpHistoryDirection::Redo => Self::Redo,
        }
    }
}

/// 先頭の履歴まとまり全体を、編集列の改訂に結び付けて固定する。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McpHistoryAction {
    pub(crate) direction: McpHistoryDirection,
    pub(crate) head_id: u64,
    pub(crate) revision: u64,
    pub(crate) group_name: String,
    pub(crate) source: EditSource,
    pub(crate) records: Vec<HistoryRecord>,
}

impl McpHistoryAction {
    /// この操作より前に再作成される旧IDだけを解決する。範囲はcheck_historyで検証する。
    pub(crate) fn recreates_before(&self, position: usize, object_id: &str) -> bool {
        self.direction == McpHistoryDirection::Redo
            && self.records.iter().take(position).any(|record| match record {
                HistoryRecord::Create { created_id, .. }
                | HistoryRecord::Duplicate { created_id, .. } => created_id == object_id,
                _ => false,
            })
    }
}

/// 書き込み道具が要求する対象。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum McpWriteOperation {
    Property {
        object_id: String,
    },
    Create {
        parent_id: Option<String>,
    },
    Duplicate {
        object_id: String,
        new_parent_id: Option<String>,
    },
    Reparent {
        object_id: String,
        new_parent_id: Option<String>,
    },
    Delete {
        object_id: String,
    },
    ComponentAdd {
        object_id: String,
    },
    ComponentRemove {
        component_id: String,
    },
    RuntimeControl,
    History(McpHistoryAction),
}

impl McpWriteOperation {
    /// Bridge methodの名前とparamsから、検査対象を取り出す。
    pub(crate) fn from_bridge_method(method: &str, params: &Value) -> Result<Self, ScopeError> {
        let object = params.as_object().ok_or(ScopeError::InvalidWriteRequest)?;
        let required_id = |key: &str| -> Result<String, ScopeError> {
            object
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or(ScopeError::InvalidWriteRequest)
        };
        let optional_id = |key: &str| -> Result<Option<String>, ScopeError> {
            match object.get(key) {
                None => Ok(None),
                Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.clone())),
                _ => Err(ScopeError::InvalidWriteRequest),
            }
        };

        match method {
            "object.setProperty" => Ok(Self::Property {
                object_id: required_id("objectId")?,
            }),
            "scene.createObject" => Ok(Self::Create {
                parent_id: optional_id("parentId")?,
            }),
            "scene.duplicateObject" => Ok(Self::Duplicate {
                object_id: required_id("objectId")?,
                new_parent_id: optional_id("newParentId")?,
            }),
            "scene.reparentObject" => Ok(Self::Reparent {
                object_id: required_id("objectId")?,
                new_parent_id: optional_id("newParentId")?,
            }),
            "scene.deleteObject" => Ok(Self::Delete {
                object_id: required_id("objectId")?,
            }),
            "component.add" => Ok(Self::ComponentAdd {
                object_id: required_id("objectId")?,
            }),
            "component.remove" => Ok(Self::ComponentRemove {
                component_id: required_id("objectId")?,
            }),
            "runtime.play" | "runtime.pause" | "runtime.stop" if object.is_empty() => {
                Ok(Self::RuntimeControl)
            }
            _ => Err(ScopeError::UnsupportedWrite),
        }
    }

    /// 範囲検査に必要な能力名を返す。
    pub(crate) fn required_capabilities(&self) -> Vec<&'static str> {
        match self {
            Self::Property { .. } => vec!["object.edit", "object.query", "scene.query"],
            Self::Create { .. } => vec!["scene.edit", "scene.query"],
            Self::Duplicate { .. } | Self::Reparent { .. } | Self::Delete { .. } => {
                vec!["scene.edit", "scene.query"]
            }
            Self::ComponentAdd { .. } | Self::ComponentRemove { .. } => {
                vec!["component.edit", "object.query", "scene.query"]
            }
            Self::RuntimeControl => vec!["runtime.control"],
            Self::History(action) => history_required_capabilities(action),
        }
    }

    /// 書き込み可モードでも、個別確認が欠かせない操作かを返す。
    pub(crate) fn requires_confirmation(&self) -> bool {
        match self {
            Self::Delete { .. } | Self::ComponentRemove { .. } => true,
            Self::History(action) => action.records.iter().any(|record| {
                matches!(
                    (record, action.direction),
                    (
                        HistoryRecord::Create { .. } | HistoryRecord::Duplicate { .. },
                        McpHistoryDirection::Undo
                    )
                )
            }),
            _ => false,
        }
    }
}

fn history_required_capabilities(action: &McpHistoryAction) -> Vec<&'static str> {
    let mut required = vec!["scene.query"];
    for record in &action.records {
        match record {
            HistoryRecord::Create { .. }
            | HistoryRecord::Duplicate { .. }
            | HistoryRecord::Reparent { .. } => {
                required.push("scene.edit");
            }
            HistoryRecord::SetProperty { .. } => {
                required.push("object.edit");
                required.push("object.query");
            }
        }
    }
    required.sort_unstable();
    required.dedup();
    required
}

/// 1回限りの許可の検査に失敗した理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeError {
    InvalidScopeRoot,
    InvalidWriteRequest,
    UnsupportedWrite,
    ReadOnly,
    ConfirmationRequired,
    AuthorizationRevoked,
    ScopeRootUnknown,
    ObjectUnknown,
    ObjectOutsideScope,
    ComponentMembershipUnknown,
    ComponentOutsideScope,
    TreeMalformed,
    DuplicateObjectId,
    LimitExceeded,
    DeadlineExceeded,
    HistoryUnavailable,
}

impl ScopeError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::InvalidScopeRoot => "シーン範囲のIDが空か、長さの上限を超えています。",
            Self::InvalidWriteRequest => "書き込み対象のIDまたは引数が不正です。",
            Self::UnsupportedWrite => "この書き込み操作は未対応です。",
            Self::ReadOnly => "MCPは読み取り専用です。Settingsで書き込み許可を変更してください。",
            Self::ConfirmationRequired => "この操作にはエディタ画面での確認が必要です。",
            Self::AuthorizationRevoked => "MCPの許可が変更されたため、この要求は失効しました。",
            Self::ScopeRootUnknown => "許可範囲のシーンIDが現在のツリーに存在しません。",
            Self::ObjectUnknown => "対象IDがシーン内に存在することを確認できません。",
            Self::ObjectOutsideScope => "対象IDが許可されたシーン部分木の外にあります。",
            Self::ComponentMembershipUnknown => {
                "componentの所属を確認できないため、操作を拒否しました。"
            }
            Self::ComponentOutsideScope => "componentが許可されたシーン部分木に属していません。",
            Self::TreeMalformed => "範囲検査に使うシーンツリーの応答形式が不正です。",
            Self::DuplicateObjectId => "シーンツリーに重複IDがあるため、範囲を確定できません。",
            Self::LimitExceeded => "範囲検査がノード・snapshot・データ量の上限を超えました。",
            Self::DeadlineExceeded => "範囲検査が5秒以内に完了しませんでした。",
            Self::HistoryUnavailable => "取り消し対象の先頭まとまりを取得できません。",
        }
    }
}

/// scene treeと必要時のsnapshotを集計し、上限超過を拒否する。
#[derive(Debug)]
pub(crate) struct McpScopeBudget {
    nodes: usize,
    snapshots: usize,
    bytes: usize,
}

impl McpScopeBudget {
    pub(crate) fn new(tree_bytes: usize) -> Result<Self, ScopeError> {
        if tree_bytes > MAX_SCOPE_BYTES {
            return Err(ScopeError::LimitExceeded);
        }
        Ok(Self {
            nodes: 0,
            snapshots: 0,
            bytes: tree_bytes,
        })
    }

    pub(crate) fn add_nodes(&mut self, count: usize) -> Result<(), ScopeError> {
        self.nodes = self.nodes.saturating_add(count);
        if self.nodes > MAX_SCOPE_NODES {
            return Err(ScopeError::LimitExceeded);
        }
        Ok(())
    }

    pub(crate) fn begin_snapshot(&mut self) -> Result<(), ScopeError> {
        if self.snapshots >= MAX_SCOPE_SNAPSHOTS {
            return Err(ScopeError::LimitExceeded);
        }
        self.snapshots += 1;
        Ok(())
    }

    pub(crate) fn add_bytes(&mut self, size: usize) -> Result<(), ScopeError> {
        self.bytes = self.bytes.saturating_add(size);
        if self.bytes > MAX_SCOPE_BYTES {
            return Err(ScopeError::LimitExceeded);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn snapshot_count(&self) -> usize {
        self.snapshots
    }
}

#[derive(Debug, Clone)]
struct SceneNodeMeta {
    parent: Option<String>,
    children: Vec<String>,
}

/// 接続中の完全なシーンツリーから作る、部分木の範囲索引。
#[derive(Debug, Clone)]
pub(crate) struct McpSceneScopeIndex {
    root_id: String,
    scene_root_id: String,
    nodes: HashMap<String, SceneNodeMeta>,
    allowed: HashSet<String>,
    component_owners: HashMap<String, String>,
}

impl McpSceneScopeIndex {
    pub(crate) fn from_result(
        result: &Value,
        scene_root_scope: Option<&str>,
        budget: &mut McpScopeBudget,
    ) -> Result<Self, ScopeError> {
        let tree = parse_scene_tree_result(result).map_err(|_| ScopeError::TreeMalformed)?;
        let scene_root_id = tree.root.id.clone();
        let mut nodes = HashMap::new();
        let mut pending = vec![(&tree.root, None::<String>)];
        while let Some((node, parent)) = pending.pop() {
            budget.add_nodes(1)?;
            if nodes.contains_key(&node.id) {
                return Err(ScopeError::DuplicateObjectId);
            }
            let children = node
                .children
                .iter()
                .map(|child| child.id.clone())
                .collect::<Vec<_>>();
            for child in node.children.iter().rev() {
                pending.push((child, Some(node.id.clone())));
            }
            nodes.insert(node.id.clone(), SceneNodeMeta { parent, children });
        }

        let root_id = scene_root_scope.unwrap_or(&scene_root_id).to_owned();
        let Some(root) = nodes.get(&root_id) else {
            return Err(ScopeError::ScopeRootUnknown);
        };
        let _ = root;
        let mut allowed = HashSet::new();
        let mut descendants = vec![root_id.clone()];
        while let Some(id) = descendants.pop() {
            if !allowed.insert(id.clone()) {
                return Err(ScopeError::DuplicateObjectId);
            }
            let Some(node) = nodes.get(&id) else {
                return Err(ScopeError::TreeMalformed);
            };
            descendants.extend(node.children.iter().cloned());
        }

        Ok(Self {
            root_id,
            scene_root_id,
            nodes,
            allowed,
            component_owners: HashMap::new(),
        })
    }

    pub(crate) fn scene_object_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.nodes.keys().cloned().collect();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn contains_node(&self, object_id: &str) -> bool {
        self.nodes.contains_key(object_id)
    }

    pub(crate) fn contains_component(&self, component_id: &str) -> bool {
        self.component_owners.contains_key(component_id)
    }

    pub(crate) fn is_full_scene(&self) -> bool {
        self.root_id == self.scene_root_id
    }

    pub(crate) fn record_component_snapshot(
        &mut self,
        expected_owner: &str,
        snapshot: ObjectSnapshot,
    ) -> Result<(), ScopeError> {
        if snapshot.object_id != expected_owner || !self.nodes.contains_key(expected_owner) {
            return Err(ScopeError::ComponentMembershipUnknown);
        }
        let Some(components) = snapshot.components else {
            return Err(ScopeError::ComponentMembershipUnknown);
        };
        let mut snapshot_ids = HashSet::new();
        for component in &components {
            if self.nodes.contains_key(&component.object_id)
                || self.component_owners.contains_key(&component.object_id)
                || !snapshot_ids.insert(component.object_id.as_str())
            {
                return Err(ScopeError::ComponentMembershipUnknown);
            }
        }
        for component in components {
            self.component_owners
                .insert(component.object_id, expected_owner.to_owned());
        }
        Ok(())
    }

    pub(crate) fn check_operation(&self, operation: &McpWriteOperation) -> Result<(), ScopeError> {
        match operation {
            McpWriteOperation::Property { object_id } => self.check_object_or_component(object_id),
            McpWriteOperation::Create { parent_id } => self.check_parent(parent_id.as_deref()),
            McpWriteOperation::Duplicate {
                object_id,
                new_parent_id,
            } => {
                self.check_object(object_id)?;
                let parent = new_parent_id.as_deref().or_else(|| {
                    self.nodes
                        .get(object_id)
                        .and_then(|node| node.parent.as_deref())
                });
                self.check_parent(parent)
            }
            McpWriteOperation::Reparent {
                object_id,
                new_parent_id,
            } => {
                self.check_object(object_id)?;
                let old_parent = self
                    .nodes
                    .get(object_id)
                    .and_then(|node| node.parent.as_deref());
                self.check_parent(old_parent)?;
                self.check_parent(new_parent_id.as_deref())
            }
            McpWriteOperation::Delete { object_id } => self.check_delete_subtree(object_id),
            McpWriteOperation::ComponentAdd { object_id } => self.check_object(object_id),
            McpWriteOperation::ComponentRemove { component_id } => {
                self.check_component(component_id)
            }
            McpWriteOperation::RuntimeControl => {
                if self.is_full_scene() {
                    Ok(())
                } else {
                    Err(ScopeError::ObjectOutsideScope)
                }
            }
            McpWriteOperation::History(action) => self.check_history(action),
        }
    }

    pub(crate) fn check_object_or_component(&self, object_id: &str) -> Result<(), ScopeError> {
        if self.nodes.contains_key(object_id) {
            return self.check_object(object_id);
        }
        self.check_component(object_id)
    }

    fn check_object(&self, object_id: &str) -> Result<(), ScopeError> {
        if !self.nodes.contains_key(object_id) {
            return Err(ScopeError::ObjectUnknown);
        }
        if !self.allowed.contains(object_id) {
            return Err(ScopeError::ObjectOutsideScope);
        }
        Ok(())
    }

    fn check_component(&self, component_id: &str) -> Result<(), ScopeError> {
        match self.component_owners.get(component_id) {
            Some(owner) if self.allowed.contains(owner) => Ok(()),
            Some(_) => Err(ScopeError::ComponentOutsideScope),
            None => Err(ScopeError::ComponentMembershipUnknown),
        }
    }

    fn check_parent(&self, parent_id: Option<&str>) -> Result<(), ScopeError> {
        match parent_id {
            Some(parent_id) => self.check_object(parent_id),
            None if self.is_full_scene() => Ok(()),
            None => Err(ScopeError::ObjectOutsideScope),
        }
    }

    fn check_delete_subtree(&self, object_id: &str) -> Result<(), ScopeError> {
        self.check_object(object_id)?;
        let parent = self
            .nodes
            .get(object_id)
            .and_then(|node| node.parent.as_deref());
        self.check_parent(parent)?;
        let mut descendants = vec![object_id.to_owned()];
        while let Some(id) = descendants.pop() {
            if !self.allowed.contains(&id) {
                return Err(ScopeError::ObjectOutsideScope);
            }
            let Some(node) = self.nodes.get(&id) else {
                return Err(ScopeError::ObjectUnknown);
            };
            descendants.extend(node.children.iter().cloned());
        }
        Ok(())
    }

    fn check_history(&self, action: &McpHistoryAction) -> Result<(), ScopeError> {
        if action.records.is_empty() {
            return Err(ScopeError::HistoryUnavailable);
        }
        // 現在のツリーを変更せず、順に検査を通った再作成だけを後続の対象へ加える。
        let mut projected = self.clone();
        for record in &action.records {
            match (record, action.direction) {
                (HistoryRecord::Create { created_id, .. }, McpHistoryDirection::Undo)
                | (HistoryRecord::Duplicate { created_id, .. }, McpHistoryDirection::Undo) => {
                    self.check_delete_subtree(created_id)?;
                }
                (
                    HistoryRecord::Create {
                        parent_id,
                        created_id,
                        ..
                    },
                    McpHistoryDirection::Redo,
                ) => {
                    projected.add_recreated_node(created_id, parent_id.clone())?;
                }
                (
                    HistoryRecord::Duplicate {
                        source_object_id,
                        created_id,
                        parent_id,
                    },
                    McpHistoryDirection::Redo,
                ) => {
                    projected.check_object(source_object_id)?;
                    let parent = parent_id.clone().or_else(|| {
                        projected
                            .nodes
                            .get(source_object_id)
                            .and_then(|node| node.parent.clone())
                    });
                    projected.add_recreated_node(created_id, parent)?;
                }
                (
                    HistoryRecord::Reparent {
                        object_id,
                        old_parent_id,
                        new_parent_id,
                    },
                    _,
                ) => {
                    projected.check_object(object_id)?;
                    projected.check_parent(old_parent_id.as_deref())?;
                    projected.check_parent(new_parent_id.as_deref())?;
                    if action.direction == McpHistoryDirection::Redo {
                        let current_parent = projected
                            .nodes
                            .get(object_id)
                            .and_then(|node| node.parent.as_deref());
                        projected.check_parent(current_parent)?;
                        // 後続duplicateの親省略も、先行reparent後の親で検査する。
                        projected
                            .nodes
                            .get_mut(object_id)
                            .expect("対象を検査済み")
                            .parent = new_parent_id
                            .clone()
                            .or_else(|| Some(projected.scene_root_id.clone()));
                    }
                }
                (HistoryRecord::SetProperty { object_id, .. }, _) => {
                    projected.check_object_or_component(object_id)?;
                }
            }
        }
        Ok(())
    }

    fn add_recreated_node(&mut self, id: &str, parent: Option<String>) -> Result<(), ScopeError> {
        self.check_parent(parent.as_deref())?;
        if self.nodes.contains_key(id) || self.component_owners.contains_key(id) {
            return Err(ScopeError::DuplicateObjectId);
        }
        if self.nodes.len() >= MAX_SCOPE_NODES {
            return Err(ScopeError::LimitExceeded);
        }
        let parent = parent.unwrap_or_else(|| self.scene_root_id.clone());
        self.nodes
            .get_mut(&parent)
            .ok_or(ScopeError::ObjectUnknown)?
            .children
            .push(id.to_owned());
        self.nodes.insert(
            id.to_owned(),
            SceneNodeMeta {
                parent: Some(parent),
                children: Vec::new(),
            },
        );
        self.allowed.insert(id.to_owned());
        Ok(())
    }
}

/// scene nodeでないproperty/component IDの所属を、snapshotから解決する。
pub(crate) fn parse_scope_tree_bytes(value: &Value) -> Result<usize, ScopeError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| ScopeError::TreeMalformed)
}

/// object snapshotの転送サイズを計測する。
pub(crate) fn parse_scope_snapshot_bytes(value: &Value) -> Result<usize, ScopeError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| ScopeError::ComponentMembershipUnknown)
}

/// 範囲照会のfutureへ共通の5秒期限を適用する。
pub(crate) async fn with_scope_deadline<T, F>(future: F) -> Result<T, ScopeError>
where
    F: Future<Output = T>,
{
    with_scope_deadline_for(future, Duration::from_millis(MAX_SCOPE_RESOLUTION_MILLIS)).await
}

async fn with_scope_deadline_for<T, F>(future: F, limit: Duration) -> Result<T, ScopeError>
where
    F: Future<Output = T>,
{
    tokio::time::timeout(limit, future)
        .await
        .map_err(|_| ScopeError::DeadlineExceeded)
}

/// 1回の要求を許可した設定改訂に結び付ける、使い捨てpermit。
#[derive(Debug, PartialEq)]
pub(crate) struct McpWritePermit {
    pub(crate) auth_revision: u64,
    pub(crate) policy_revision: u64,
    pub(crate) generation: u64,
    pub(crate) request_id: u64,
    pub(crate) confirmed: bool,
    pub(crate) operation: McpWriteOperation,
    pub(crate) review: Option<Box<super::reads::AuthorizationReview>>,
}

/// 認証・モード・範囲の検査に成功した瞬間の設定snapshot。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpWritePolicySnapshot {
    pub(crate) settings: McpWriteSettings,
    pub(crate) revision: u64,
}

/// actorの履歴記録から、まとまり内で触る全IDを取り出すための試験用変換。
#[cfg(test)]
pub(crate) fn history_record_ids(action: &McpHistoryAction) -> HashSet<String> {
    let mut ids = HashSet::new();
    for record in &action.records {
        match record {
            HistoryRecord::Create {
                created_id,
                parent_id,
                ..
            } => {
                ids.insert(created_id.clone());
                ids.extend(parent_id.iter().cloned());
            }
            HistoryRecord::Duplicate {
                source_object_id,
                created_id,
                parent_id,
            } => {
                ids.insert(source_object_id.clone());
                ids.insert(created_id.clone());
                ids.extend(parent_id.iter().cloned());
            }
            HistoryRecord::Reparent {
                object_id,
                old_parent_id,
                new_parent_id,
            } => {
                ids.insert(object_id.clone());
                ids.extend(old_parent_id.iter().cloned());
                ids.extend(new_parent_id.iter().cloned());
            }
            HistoryRecord::SetProperty { object_id, .. } => {
                ids.insert(object_id.clone());
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::McpAuthorization;
    use serde_json::json;

    fn tree() -> Value {
        json!({"root":{"id":"scene","children":[
            {"id":"allowed","children":[{"id":"allowed-child","children":[{"id":"allowed-grandchild"}]}]},
            {"id":"outside","children":[{"id":"outside-child"}]}
        ]}})
    }

    fn index(scope: Option<&str>) -> (McpSceneScopeIndex, McpScopeBudget) {
        let result = tree();
        let bytes = parse_scope_tree_bytes(&result).expect("ツリーサイズを測る");
        let mut budget = McpScopeBudget::new(bytes).expect("予算を作る");
        let index = McpSceneScopeIndex::from_result(&result, scope, &mut budget)
            .expect("ツリーを索引化する");
        (index, budget)
    }

    #[test]
    fn scope_checks_write_modes_and_full_scene_runtime_control() {
        let full = McpWriteOperation::RuntimeControl;
        let (whole, _) = index(None);
        let (subtree, _) = index(Some("allowed"));
        assert_eq!(whole.check_operation(&full), Ok(()));
        assert_eq!(
            subtree.check_operation(&full),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(McpWriteMode::default(), McpWriteMode::ReadOnly);
        assert_eq!(McpWriteMode::Enabled, McpWriteMode::Enabled);
        assert_eq!(McpWriteMode::Confirm, McpWriteMode::Confirm);
        assert_eq!(
            McpWriteOperation::from_bridge_method("scene.unknown", &json!({})),
            Err(ScopeError::UnsupportedWrite)
        );
    }

    #[test]
    fn scope_checks_parent_duplicate_component_and_delete_targets() {
        let (mut scoped, _) = index(Some("allowed"));
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Create {
                parent_id: Some("outside".to_owned()),
            }),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Duplicate {
                object_id: "allowed".to_owned(),
                new_parent_id: Some("outside".to_owned()),
            }),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Reparent {
                object_id: "allowed".to_owned(),
                new_parent_id: Some("outside".to_owned()),
            }),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Reparent {
                object_id: "allowed".to_owned(),
                new_parent_id: Some("allowed-child".to_owned()),
            }),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Duplicate {
                object_id: "missing".to_owned(),
                new_parent_id: Some("allowed".to_owned()),
            }),
            Err(ScopeError::ObjectUnknown)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Property {
                object_id: "missing".to_owned(),
            }),
            Err(ScopeError::ComponentMembershipUnknown)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Property {
                object_id: "allowed-child".to_owned(),
            }),
            Ok(())
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Property {
                object_id: "outside-child".to_owned(),
            }),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::ComponentAdd {
                object_id: "allowed-child".to_owned(),
            }),
            Ok(())
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::ComponentAdd {
                object_id: "outside".to_owned(),
            }),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Delete {
                object_id: "outside".to_owned(),
            }),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Delete {
                object_id: "allowed".to_owned(),
            }),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::Delete {
                object_id: "allowed-child".to_owned(),
            }),
            Ok(())
        );
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::ComponentRemove {
                component_id: "unknown-component".to_owned(),
            }),
            Err(ScopeError::ComponentMembershipUnknown)
        );
        let snapshot: ObjectSnapshot =
            norves_bridge_editor_client::parse_object_snapshot_result(&json!({
                "objectId":"allowed",
                "properties":[],
                "components":[{"objectId":"component-1","kind":"Camera"}]
            }))
            .expect("component所属snapshotを読む");
        scoped
            .record_component_snapshot("allowed", snapshot)
            .expect("所属を登録する");
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::ComponentRemove {
                component_id: "component-1".to_owned(),
            }),
            Ok(())
        );
        let outside_snapshot: ObjectSnapshot =
            norves_bridge_editor_client::parse_object_snapshot_result(&json!({
                "objectId":"outside",
                "properties":[],
                "components":[{"objectId":"outside-component","kind":"Camera"}]
            }))
            .expect("範囲外component所属snapshotを読む");
        scoped
            .record_component_snapshot("outside", outside_snapshot)
            .expect("範囲外の所属も索引へ登録する");
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::ComponentRemove {
                component_id: "outside-component".to_owned(),
            }),
            Err(ScopeError::ComponentOutsideScope)
        );
        let incomplete_snapshot: ObjectSnapshot =
            norves_bridge_editor_client::parse_object_snapshot_result(&json!({
                "objectId":"allowed-child",
                "properties":[]
            }))
            .expect("所属が欠けたsnapshotを読む");
        assert_eq!(
            scoped.record_component_snapshot("allowed-child", incomplete_snapshot),
            Err(ScopeError::ComponentMembershipUnknown)
        );
    }

    #[test]
    fn ambiguous_component_membership_fails_closed() {
        let (mut scoped, _) = index(Some("allowed"));
        let duplicate_in_snapshot = ObjectSnapshot {
            object_id: "allowed".to_owned(),
            name: None,
            kind: None,
            properties: Vec::new(),
            components: Some(vec![
                norves_bridge_editor_client::ComponentRef {
                    object_id: "component-1".to_owned(),
                    kind: "Camera".to_owned(),
                },
                norves_bridge_editor_client::ComponentRef {
                    object_id: "component-1".to_owned(),
                    kind: "Camera".to_owned(),
                },
            ]),
        };
        assert_eq!(
            scoped.record_component_snapshot("allowed", duplicate_in_snapshot),
            Err(ScopeError::ComponentMembershipUnknown)
        );

        let colliding_with_node = ObjectSnapshot {
            object_id: "allowed".to_owned(),
            name: None,
            kind: None,
            properties: Vec::new(),
            components: Some(vec![norves_bridge_editor_client::ComponentRef {
                object_id: "allowed-child".to_owned(),
                kind: "Camera".to_owned(),
            }]),
        };
        assert_eq!(
            scoped.record_component_snapshot("allowed", colliding_with_node),
            Err(ScopeError::ComponentMembershipUnknown)
        );

        let first_owner = ObjectSnapshot {
            object_id: "allowed".to_owned(),
            name: None,
            kind: None,
            properties: Vec::new(),
            components: Some(vec![norves_bridge_editor_client::ComponentRef {
                object_id: "shared-component".to_owned(),
                kind: "Camera".to_owned(),
            }]),
        };
        let second_owner = ObjectSnapshot {
            object_id: "outside".to_owned(),
            name: None,
            kind: None,
            properties: Vec::new(),
            components: Some(vec![norves_bridge_editor_client::ComponentRef {
                object_id: "shared-component".to_owned(),
                kind: "Camera".to_owned(),
            }]),
        };
        scoped
            .record_component_snapshot("allowed", first_owner)
            .expect("一意な所属を記録する");
        assert_eq!(
            scoped.record_component_snapshot("outside", second_owner),
            Err(ScopeError::ComponentMembershipUnknown)
        );
    }

    #[test]
    fn history_scope_checks_every_record_in_the_top_group() {
        let (scoped, _) = index(Some("allowed"));
        let action = McpHistoryAction {
            direction: McpHistoryDirection::Undo,
            head_id: 9,
            revision: 4,
            group_name: "ユーザー編集".to_owned(),
            source: EditSource::Ui,
            records: vec![
                HistoryRecord::SetProperty {
                    object_id: "allowed-child".to_owned(),
                    property: "visible".to_owned(),
                    old_value: Value::Bool(false),
                    new_value: Value::Bool(true),
                },
                HistoryRecord::SetProperty {
                    object_id: "outside-child".to_owned(),
                    property: "visible".to_owned(),
                    old_value: Value::Bool(false),
                    new_value: Value::Bool(true),
                },
            ],
        };
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::History(action.clone())),
            Err(ScopeError::ObjectOutsideScope)
        );
        assert!(history_record_ids(&action).contains("outside-child"));

        for direction in [McpHistoryDirection::Undo, McpHistoryDirection::Redo] {
            let action = McpHistoryAction {
                direction,
                head_id: 10,
                revision: 5,
                group_name: "ユーザー編集".to_owned(),
                source: EditSource::Ui,
                records: vec![HistoryRecord::Reparent {
                    object_id: "allowed-child".to_owned(),
                    old_parent_id: Some("allowed".to_owned()),
                    new_parent_id: Some("outside".to_owned()),
                }],
            };
            assert_eq!(
                scoped.check_operation(&McpWriteOperation::History(action)),
                Err(ScopeError::ObjectOutsideScope)
            );
        }
        let undo_create_outside_parent = McpHistoryAction {
            direction: McpHistoryDirection::Undo,
            head_id: 13,
            revision: 8,
            group_name: "ユーザー編集".to_owned(),
            source: EditSource::Ui,
            records: vec![HistoryRecord::Create {
                created_id: "allowed".to_owned(),
                parent_id: Some("scene".to_owned()),
                kind: None,
            }],
        };
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::History(undo_create_outside_parent)),
            Err(ScopeError::ObjectOutsideScope)
        );
        let redo_create = McpHistoryAction {
            direction: McpHistoryDirection::Redo,
            head_id: 11,
            revision: 6,
            group_name: "ユーザー編集".to_owned(),
            source: EditSource::Ui,
            records: vec![HistoryRecord::Create {
                created_id: "future-child".to_owned(),
                parent_id: Some("outside".to_owned()),
                kind: None,
            }],
        };
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::History(redo_create)),
            Err(ScopeError::ObjectOutsideScope)
        );
        let redo_duplicate = McpHistoryAction {
            direction: McpHistoryDirection::Redo,
            head_id: 12,
            revision: 7,
            group_name: "ユーザー編集".to_owned(),
            source: EditSource::Ui,
            records: vec![HistoryRecord::Duplicate {
                source_object_id: "allowed".to_owned(),
                created_id: "future-copy".to_owned(),
                parent_id: Some("outside".to_owned()),
            }],
        };
        assert_eq!(
            scoped.check_operation(&McpWriteOperation::History(redo_duplicate)),
            Err(ScopeError::ObjectOutsideScope)
        );
    }

    #[test]
    fn missing_or_oversized_scope_results_fail_closed() {
        let result = tree();
        let budget = McpScopeBudget::new(MAX_SCOPE_BYTES + 1);
        assert!(matches!(budget, Err(ScopeError::LimitExceeded)));
        let mut budget = McpScopeBudget::new(0).expect("予算を作る");
        assert!(matches!(
            McpSceneScopeIndex::from_result(&result, Some("missing"), &mut budget),
            Err(ScopeError::ScopeRootUnknown)
        ));
        assert_eq!(
            McpWriteSettings {
                mode: McpWriteMode::Enabled,
                scene_root_id: Some("".to_owned()),
            }
            .validate(),
            Err(ScopeError::InvalidScopeRoot)
        );
    }

    fn redo(records: Vec<HistoryRecord>) -> McpHistoryAction {
        McpHistoryAction {
            direction: McpHistoryDirection::Redo,
            head_id: 1,
            revision: 1,
            group_name: "依存ID".to_owned(),
            source: EditSource::Mcp,
            records,
        }
    }

    fn create(id: &str, parent: &str) -> HistoryRecord {
        HistoryRecord::Create {
            created_id: id.to_owned(),
            parent_id: Some(parent.to_owned()),
            kind: None,
        }
    }

    fn property(id: &str) -> HistoryRecord {
        HistoryRecord::SetProperty {
            object_id: id.to_owned(),
            property: "visible".to_owned(),
            old_value: Value::Bool(false),
            new_value: Value::Bool(true),
        }
    }

    #[test]
    fn redo_allows_only_ordered_recreated_dependencies_inside_scope() {
        let (scoped, _) = index(Some("allowed"));
        let records = vec![
            create("parent", "allowed"),
            HistoryRecord::Duplicate {
                source_object_id: "allowed-child".to_owned(),
                created_id: "copy".to_owned(),
                parent_id: Some("parent".to_owned()),
            },
            property("copy"),
            HistoryRecord::Reparent {
                object_id: "copy".to_owned(),
                old_parent_id: Some("parent".to_owned()),
                new_parent_id: Some("allowed-grandchild".to_owned()),
            },
            create("child", "copy"),
            property("child"),
        ];
        assert_eq!(scoped.check_history(&redo(records.clone())), Ok(()));
        assert!(
            !scoped.contains_node("copy"),
            "認可の投影を実ツリーへ残さない"
        );
        for records in [
            vec![property("parent"), create("parent", "allowed")],
            vec![create("child", "parent"), create("parent", "allowed")],
            vec![create("parent", "allowed"), property("unrelated")],
            vec![
                HistoryRecord::Duplicate {
                    source_object_id: "future".to_owned(),
                    created_id: "copy".to_owned(),
                    parent_id: Some("allowed".to_owned()),
                },
                create("future", "allowed"),
            ],
            vec![create("parent", "outside"), property("parent")],
            vec![create("allowed-child", "allowed")],
            vec![create("parent", "allowed"), create("parent", "allowed")],
        ] {
            assert!(
                scoped.check_history(&redo(records.clone())).is_err(),
                "不正な依存を拒否: {records:?}"
            );
        }
        let mut undo = redo(records);
        undo.direction = McpHistoryDirection::Undo;
        assert!(!undo.recreates_before(undo.records.len(), "parent"));
        assert!(
            scoped.check_history(&undo).is_err(),
            "undoには未来のIDを許可しない"
        );
    }

    #[test]
    fn redo_dependencies_do_not_bypass_existing_targets_or_component_membership() {
        let (mut scoped, _) = index(Some("allowed"));
        for (owner, component) in [("allowed", "component-in"), ("outside", "component-out")] {
            let snapshot = norves_bridge_editor_client::parse_object_snapshot_result(&json!({
                "objectId":owner,"properties":[],"components":[{"objectId":component,"kind":"camera"}]
            })).unwrap();
            scoped.record_component_snapshot(owner, snapshot).unwrap();
        }
        assert_eq!(
            scoped.check_history(&redo(vec![
                create("new", "allowed"),
                property("component-in")
            ])),
            Ok(())
        );
        for last in [
            property("component-out"),
            property("outside-child"),
            HistoryRecord::Duplicate {
                source_object_id: "outside".to_owned(),
                created_id: "copy".to_owned(),
                parent_id: Some("new".to_owned()),
            },
            HistoryRecord::Reparent {
                object_id: "new".to_owned(),
                old_parent_id: Some("outside".to_owned()),
                new_parent_id: Some("allowed".to_owned()),
            },
            HistoryRecord::Reparent {
                object_id: "new".to_owned(),
                old_parent_id: Some("allowed".to_owned()),
                new_parent_id: Some("outside".to_owned()),
            },
            create("component-in", "new"),
        ] {
            assert!(scoped
                .check_history(&redo(vec![create("new", "allowed"), last]))
                .is_err());
        }
        // 部分redo済みのIDは実在対象として改めて検査する。
        assert_eq!(
            scoped.check_history(&redo(vec![property("allowed-child")])),
            Ok(())
        );
        let records = (0..MAX_SCOPE_NODES)
            .map(|i| create(&format!("future-{i}"), "allowed"))
            .collect();
        assert_eq!(
            scoped.check_history(&redo(records)),
            Err(ScopeError::LimitExceeded)
        );
    }

    #[test]
    fn range_budget_enforces_nodes_snapshots_and_aggregate_bytes() {
        let mut budget = McpScopeBudget::new(0).expect("予算を作る");
        budget.add_nodes(MAX_SCOPE_NODES).expect("ノード上限内");
        assert_eq!(budget.add_nodes(1), Err(ScopeError::LimitExceeded));
        for _ in 0..MAX_SCOPE_SNAPSHOTS {
            budget.begin_snapshot().expect("snapshot上限内");
        }
        assert_eq!(budget.snapshot_count(), MAX_SCOPE_SNAPSHOTS);
        assert_eq!(budget.begin_snapshot(), Err(ScopeError::LimitExceeded));
        budget.add_bytes(MAX_SCOPE_BYTES).expect("byte上限ちょうど");
        assert_eq!(budget.add_bytes(1), Err(ScopeError::LimitExceeded));
    }

    #[test]
    fn destructive_operations_and_history_delete_require_confirmation() {
        let delete = McpWriteOperation::Delete {
            object_id: "node".to_owned(),
        };
        let remove = McpWriteOperation::ComponentRemove {
            component_id: "component".to_owned(),
        };
        let undo_create = McpWriteOperation::History(McpHistoryAction {
            direction: McpHistoryDirection::Undo,
            head_id: 1,
            revision: 1,
            group_name: "ユーザー編集".to_owned(),
            source: EditSource::Ui,
            records: vec![HistoryRecord::Create {
                created_id: "node".to_owned(),
                parent_id: None,
                kind: None,
            }],
        });
        assert!(delete.requires_confirmation());
        assert!(remove.requires_confirmation());
        assert!(undo_create.requires_confirmation());
    }

    #[test]
    fn authorization_modes_and_configuration_changes_invalidate_old_permits() {
        let authorization = McpAuthorization::default();
        let operation = McpWriteOperation::Property {
            object_id: "allowed-child".to_owned(),
        };
        let original_lease = authorization.current_lease();
        let read_only = authorization.write_policy_snapshot();
        assert_eq!(
            authorization.check_write_request(&original_lease, &read_only, &operation),
            Err(ScopeError::ReadOnly)
        );

        authorization
            .set_write_settings(McpWriteSettings {
                mode: McpWriteMode::Enabled,
                scene_root_id: None,
            })
            .expect("書き込み可へ変更する");
        assert!(!original_lease.is_current());
        let enabled_lease = authorization.current_lease();
        let enabled = authorization.write_policy_snapshot();
        authorization
            .check_write_request(&enabled_lease, &enabled, &operation)
            .expect("範囲検査前のwrite enabledを通す");
        let permit = authorization
            .issue_write_permit(&enabled_lease, &enabled, 4, operation.clone())
            .expect("許可されたpropertyのpermitを作る");
        assert_eq!(
            authorization.issue_write_permit(
                &enabled_lease,
                &enabled,
                4,
                McpWriteOperation::Delete {
                    object_id: "allowed-child".to_owned(),
                }
            ),
            Err(ScopeError::ConfirmationRequired)
        );
        authorization
            .validate_write_permit(&permit, &enabled_lease, 4, &operation)
            .expect("現行接続・設定のpermitを受け付ける");

        authorization
            .set_write_settings(McpWriteSettings {
                mode: McpWriteMode::Confirm,
                scene_root_id: Some("allowed".to_owned()),
            })
            .expect("確認モードへ変更する");
        assert!(!enabled_lease.is_current());
        assert_eq!(
            authorization.validate_write_permit(&permit, &enabled_lease, 4, &operation),
            Err(ScopeError::AuthorizationRevoked)
        );
        let confirm_lease = authorization.current_lease();
        let confirm = authorization.write_policy_snapshot();
        assert_eq!(
            authorization.issue_write_permit(&confirm_lease, &confirm, 4, operation),
            Err(ScopeError::ConfirmationRequired)
        );
    }

    #[tokio::test]
    async fn scope_deadline_returns_a_closed_error() {
        let outcome = with_scope_deadline_for(
            tokio::time::sleep(Duration::from_millis(50)),
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(outcome, Err(ScopeError::DeadlineExceeded));
    }
}
