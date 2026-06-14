//! Generic serialized property / scene-tree DTO types.
//!
//! These mirror the property and scene `$defs` of `common.schema.json`. Every
//! value here is a *snapshot copy* — never a reference into engine live memory
//! (see `docs/memory-buffer-policy.md`). No engine-specific (NorvesLib) type
//! names appear; values are plain JSON.

use serde::{Deserialize, Serialize};

/// Generic serialized property value: an arbitrary plain JSON value.
///
/// A snapshot copy, never a reference into engine live memory. Schema:
/// `common.schema.json#/$defs/propertyValue`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PropertyValue(pub serde_json::Value);

/// One generic name->value entry in an object property snapshot.
/// Schema: `common.schema.json#/$defs/propertyEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyEntry {
    /// Generic property name.
    pub name: String,
    /// Snapshot copy of the property value.
    pub value: PropertyValue,
    /// Optional free-form type label.
    #[serde(rename = "valueType", default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<String>,
}

/// Ordered collection of generic property entries; may be empty.
/// Schema: `common.schema.json#/$defs/propertyBag`.
pub type PropertyBag = Vec<PropertyEntry>;

/// Generic definition of one property exposed by a type.
/// Schema: `common.schema.json#/$defs/propertyDefinition`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyDefinition {
    /// Generic property name.
    pub name: String,
    /// Free-form generic type label for this property's value.
    #[serde(rename = "valueType")]
    pub value_type: String,
}

/// Generic description of one object/component type the engine exposes.
/// Schema: `common.schema.json#/$defs/typeDescriptor`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeDescriptor {
    /// Free-form generic name of the type.
    #[serde(rename = "typeName")]
    pub type_name: String,
    /// Optional generic classification, e.g. `object` or `component`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Optional property definitions for this type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<PropertyDefinition>>,
}

/// Recursive generic node in a scene-tree snapshot.
///
/// A serialized DTO copy, not a live engine pointer. Schema:
/// `common.schema.json#/$defs/sceneNode`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneNode {
    /// Opaque identifier of this node.
    pub id: crate::common::ObjectId,
    /// Optional human-readable node name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional generic node classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Optional child nodes; recursion via self-reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<SceneNode>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(json: &str) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let original: serde_json::Value = serde_json::from_str(json).unwrap();
        let typed: T = serde_json::from_str(json).unwrap();
        let reserialized = serde_json::to_value(&typed).unwrap();
        assert_eq!(reserialized, original);
        typed
    }

    #[test]
    fn property_value_accepts_null_number_and_nested() {
        let json = r#"{
            "name": "transform",
            "value": {
                "scalar": 3.5,
                "flag": null,
                "nested": { "x": 1, "y": [true, "s"] }
            },
            "valueType": "object"
        }"#;
        let entry: PropertyEntry = round_trip(json);
        assert_eq!(entry.name, "transform");
        assert_eq!(entry.value_type.as_deref(), Some("object"));
    }

    #[test]
    fn property_entry_minimal_omits_value_type() {
        let json = r#"{"name":"hp","value":100}"#;
        let entry: PropertyEntry = round_trip(json);
        assert!(entry.value_type.is_none());
        assert_eq!(entry.value, PropertyValue(serde_json::json!(100)));
    }

    #[test]
    fn property_entry_rejects_unknown_field() {
        let json = r#"{"name":"hp","value":1,"unexpected":true}"#;
        assert!(serde_json::from_str::<PropertyEntry>(json).is_err());
    }

    #[test]
    fn property_entry_requires_value() {
        let json = r#"{"name":"hp"}"#;
        assert!(serde_json::from_str::<PropertyEntry>(json).is_err());
    }

    #[test]
    fn property_definition_round_trips() {
        let json = r#"{"name":"hp","valueType":"int"}"#;
        let def: PropertyDefinition = round_trip(json);
        assert_eq!(def.value_type, "int");
    }

    #[test]
    fn type_descriptor_round_trips_with_properties() {
        let json = r#"{
            "typeName": "Enemy",
            "kind": "object",
            "properties": [
                {"name":"hp","valueType":"int"},
                {"name":"name","valueType":"string"}
            ]
        }"#;
        let desc: TypeDescriptor = round_trip(json);
        assert_eq!(desc.type_name, "Enemy");
        assert_eq!(desc.properties.as_ref().map(Vec::len), Some(2));
    }

    #[test]
    fn scene_node_recurses() {
        let json = r#"{
            "id": "root",
            "name": "Root",
            "children": [
                {"id":"child-a"},
                {"id":"child-b","children":[{"id":"grandchild"}]}
            ]
        }"#;
        let node: SceneNode = round_trip(json);
        assert_eq!(node.id.as_str(), "root");
        assert_eq!(node.children.as_ref().map(Vec::len), Some(2));
    }

    #[test]
    fn scene_node_requires_id() {
        assert!(serde_json::from_str::<SceneNode>(r#"{"name":"x"}"#).is_err());
        assert!(serde_json::from_str::<SceneNode>(r#"{"id":""}"#).is_err());
    }
}
