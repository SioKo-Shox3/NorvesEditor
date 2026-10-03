//! 接続能力・許可とBridge仕様から、公開可能なMCP道具と入力schemaを作る。

use norves_bridge_core::CapabilityDescriptor;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::sync::watch;

const COMMON_SCHEMA_ID: &str = "https://norveseditor.dev/bridge/spec/schema/common.schema.json";
const COMMON_SCHEMA: &str = include_str!("../../../../../bridge/spec/schema/common.schema.json");

#[derive(Clone)]
pub(crate) struct McpToolCatalog {
    inner: Arc<CatalogInner>,
}

struct CatalogInner {
    state: Mutex<CatalogState>,
    revision_tx: watch::Sender<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogState {
    generation: Option<u64>,
    capabilities: HashSet<String>,
    permission: WritePermission,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WritePermission {
    #[default]
    ReadOnly,
    Enabled,
    Confirm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolAccess {
    Read,
    Write,
}

#[derive(Clone, Copy)]
enum SchemaSource {
    Bridge(&'static str),
    Custom(&'static str),
}

#[derive(Clone, Copy)]
struct ToolSpec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    access: ToolAccess,
    capabilities: &'static [&'static str],
    input: SchemaSource,
    bridge_write: bool,
    destructive: bool,
}

const TREE_PAGE_SCHEMA: &str = r#"{
    "type":"object",
    "properties":{
        "rootId":{"type":"string","minLength":1},
        "maxDepth":{"type":"integer","minimum":0},
        "pageSize":{"type":"integer","minimum":1,"maximum":200},
        "cursor":{"type":"string","minLength":1,"maxLength":256}
    },
    "additionalProperties":false
}"#;
const SNAPSHOT_PAGE_SCHEMA: &str = r#"{
    "type":"object",
    "required":["objectId"],
    "properties":{
        "objectId":{"type":"string","minLength":1},
        "pageSize":{"type":"integer","minimum":1,"maximum":200},
        "cursor":{"type":"string","minLength":1,"maxLength":256}
    },
    "additionalProperties":false
}"#;
const SIMPLE_PAGE_SCHEMA: &str = r#"{
    "type":"object",
    "properties":{
        "pageSize":{"type":"integer","minimum":1,"maximum":200},
        "cursor":{"type":"string","minLength":1,"maxLength":256}
    },
    "additionalProperties":false
}"#;
const EMPTY_INPUT_SCHEMA: &str = r#"{
    "type":"object",
    "properties":{},
    "additionalProperties":false
}"#;
const ASSET_MANIFEST_PAGE_SCHEMA: &str = r#"{
    "type":"object",
    "properties":{
        "filter":{"type":"string"},
        "pageSize":{"type":"integer","minimum":1,"maximum":200},
        "cursor":{"type":"string","minLength":1,"maxLength":256}
    },
    "additionalProperties":false
}"#;
const LOGS_PAGE_SCHEMA: &str = r#"{
    "type":"object",
    "properties":{
        "generation":{"type":"integer","minimum":0},
        "afterSequence":{"type":"integer","minimum":0},
        "limit":{"type":"integer","minimum":1,"maximum":1000},
        "pageSize":{"type":"integer","minimum":1,"maximum":200},
        "cursor":{"type":"string","minLength":1,"maxLength":256}
    },
    "additionalProperties":false
}"#;

