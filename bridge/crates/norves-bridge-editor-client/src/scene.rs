//! `scene.getTree` result domain types (sans-I/O).
//!
//! Extracts a [`SceneTree`] from the `result` value of a `scene.getTree`
//! response. Wire shape follows `scene.getTree.result.schema.json`: a single
//! required `root` node whose shape is the recursive `sceneNode` $def from
//! `common.schema.json` (`id` required; `name`, `kind`, `children` optional).
//!
//! This is the drift-guard used by the `scene_get_tree` Tauri command: it
//! validates the wire shape so a malformed engine result surfaces as a clean
//! backend error rather than being forwarded blindly, while the command still
//! returns the ORIGINAL wire `Value` (no re-modeling round-trip). The types here
//! are generic — `id`/`name`/`kind` carry no engine-specific semantics.

use serde_json::{Map, Value};

/// A serialized scene-tree snapshot extracted from a `scene.getTree` result.
///
/// A DTO copy, never a live engine pointer. The tree is rooted at a single
/// [`SceneNode`].
#[derive(Debug, Clone, PartialEq)]
pub struct SceneTree {
    /// Root node of the snapshotted (sub)tree.
    pub root: SceneNode,
}

/// One recursive node in a scene tree snapshot.
///
/// Mirrors the generic `sceneNode` $def: `id` is the only required field;
/// `name`, `kind`, and `children` are optional. `children` absent or empty
/// means a leaf.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneNode {
    /// Opaque identifier of this node (non-empty on the wire).
    pub id: String,
    /// Optional human-readable node name.
    pub name: Option<String>,
    /// Optional generic node classification (free-form).
    pub kind: Option<String>,
    /// Child nodes; empty when this is a leaf.
    pub children: Vec<SceneNode>,
}

/// Failure while extracting a [`SceneTree`] from a result value.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    /// A required field was missing.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// A field was present but had the wrong type.
    #[error("invalid field: {0}")]
    InvalidField(String),

    /// The result payload did not have the expected JSON shape.
    #[error("unexpected payload shape: {0}")]
    UnexpectedShape(String),
}

/// Extracts a [`SceneTree`] from a `scene.getTree` `result` value.
///
/// Required: `root`, itself a valid `sceneNode`. Validates the whole tree
/// recursively so a malformed descendant is rejected at the boundary.
pub fn parse_scene_tree_result(result: &Value) -> Result<SceneTree, SceneError> {
    let obj = result
        .as_object()
        .ok_or_else(|| SceneError::UnexpectedShape("scene result is not an object".to_owned()))?;

    let root_value = obj
        .get("root")
        .ok_or_else(|| SceneError::MissingField("root".to_owned()))?;
    let root = parse_scene_node(root_value, "root")?;

    Ok(SceneTree { root })
}

/// Parses one `sceneNode`. `path` names the field for error messages (e.g.
/// `root` or `root.children[0]`).
fn parse_scene_node(value: &Value, path: &str) -> Result<SceneNode, SceneError> {
    let obj = value
        .as_object()
        .ok_or_else(|| SceneError::InvalidField(format!("{path} is not an object")))?;

    let id = required_str(obj, "id", path)?.to_owned();
    let name = optional_str(obj, "name", path)?.map(str::to_owned);
    let kind = optional_str(obj, "kind", path)?.map(str::to_owned);
    let children = parse_children(obj, path)?;

    Ok(SceneNode {
        id,
        name,
        kind,
        children,
    })
}

/// Parses the optional `children` array, recursing into each entry.
fn parse_children(obj: &Map<String, Value>, path: &str) -> Result<Vec<SceneNode>, SceneError> {
    match obj.get("children") {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut children = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                children.push(parse_scene_node(
                    item,
                    &format!("{path}.children[{index}]"),
                )?);
            }
            Ok(children)
        }
        Some(_) => Err(SceneError::InvalidField(format!("{path}.children"))),
    }
}