fn tool_specs() -> &'static [ToolSpec] {
    static SPECS: OnceLock<Vec<ToolSpec>> = OnceLock::new();
    SPECS
        .get_or_init(|| {
            vec![
    bridge_tool(
        "engine_get_status",
        "エンジンの状態を取得する",
        "現在接続中のエンジン状態を読み取ります。",
        "engine.getStatus",
        ToolAccess::Read,
        &[],
        false,
    ),
    custom_tool(
        "viewport_get_thumbnail",
        "Game Viewの画像を取得する",
        "接続中のGame ViewをPNG画像として取得します。",
        ToolAccess::Read,
        &["viewport.thumbnail"],
        EMPTY_INPUT_SCHEMA,
    ),
    custom_tool(
        "bridge_get_capabilities",
        "接続能力を取得する",
        "接続中のエンジンが広告した能力を上限付きのページで読み取ります。",
        ToolAccess::Read,
        &[],
        SIMPLE_PAGE_SCHEMA,
    ),
    custom_tool(
        "scene_get_tree",
        "シーンのツリーを取得する",
        "シーン階層を深さ優先の平坦な項目として、上限付きで読み取ります。",
        ToolAccess::Read,
        &["scene.query"],
        TREE_PAGE_SCHEMA,
    ),
    custom_tool(
        "scene_get_tree_page",
        "シーンのツリーをページ単位で取得する",
        "シーン階層を上限付きのページで読み取ります。",
        ToolAccess::Read,
        &["scene.query"],
        TREE_PAGE_SCHEMA,
    ),
    custom_tool(
        "object_get_snapshot",
        "オブジェクトのsnapshotを取得する",
        "オブジェクトの値と構成要素を上限付きのページで読み取ります。",
        ToolAccess::Read,
        &["object.query"],
        SNAPSHOT_PAGE_SCHEMA,
    ),
    custom_tool(
        "schema_get_snapshot",
        "型のschemaを取得する",
        "接続中のエンジンが公開する型とプロパティ定義を上限付きのページで読み取ります。",
        ToolAccess::Read,
        &["object.query"],
        SIMPLE_PAGE_SCHEMA,
    ),
    custom_tool(
        "asset_get_manifest",
        "読み込み済み資産manifestを取得する",
        "エンジンが読み込んだ資産manifestを上限付きのページで読み取ります。",
        ToolAccess::Read,
        &["asset.read"],
        ASSET_MANIFEST_PAGE_SCHEMA,
    ),
    bridge_tool(
        "asset_resolve",
        "資産パスを解決する",
        "論理資産パスの解決状態を取得します。",
        "asset.resolve",
        ToolAccess::Read,
        &["asset.read"],
        false,
    ),
    custom_tool(
        "logs_get_recent",
        "最近のエンジンログを取得する",
        "保持されているエンジンログを世代と連番で絞って取得します。",
        ToolAccess::Read,
        &[],
        LOGS_PAGE_SCHEMA,
    ),
    bridge_tool(
        "object_set_property",
        "オブジェクトのプロパティを設定する",
        "共通編集列を通してオブジェクトのプロパティを設定します。",
        "object.setProperty",
        ToolAccess::Write,
        &["object.edit", "object.query"],
        false,
    ),
    bridge_tool(
        "scene_create_object",
        "シーンにオブジェクトを作成する",
        "共通編集列を通してシーンにオブジェクトを作成します。",
        "scene.createObject",
        ToolAccess::Write,
        &["scene.edit"],
        false,
    ),
    bridge_tool(
        "scene_delete_object",
        "シーンからオブジェクトを削除する",
        "共通編集列を通してオブジェクトと子孫を削除します。",
        "scene.deleteObject",
        ToolAccess::Write,
        &["scene.edit", "scene.query"],
        true,
    ),
    bridge_tool(
        "scene_reparent_object",
        "シーン内の親を変更する",
        "共通編集列を通してオブジェクトの親を変更します。",
        "scene.reparentObject",
        ToolAccess::Write,
        &["scene.edit", "scene.query"],
        false,
    ),
    bridge_tool(
        "scene_duplicate_object",
        "シーン内のオブジェクトを複製する",
        "共通編集列を通してオブジェクトを複製します。",
        "scene.duplicateObject",
        ToolAccess::Write,
        &["scene.edit", "scene.query"],
        false,
    ),
    bridge_tool(
        "component_add",
        "オブジェクトへcomponentを追加する",
        "共通編集列を通して型schemaにあるcomponentを追加します。",
        "component.add",
        ToolAccess::Write,
        &["component.edit", "object.query"],
        false,
    ),
    bridge_tool(
        "component_remove",
        "オブジェクトからcomponentを外す",
        "共通編集列を通してcomponentを外します。",
        "component.remove",
        ToolAccess::Write,
        &["component.edit", "object.query"],
        true,
    ),
    bridge_tool(
        "runtime_play",
        "エンジンを再生する",
        "許可を確認してエンジンを再生します。",
        "runtime.play",
        ToolAccess::Write,
        &["runtime.control"],
        false,
    ),
    bridge_tool(
        "runtime_pause",
        "エンジンを一時停止する",
        "許可を確認してエンジンを一時停止します。",
        "runtime.pause",
        ToolAccess::Write,
        &["runtime.control"],
        false,
    ),
    bridge_tool(
        "runtime_stop",
        "エンジンを停止する",
        "許可を確認してエンジンを停止します。",
        "runtime.stop",
        ToolAccess::Write,
        &["runtime.control"],
        false,
    ),
    custom_tool(
        "edit_begin_group",
        "名前付き編集まとまりを開始する",
        "複数の編集をまとめるためのまとまりを開始します。",
        ToolAccess::Write,
        &["object.edit", "scene.edit", "component.edit"],
        r#"{
            "type":"object",
            "required":["name"],
            "properties":{"name":{"type":"string","minLength":1,"maxLength":128}},
            "additionalProperties":false
        }"#,
    ),
    custom_tool(
        "edit_end_group",
        "名前付き編集まとまりを終了する",
        "groupIdで指定した名前付き編集まとまりを終了します。",
        ToolAccess::Write,
        &["object.edit", "scene.edit", "component.edit"],
        r#"{
            "type":"object",
            "required":["groupId"],
            "properties":{"groupId":{"type":"string","minLength":1,"maxLength":128}},
            "additionalProperties":false
        }"#,
    ),
    ]
        })
        .as_slice()
}

impl Default for McpToolCatalog {
    fn default() -> Self {
        let (revision_tx, _) = watch::channel(0);
        Self {
            inner: Arc::new(CatalogInner {
                state: Mutex::new(CatalogState {
                    generation: None,
                    capabilities: HashSet::new(),
                    permission: WritePermission::ReadOnly,
                }),
                revision_tx,
            }),
        }
    }
}

impl McpToolCatalog {
    pub(crate) fn set_connection(
        &self,
        generation: Option<u64>,
        capabilities: &[CapabilityDescriptor],
    ) {
        let mut state = self.lock_state();
        let next = CatalogState {
            generation,
            capabilities: capabilities
                .iter()
                .map(|capability| capability.name.as_str().to_owned())
                .collect(),
            permission: state.permission,
        };
        if *state == next {
            return;
        }
        *state = next;
        drop(state);
        self.notify_changed();
    }

    #[allow(dead_code)]
    pub(crate) fn set_write_permission(&self, permission: WritePermission) {
        let mut state = self.lock_state();
        if state.permission == permission {
            return;
        }
        state.permission = permission;
        drop(state);
        self.notify_changed();
    }

    pub(crate) fn current_generation(&self) -> Option<u64> {
        self.snapshot().generation
    }

    pub(crate) fn list(&self) -> Vec<Tool> {
        let state = self.snapshot();
        if state.generation.is_none() {
            return Vec::new();
        }
        tool_specs()
            .iter()
            .filter(|spec| is_exposed(spec, &state))
            .filter_map(|spec| build_tool(spec).ok())
            .collect()
    }

    pub(crate) fn get(&self, name: &str) -> Option<Tool> {
        let state = self.snapshot();
        state.generation?;
        tool_specs()
            .iter()
            .find(|spec| spec.name == name && is_exposed(spec, &state))
            .and_then(|spec| build_tool(spec).ok())
    }