/// Returns the required string at `key`, erroring if absent or not a string.
fn required_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a str, SceneError> {
    let value = obj
        .get(key)
        .ok_or_else(|| SceneError::MissingField(format!("{path}.{key}")))?;
    value
        .as_str()
        .ok_or_else(|| SceneError::InvalidField(format!("{path}.{key}")))
}

/// Returns the string at `key` if present; errors if present but not a string.
fn optional_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<&'a str>, SceneError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| SceneError::InvalidField(format!("{path}.{key}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scene_tree_result_extracts_nested_tree() {
        // Mirrors fixtures/methods/scene.getTree/positive/response-valid.json result.
        let value = serde_json::json!({
            "root": {
                "id": "n-0",
                "name": "Root",
                "kind": "object",
                "children": [
                    { "id": "n-1", "name": "NodeA", "kind": "object" },
                    {
                        "id": "n-2",
                        "name": "GroupNode",
                        "kind": "object",
                        "children": [ { "id": "n-3", "name": "NodeB" } ]
                    }
                ]
            }
        });
        let tree = parse_scene_tree_result(&value).expect("parses");
        assert_eq!(tree.root.id, "n-0");
        assert_eq!(tree.root.name.as_deref(), Some("Root"));
        assert_eq!(tree.root.kind.as_deref(), Some("object"));
        assert_eq!(tree.root.children.len(), 2);

        let node_a = &tree.root.children[0];
        assert_eq!(node_a.id, "n-1");
        assert_eq!(node_a.name.as_deref(), Some("NodeA"));
        assert!(node_a.children.is_empty());

        let group = &tree.root.children[1];
        assert_eq!(group.id, "n-2");
        assert_eq!(group.children.len(), 1);
        let node_b = &group.children[0];
        assert_eq!(node_b.id, "n-3");
        assert_eq!(node_b.name.as_deref(), Some("NodeB"));
        assert_eq!(node_b.kind, None);
        assert!(node_b.children.is_empty());
    }

    #[test]
    fn parse_scene_tree_result_minimal_root_only() {
        let value = serde_json::json!({ "root": { "id": "only" } });
        let tree = parse_scene_tree_result(&value).expect("parses");
        assert_eq!(tree.root.id, "only");
        assert_eq!(tree.root.name, None);
        assert_eq!(tree.root.kind, None);
        assert!(tree.root.children.is_empty());
    }

    #[test]
    fn parse_scene_tree_result_missing_root_errors() {
        let value = serde_json::json!({});
        assert!(matches!(
            parse_scene_tree_result(&value),
            Err(SceneError::MissingField(f)) if f == "root"
        ));
    }

    #[test]
    fn parse_scene_tree_result_root_missing_id_errors() {
        let value = serde_json::json!({ "root": { "name": "no id" } });
        assert!(matches!(
            parse_scene_tree_result(&value),
            Err(SceneError::MissingField(f)) if f == "root.id"
        ));
    }

    #[test]
    fn parse_scene_tree_result_child_missing_id_errors() {
        let value = serde_json::json!({
            "root": { "id": "n-0", "children": [ { "name": "bad" } ] }
        });
        assert!(matches!(
            parse_scene_tree_result(&value),
            Err(SceneError::MissingField(f)) if f == "root.children[0].id"
        ));
    }

    #[test]
    fn parse_scene_tree_result_non_object_root_errors() {
        let value = serde_json::json!({ "root": "not-an-object" });
        assert!(matches!(
            parse_scene_tree_result(&value),
            Err(SceneError::InvalidField(_))
        ));
    }

    #[test]
    fn parse_scene_tree_result_non_array_children_errors() {
        let value = serde_json::json!({ "root": { "id": "n-0", "children": 42 } });
        assert!(matches!(
            parse_scene_tree_result(&value),
            Err(SceneError::InvalidField(f)) if f == "root.children"
        ));
    }

    #[test]
    fn parse_scene_tree_result_non_object_result_errors() {
        let value = serde_json::json!([]);
        assert!(matches!(
            parse_scene_tree_result(&value),
            Err(SceneError::UnexpectedShape(_))
        ));
    }
}