    pub(crate) fn validate_call(
        &self,
        name: &str,
        arguments: &Value,
    ) -> Result<ValidatedToolInput, ToolInputError> {
        let state = self.snapshot();
        let spec = tool_specs()
            .iter()
            .find(|spec| spec.name == name && is_exposed(spec, &state))
            .ok_or(ToolInputError::Unavailable)?;
        let schema = input_schema(spec).map_err(|_| ToolInputError::Schema)?;
        if !schema_is_valid(&schema, arguments, &schema) {
            return Err(ToolInputError::Invalid);
        }

        match (spec.input, spec.bridge_write) {
            (SchemaSource::Bridge(_), true) => {
                let object = arguments.as_object().ok_or(ToolInputError::Invalid)?;
                let params = object
                    .get("params")
                    .cloned()
                    .ok_or(ToolInputError::Invalid)?;
                let group_id = object
                    .get("groupId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Ok(ValidatedToolInput::BridgeWrite { params, group_id })
            }
            (SchemaSource::Bridge(_), false) => {
                Ok(ValidatedToolInput::BridgeRead(arguments.clone()))
            }
            (SchemaSource::Custom(_), _) => Ok(ValidatedToolInput::Custom(arguments.clone())),
        }
    }

    fn snapshot(&self) -> CatalogState {
        self.lock_state().clone()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, CatalogState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn notify_changed(&self) {
        let revision = *self.inner.revision_tx.borrow();
        self.inner
            .revision_tx
            .send_replace(revision.wrapping_add(1));
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum ValidatedToolInput {
    BridgeRead(Value),
    BridgeWrite {
        params: Value,
        group_id: Option<String>,
    },
    Custom(Value),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolInputError {
    Unavailable,
    Invalid,
    Schema,
}

fn bridge_tool(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    method: &'static str,
    access: ToolAccess,
    capabilities: &'static [&'static str],
    destructive: bool,
) -> ToolSpec {
    ToolSpec {
        name,
        title,
        description,
        access,
        capabilities,
        input: SchemaSource::Bridge(method_schema(method)),
        bridge_write: access == ToolAccess::Write,
        destructive,
    }
}

fn custom_tool(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    access: ToolAccess,
    capabilities: &'static [&'static str],
    schema: &'static str,
) -> ToolSpec {
    ToolSpec {
        name,
        title,
        description,
        access,
        capabilities,
        input: SchemaSource::Custom(schema),
        bridge_write: false,
        destructive: false,
    }
}

fn method_schema(method: &str) -> &'static str {
    match method {
        "engine.getStatus" => include_str!(
            "../../../../../bridge/spec/schema/methods/engine.getStatus.params.schema.json"
        ),
        "bridge.getCapabilities" => include_str!(
            "../../../../../bridge/spec/schema/methods/bridge.getCapabilities.params.schema.json"
        ),
        "scene.getTree" => include_str!(
            "../../../../../bridge/spec/schema/methods/scene.getTree.params.schema.json"
        ),
        "object.getSnapshot" => include_str!(
            "../../../../../bridge/spec/schema/methods/object.getSnapshot.params.schema.json"
        ),
        "schema.getSnapshot" => include_str!(
            "../../../../../bridge/spec/schema/methods/schema.getSnapshot.params.schema.json"
        ),
        "asset.getManifest" => include_str!(
            "../../../../../bridge/spec/schema/methods/asset.getManifest.params.schema.json"
        ),
        "asset.resolve" => include_str!(
            "../../../../../bridge/spec/schema/methods/asset.resolve.params.schema.json"
        ),
        "viewport.getThumbnail" => include_str!(
            "../../../../../bridge/spec/schema/methods/viewport.getThumbnail.params.schema.json"
        ),
        "object.setProperty" => include_str!(
            "../../../../../bridge/spec/schema/methods/object.setProperty.params.schema.json"
        ),
        "scene.createObject" => include_str!(
            "../../../../../bridge/spec/schema/methods/scene.createObject.params.schema.json"
        ),
        "scene.deleteObject" => include_str!(
            "../../../../../bridge/spec/schema/methods/scene.deleteObject.params.schema.json"
        ),
        "scene.reparentObject" => include_str!(
            "../../../../../bridge/spec/schema/methods/scene.reparentObject.params.schema.json"
        ),
        "scene.duplicateObject" => include_str!(
            "../../../../../bridge/spec/schema/methods/scene.duplicateObject.params.schema.json"
        ),
        "component.add" => include_str!(
            "../../../../../bridge/spec/schema/methods/component.add.params.schema.json"
        ),
        "component.remove" => include_str!(
            "../../../../../bridge/spec/schema/methods/component.remove.params.schema.json"
        ),
        "runtime.play" => include_str!(
            "../../../../../bridge/spec/schema/methods/runtime.play.params.schema.json"
        ),
        "runtime.pause" => include_str!(
            "../../../../../bridge/spec/schema/methods/runtime.pause.params.schema.json"
        ),
        "runtime.stop" => include_str!(
            "../../../../../bridge/spec/schema/methods/runtime.stop.params.schema.json"
        ),
        _ => unreachable!("Bridge tool method has a registered embedded schema"),
    }
}

fn is_exposed(spec: &ToolSpec, state: &CatalogState) -> bool {
    spec.capabilities
        .iter()
        .all(|required| state.capabilities.contains(*required))
        && (spec.access == ToolAccess::Read || state.permission != WritePermission::ReadOnly)
}

fn build_tool(spec: &ToolSpec) -> Result<Tool, String> {
    let schema = input_schema(spec)?;
    let schema = schema
        .as_object()
        .cloned()
        .ok_or_else(|| "MCP input schema root is not an object".to_owned())?;
    let annotation = ToolAnnotations::from_raw(
        Some(spec.title.to_owned()),
        Some(spec.access == ToolAccess::Read),
        Some(spec.destructive),
        Some(false),
        Some(false),
    );
    Ok(Tool::new(spec.name, spec.description, Arc::new(schema))
        .with_title(spec.title)
        .with_annotations(annotation))
}

fn input_schema(spec: &ToolSpec) -> Result<Value, String> {
    let (source, bridge_write) = match spec.input {
        SchemaSource::Bridge(source) => {
            let params = serde_json::from_str::<Value>(source)
                .map_err(|error| format!("埋め込みBridge schemaを読めません: {error}"))?;
            (resolve_embedded_references(params)?, spec.bridge_write)
        }
        SchemaSource::Custom(source) => (
            serde_json::from_str::<Value>(source)
                .map_err(|error| format!("独自schemaを読めません: {error}"))?,
            false,
        ),
    };
    if !bridge_write {
        return Ok(source);
    }
    Ok(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["params"],
        "properties": {
            "params": source,
            "groupId": {"type":"string","minLength":1,"maxLength":128}
        },
        "additionalProperties": false
    }))
}

fn resolve_embedded_references(schema: Value) -> Result<Value, String> {
    let common = serde_json::from_str::<Value>(COMMON_SCHEMA)
        .map_err(|error| format!("共通Bridge schemaを読めません: {error}"))?;
    let common_definitions = common
        .get("$defs")
        .and_then(Value::as_object)
        .ok_or_else(|| "共通Bridge schemaに$defsがありません。".to_owned())?;
    let mut resolved = schema;
    let object = resolved
        .as_object_mut()
        .ok_or_else(|| "Bridge input schemaのrootがobjectではありません。".to_owned())?;
    let definitions = object
        .entry("$defs".to_owned())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "Bridge input schemaの$defsがobjectではありません。".to_owned())?;
    for (name, definition) in common_definitions {
        definitions
            .entry(name.clone())
            .or_insert_with(|| definition.clone());
    }
    rewrite_embedded_references(&mut resolved)?;
    Ok(resolved)
}

fn rewrite_embedded_references(value: &mut Value) -> Result<(), String> {
    match value {
        Value::Array(items) => {
            for item in items {
                rewrite_embedded_references(item)?;
            }
        }
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref") {
                let reference = reference
                    .as_str()
                    .ok_or_else(|| "schema $refは文字列ではありません。".to_owned())?
                    .to_owned();
                if let Some(fragment) = reference.strip_prefix(COMMON_SCHEMA_ID) {
                    let local = if fragment.starts_with('#') {
                        fragment.to_owned()
                    } else {
                        format!("#{fragment}")
                    };
                    object.insert("$ref".to_owned(), Value::String(local));
                } else if !reference.starts_with('#') {
                    return Err("埋め込み外のschema参照は使えません。".to_owned());
                }
            }
            for child in object.values_mut() {
                rewrite_embedded_references(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn schema_is_valid(schema: &Value, value: &Value, root: &Value) -> bool {
    let root = if schema.get("$id").is_some() {
        schema
    } else {
        root
    };
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let Some(target) = reference
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer))
        else {
            return false;
        };
        return schema_is_valid(target, value, root);
    }

    if let Some(required_type) = schema.get("type") {
        let matches_type = match required_type {
            Value::String(kind) => value_matches_type(value, kind),
            Value::Array(kinds) => kinds
                .iter()
                .filter_map(Value::as_str)
                .any(|kind| value_matches_type(value, kind)),
            _ => false,
        };
        if !matches_type {
            return false;
        }
    }

    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            return false;
        }
    }

    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            if required
                .iter()
                .filter_map(Value::as_str)
                .any(|key| !object.contains_key(key))
            {
                return false;
            }
        }
        if schema.get("additionalProperties") == Some(&Value::Bool(false))
            && properties
                .is_some_and(|properties| object.keys().any(|key| !properties.contains_key(key)))
        {
            return false;
        }
        if let Some(properties) = properties {
            for (key, property_schema) in properties {
                if let Some(property) = object.get(key) {
                    if !schema_is_valid(property_schema, property, root) {
                        return false;
                    }
                }
            }
        }
    }

    if let Some(items) = value.as_array() {
        if schema
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|minimum| items.len() < minimum as usize)
            || schema
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|maximum| items.len() > maximum as usize)
        {
            return false;
        }
        if let Some(item_schema) = schema.get("items") {
            if items
                .iter()
                .any(|item| !schema_is_valid(item_schema, item, root))
            {
                return false;
            }
        }
    }

    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if schema
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|minimum| length < minimum)
            || schema
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|maximum| length > maximum)
        {
            return false;
        }
    }

    if let Some(number) = value.as_f64() {
        if schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|minimum| number < minimum)
            || schema
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|maximum| number > maximum)
        {
            return false;
        }
    }

    for (keyword, expected) in [("allOf", 1usize), ("anyOf", 2usize), ("oneOf", 3usize)] {
        if let Some(schemas) = schema.get(keyword).and_then(Value::as_array) {
            let valid = schemas
                .iter()
                .filter(|subschema| schema_is_valid(subschema, value, root))
                .count();
            if (expected == 1 && valid != schemas.len())
                || (expected == 2 && valid == 0)
                || (expected == 3 && valid != 1)
            {
                return false;
            }
        }
    }
    true
}

fn value_matches_type(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(name: &str) -> CapabilityDescriptor {
        serde_json::from_value(json!({"name": name})).expect("能力descriptorを作る")
    }

    fn connected_catalog(capabilities: &[&str]) -> McpToolCatalog {
        let catalog = McpToolCatalog::default();
        let capabilities = capabilities
            .iter()
            .map(|name| descriptor(name))
            .collect::<Vec<_>>();
        catalog.set_connection(Some(1), &capabilities);
        catalog
    }

    #[test]
    fn write_tools_require_connection_capabilities_and_non_read_only_permission() {
        let catalog = connected_catalog(&["object.edit"]);
        assert!(catalog.get("object_set_property").is_none());
        catalog.set_connection(
            Some(2),
            &[descriptor("object.edit"), descriptor("object.query")],
        );
        assert!(catalog.get("object_set_property").is_none());
        catalog.set_write_permission(WritePermission::Enabled);
        assert!(catalog.get("object_set_property").is_some());
        catalog.set_connection(None, &[]);
        assert!(catalog.get("object_set_property").is_none());
        assert!(catalog.list().is_empty());
    }

    #[test]
    fn bridge_tool_schema_resolves_absolute_ids_and_fragment_references_offline() {
        let catalog = connected_catalog(&["object.query", "object.edit"]);
        catalog.set_write_permission(WritePermission::Enabled);
        let tool = catalog
            .get("object_set_property")
            .expect("必要能力がそろった道具を得る");
        let schema = Value::Object((*tool.input_schema).clone());
        let params_schema = schema
            .pointer("/properties/params")
            .expect("params schemaがある");
        assert_eq!(params_schema["$id"], "https://norveseditor.dev/bridge/spec/schema/methods/object.setProperty.params.schema.json");
        assert_eq!(
            params_schema.pointer("/$defs/objectId/type"),
            Some(&json!("string"))
        );
        assert_eq!(
            params_schema.pointer("/properties/objectId/$ref"),
            Some(&json!("#/$defs/objectId"))
        );
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema_is_valid(
            &schema,
            &json!({
                "params":{"objectId":"node-1","property":"visible","value":true}
            }),
            &schema
        ));

        let unresolved = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "$ref":"https://outside.example/schema.json"
        });
        assert!(resolve_embedded_references(unresolved).is_err());
    }

    #[test]
    fn write_wrapper_validates_params_and_group_id_separately() {
        let catalog = connected_catalog(&["object.edit", "object.query"]);
        catalog.set_write_permission(WritePermission::Enabled);
        let valid = json!({
            "params":{"objectId":"node-1","property":"visible","value":true},
            "groupId":"opaque-group"
        });
        assert_eq!(
            catalog.validate_call("object_set_property", &valid),
            Ok(ValidatedToolInput::BridgeWrite {
                params: json!({"objectId":"node-1","property":"visible","value":true}),
                group_id: Some("opaque-group".to_owned()),
            })
        );
        for invalid in [
            json!({"params":{"objectId":"node-1","property":"visible","value":true,"unknown":1}}),
            json!({"params":{"objectId":"node-1","property":2,"value":true}}),
            json!({"params":{"objectId":"node-1","property":"visible","value":true},"unexpected":true}),
            json!({"params":{"objectId":"node-1","property":"visible","value":true},"groupId":"x".repeat(129)}),
        ] {
            assert_eq!(
                catalog.validate_call("object_set_property", &invalid),
                Err(ToolInputError::Invalid)
            );
        }
        let unavailable = McpToolCatalog::default().validate_call("object_set_property", &valid);
        assert_eq!(unavailable, Err(ToolInputError::Unavailable));
    }

    #[test]
    fn custom_tools_have_independent_schemas_and_limits() {
        let catalog = connected_catalog(&[
            "scene.query",
            "asset.read",
            "object.edit",
            "scene.edit",
            "component.edit",
        ]);
        catalog.set_write_permission(WritePermission::Enabled);
        let page_schema = Value::Object(
            catalog
                .get("scene_get_tree_page")
                .expect("scene.queryでページ道具が見える")
                .input_schema
                .as_ref()
                .clone(),
        );
        assert!(schema_is_valid(
            &page_schema,
            &json!({"pageSize":200}),
            &page_schema
        ));
        assert!(!schema_is_valid(
            &page_schema,
            &json!({"pageSize":201}),
            &page_schema
        ));
        let manifest_schema = Value::Object(
            catalog
                .get("asset_get_manifest")
                .expect("asset.readでmanifest道具が見える")
                .input_schema
                .as_ref()
                .clone(),
        );
        assert!(schema_is_valid(
            &manifest_schema,
            &json!({"filter":"texture","pageSize":200}),
            &manifest_schema
        ));
        assert!(!schema_is_valid(
            &manifest_schema,
            &json!({"pageSize":201}),
            &manifest_schema
        ));
        assert!(!schema_is_valid(
            &manifest_schema,
            &json!({"page":0}),
            &manifest_schema
        ));
        assert_eq!(
            catalog.validate_call("asset_get_manifest", &json!({"page":0})),
            Err(ToolInputError::Invalid)
        );
        assert!(catalog.get("edit_begin_group").is_some());
        assert!(catalog.get("edit_end_group").is_some());
        assert!(catalog
            .validate_call("edit_begin_group", &json!({"name":"scene setup"}))
            .is_ok());
        assert_eq!(
            catalog.validate_call("edit_begin_group", &json!({"params":{}})),
            Err(ToolInputError::Invalid)
        );
    }

    #[test]
    fn embedded_params_validation_matches_resolved_params_for_bridge_samples() {
        let spec = serde_json::from_str::<Value>(method_schema("object.setProperty"))
            .expect("Bridge params schemaを読む");
        let resolved = resolve_embedded_references(spec.clone()).expect("参照を解決する");
        for sample in [
            json!({"objectId":"node-1","property":"visible","value":true}),
            json!({"objectId":"node-1","property":"visible","value":true,"extra":1}),
            json!({"objectId":"","property":"visible","value":true}),
            json!({"objectId":"node-1","property":"visible","value":null}),
        ] {
            let resolved_valid = schema_is_valid(&resolved, &sample, &resolved);
            assert_eq!(
                resolved_valid,
                matches!(sample["objectId"].as_str(), Some(id) if !id.is_empty())
                    && sample["property"].is_string()
                    && sample.get("extra").is_none()
                    && (sample["value"].is_string()
                        || sample["value"].is_number()
                        || sample["value"].is_boolean()
                        || sample["value"].is_null()
                        || sample["value"].is_array()
                        || sample["value"].is_object()),
                "schema参照解決後の判定が期待と異なります: {sample}"
            );
        }
    }

    #[test]
    fn local_read_tools_are_hidden_until_a_bridge_generation_exists() {
        let catalog = McpToolCatalog::default();
        assert!(catalog.get("engine_get_status").is_none());
        catalog.set_connection(Some(1), &[]);
        assert!(catalog.get("engine_get_status").is_some());
        assert!(catalog.get("scene_get_tree").is_none());
    }

    #[test]
    fn thumbnail_tool_requires_engine_capability_and_has_no_input_arguments() {
        let catalog = connected_catalog(&[]);
        assert!(catalog.get("viewport_get_thumbnail").is_none());

        catalog.set_connection(Some(2), &[descriptor("viewport.thumbnail")]);
        assert!(catalog.get("viewport_get_thumbnail").is_some());
        assert!(catalog
            .validate_call("viewport_get_thumbnail", &json!({}))
            .is_ok());
        assert_eq!(
            catalog.validate_call("viewport_get_thumbnail", &json!({"maxWidth":640})),
            Err(ToolInputError::Invalid)
        );
    }
}
