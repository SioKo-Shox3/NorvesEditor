//! `object.getSnapshot` / `schema.getSnapshot` result domain types (sans-I/O).
//!
//! Extracts an [`ObjectSnapshot`] from the `result` value of an
//! `object.getSnapshot` response and a [`SchemaSnapshot`] from a
//! `schema.getSnapshot` response. Wire shapes follow
//! `object.getSnapshot.result.schema.json` (`{ objectId, name?, kind?,
//! properties }`, `properties` a `propertyBag`) and
//! `schema.getSnapshot.result.schema.json` (`{ types }`, each a
//! `typeDescriptor`), whose `$defs` (`propertyBag` / `propertyEntry` /
//! `typeDescriptor` / `propertyDefinition`) live in `common.schema.json`.
//!
//! These are the drift-guards used by the `object_get_snapshot` /
//! `schema_get_snapshot` Tauri commands: they validate the wire shape so a
//! malformed engine result surfaces as a clean backend error rather than being
//! forwarded blindly, while the command still returns the ORIGINAL wire `Value`
//! (no re-modeling round-trip). The types here are generic — `objectId` / `name`
//! / `kind` / `typeName` / `valueType` carry no engine-specific semantics, and
//! property values are kept as raw [`Value`] (snapshot copies, never live engine
//! pointers).

use serde_json::{Map, Value};

/// A serialized object snapshot extracted from an `object.getSnapshot` result.
///
/// A DTO copy, never a live engine pointer. `properties` is the ordered
/// `propertyBag` and may be empty.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectSnapshot {
    /// Object this snapshot describes (non-empty on the wire).
    pub object_id: String,
    /// Optional human-readable object name.
    pub name: Option<String>,
    /// Optional generic object classification (free-form).
    pub kind: Option<String>,
    /// Serialized property entries for this object; empty when there are none.
    pub properties: Vec<PropertyEntry>,
}

/// One generic `name -> value` entry in an object property snapshot.
///
/// `value` is a raw snapshot copy of the wire value (string/number/boolean/null/
/// array/object). `value_type` is an optional free-form label.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyEntry {
    /// Generic property name (non-empty on the wire).
    pub name: String,
    /// Snapshot copy of the property value, kept as a raw JSON value.
    pub value: Value,
    /// Optional free-form type label.
    pub value_type: Option<String>,
}

/// A serialized type-schema snapshot extracted from a `schema.getSnapshot`
/// result.
///
/// A DTO copy, never a live engine pointer. `types` may be empty.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaSnapshot {
    /// Generic type descriptors the engine exposes.
    pub types: Vec<TypeDescriptor>,
}

/// One generic type descriptor: a `typeName`, an optional `kind`, and optional
/// property definitions.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDescriptor {
    /// Free-form generic name of the type (non-empty on the wire).
    pub type_name: String,
    /// Optional generic classification (free-form).
    pub kind: Option<String>,
    /// Optional property definitions for this type.
    pub properties: Vec<PropertyDefinition>,
}

/// One generic property definition: a `name` and a `valueType`. Describes a
/// type's property shape, not a concrete value.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDefinition {
    /// Generic property name (non-empty on the wire).
    pub name: String,
    /// Free-form generic type label for this property's value.
    pub value_type: String,
}

/// Failure while extracting an [`ObjectSnapshot`] or [`SchemaSnapshot`] from a
/// result value.
#[derive(Debug, thiserror::Error)]
pub enum ObjectError {
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

/// Extracts an [`ObjectSnapshot`] from an `object.getSnapshot` `result` value.
///
/// Required: `objectId`, `properties` (a `propertyBag`). Validates each property
/// entry so a malformed entry is rejected at the boundary.
pub fn parse_object_snapshot_result(result: &Value) -> Result<ObjectSnapshot, ObjectError> {
    let obj = result
        .as_object()
        .ok_or_else(|| ObjectError::UnexpectedShape("object result is not an object".to_owned()))?;

    let object_id = required_str(obj, "objectId", "result")?.to_owned();
    let name = optional_str(obj, "name", "result")?.map(str::to_owned);
    let kind = optional_str(obj, "kind", "result")?.map(str::to_owned);
    let properties = parse_property_bag(obj)?;

    Ok(ObjectSnapshot {
        object_id,
        name,
        kind,
        properties,
    })
}

/// Parses the required `properties` array (a `propertyBag`), validating each
/// entry.
fn parse_property_bag(obj: &Map<String, Value>) -> Result<Vec<PropertyEntry>, ObjectError> {
    let value = obj
        .get("properties")
        .ok_or_else(|| ObjectError::MissingField("result.properties".to_owned()))?;
    let items = value
        .as_array()
        .ok_or_else(|| ObjectError::InvalidField("result.properties".to_owned()))?;

    let mut properties = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        properties.push(parse_property_entry(
            item,
            &format!("result.properties[{index}]"),
        )?);
    }
    Ok(properties)
}

/// Parses one `propertyEntry`. `path` names the field for error messages.
fn parse_property_entry(value: &Value, path: &str) -> Result<PropertyEntry, ObjectError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ObjectError::InvalidField(format!("{path} is not an object")))?;

    let name = required_str(obj, "name", path)?.to_owned();
    // `value` is required but may be ANY JSON type (including null). It is kept
    // as a raw snapshot copy; absence is the only error.
    let property_value = obj
        .get("value")
        .ok_or_else(|| ObjectError::MissingField(format!("{path}.value")))?
        .clone();
    let value_type = optional_str(obj, "valueType", path)?.map(str::to_owned);

    Ok(PropertyEntry {
        name,
        value: property_value,
        value_type,
    })
}

/// Extracts a [`SchemaSnapshot`] from a `schema.getSnapshot` `result` value.
///
/// Required: `types` (an array of `typeDescriptor`). Validates each descriptor
/// so a malformed entry is rejected at the boundary.
pub fn parse_schema_snapshot_result(result: &Value) -> Result<SchemaSnapshot, ObjectError> {
    let obj = result
        .as_object()
        .ok_or_else(|| ObjectError::UnexpectedShape("schema result is not an object".to_owned()))?;

    let value = obj
        .get("types")
        .ok_or_else(|| ObjectError::MissingField("result.types".to_owned()))?;
    let items = value
        .as_array()
        .ok_or_else(|| ObjectError::InvalidField("result.types".to_owned()))?;

    let mut types = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        types.push(parse_type_descriptor(
            item,
            &format!("result.types[{index}]"),
        )?);
    }
    Ok(SchemaSnapshot { types })
}

/// Parses one `typeDescriptor`. `path` names the field for error messages.
fn parse_type_descriptor(value: &Value, path: &str) -> Result<TypeDescriptor, ObjectError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ObjectError::InvalidField(format!("{path} is not an object")))?;

    let type_name = required_str(obj, "typeName", path)?.to_owned();
    let kind = optional_str(obj, "kind", path)?.map(str::to_owned);
    let properties = parse_property_definitions(obj, path)?;

    Ok(TypeDescriptor {
        type_name,
        kind,
        properties,
    })
}

/// Parses the optional `properties` array of `propertyDefinition`s.
fn parse_property_definitions(
    obj: &Map<String, Value>,
    path: &str,
) -> Result<Vec<PropertyDefinition>, ObjectError> {
    match obj.get("properties") {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut defs = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                defs.push(parse_property_definition(
                    item,
                    &format!("{path}.properties[{index}]"),
                )?);
            }
            Ok(defs)
        }
        Some(_) => Err(ObjectError::InvalidField(format!("{path}.properties"))),
    }
}

/// Parses one `propertyDefinition`. `path` names the field for error messages.
fn parse_property_definition(value: &Value, path: &str) -> Result<PropertyDefinition, ObjectError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ObjectError::InvalidField(format!("{path} is not an object")))?;

    let name = required_str(obj, "name", path)?.to_owned();
    let value_type = required_str(obj, "valueType", path)?.to_owned();

    Ok(PropertyDefinition { name, value_type })
}

/// Returns the required string at `key`, erroring if absent or not a string.
fn required_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a str, ObjectError> {
    let value = obj
        .get(key)
        .ok_or_else(|| ObjectError::MissingField(format!("{path}.{key}")))?;
    value
        .as_str()
        .ok_or_else(|| ObjectError::InvalidField(format!("{path}.{key}")))
}

/// Returns the string at `key` if present; errors if present but not a string.
fn optional_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<&'a str>, ObjectError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| ObjectError::InvalidField(format!("{path}.{key}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_object_snapshot_result_extracts_all_value_types() {
        // Mirrors fixtures/methods/object.getSnapshot/positive/response-valid.json result.
        let value = serde_json::json!({
            "objectId": "n-1",
            "name": "NodeA",
            "kind": "object",
            "properties": [
                { "name": "label", "value": "Example Name", "valueType": "string" },
                { "name": "fieldOfView", "value": 60, "valueType": "number" },
                { "name": "enabled", "value": true, "valueType": "boolean" },
                { "name": "parent", "value": null },
                { "name": "position", "value": [0, 1.5, -10], "valueType": "vector3" },
                { "name": "metadata", "value": { "locked": false, "tag": "primary" } }
            ]
        });
        let snapshot = parse_object_snapshot_result(&value).expect("parses");
        assert_eq!(snapshot.object_id, "n-1");
        assert_eq!(snapshot.name.as_deref(), Some("NodeA"));
        assert_eq!(snapshot.kind.as_deref(), Some("object"));
        assert_eq!(snapshot.properties.len(), 6);

        // Scalar string.
        assert_eq!(snapshot.properties[0].name, "label");
        assert_eq!(snapshot.properties[0].value, Value::from("Example Name"));
        assert_eq!(snapshot.properties[0].value_type.as_deref(), Some("string"));

        // Number.
        assert_eq!(snapshot.properties[1].value, Value::from(60));

        // Boolean.
        assert_eq!(snapshot.properties[2].value, Value::from(true));

        // Null with no valueType.
        assert_eq!(snapshot.properties[3].name, "parent");
        assert_eq!(snapshot.properties[3].value, Value::Null);
        assert_eq!(snapshot.properties[3].value_type, None);

        // Array.
        assert!(snapshot.properties[4].value.is_array());

        // Object with no valueType.
        assert!(snapshot.properties[5].value.is_object());
        assert_eq!(snapshot.properties[5].value_type, None);
    }

    #[test]
    fn parse_object_snapshot_result_empty_properties_ok() {
        let value = serde_json::json!({ "objectId": "n-9", "properties": [] });
        let snapshot = parse_object_snapshot_result(&value).expect("parses");
        assert_eq!(snapshot.object_id, "n-9");
        assert_eq!(snapshot.name, None);
        assert_eq!(snapshot.kind, None);
        assert!(snapshot.properties.is_empty());
    }

    #[test]
    fn parse_object_snapshot_result_missing_object_id_errors() {
        let value = serde_json::json!({ "properties": [] });
        assert!(matches!(
            parse_object_snapshot_result(&value),
            Err(ObjectError::MissingField(f)) if f == "result.objectId"
        ));
    }

    #[test]
    fn parse_object_snapshot_result_missing_properties_errors() {
        let value = serde_json::json!({ "objectId": "n-1" });
        assert!(matches!(
            parse_object_snapshot_result(&value),
            Err(ObjectError::MissingField(f)) if f == "result.properties"
        ));
    }

    #[test]
    fn parse_object_snapshot_result_entry_missing_value_errors() {
        let value = serde_json::json!({
            "objectId": "n-1",
            "properties": [ { "name": "x" } ]
        });
        assert!(matches!(
            parse_object_snapshot_result(&value),
            Err(ObjectError::MissingField(f)) if f == "result.properties[0].value"
        ));
    }

    #[test]
    fn parse_object_snapshot_result_entry_missing_name_errors() {
        let value = serde_json::json!({
            "objectId": "n-1",
            "properties": [ { "value": 1 } ]
        });
        assert!(matches!(
            parse_object_snapshot_result(&value),
            Err(ObjectError::MissingField(f)) if f == "result.properties[0].name"
        ));
    }

    #[test]
    fn parse_object_snapshot_result_non_array_properties_errors() {
        let value = serde_json::json!({ "objectId": "n-1", "properties": 42 });
        assert!(matches!(
            parse_object_snapshot_result(&value),
            Err(ObjectError::InvalidField(f)) if f == "result.properties"
        ));
    }

    #[test]
    fn parse_object_snapshot_result_non_object_result_errors() {
        let value = serde_json::json!([]);
        assert!(matches!(
            parse_object_snapshot_result(&value),
            Err(ObjectError::UnexpectedShape(_))
        ));
    }

    #[test]
    fn parse_schema_snapshot_result_extracts_types() {
        // Mirrors fixtures/methods/schema.getSnapshot/positive/response-valid.json result.
        let value = serde_json::json!({
            "types": [
                {
                    "typeName": "TypeA",
                    "kind": "object",
                    "properties": [
                        { "name": "fieldOfView", "valueType": "number" },
                        { "name": "enabled", "valueType": "boolean" }
                    ]
                },
                { "typeName": "TypeB", "kind": "component" }
            ]
        });
        let snapshot = parse_schema_snapshot_result(&value).expect("parses");
        assert_eq!(snapshot.types.len(), 2);

        let type_a = &snapshot.types[0];
        assert_eq!(type_a.type_name, "TypeA");
        assert_eq!(type_a.kind.as_deref(), Some("object"));
        assert_eq!(type_a.properties.len(), 2);
        assert_eq!(type_a.properties[0].name, "fieldOfView");
        assert_eq!(type_a.properties[0].value_type, "number");

        let type_b = &snapshot.types[1];
        assert_eq!(type_b.type_name, "TypeB");
        assert_eq!(type_b.kind.as_deref(), Some("component"));
        assert!(type_b.properties.is_empty());
    }

    #[test]
    fn parse_schema_snapshot_result_empty_types_ok() {
        let value = serde_json::json!({ "types": [] });
        let snapshot = parse_schema_snapshot_result(&value).expect("parses");
        assert!(snapshot.types.is_empty());
    }

    #[test]
    fn parse_schema_snapshot_result_missing_types_errors() {
        let value = serde_json::json!({});
        assert!(matches!(
            parse_schema_snapshot_result(&value),
            Err(ObjectError::MissingField(f)) if f == "result.types"
        ));
    }

    #[test]
    fn parse_schema_snapshot_result_type_missing_type_name_errors() {
        let value = serde_json::json!({ "types": [ { "kind": "object" } ] });
        assert!(matches!(
            parse_schema_snapshot_result(&value),
            Err(ObjectError::MissingField(f)) if f == "result.types[0].typeName"
        ));
    }

    #[test]
    fn parse_schema_snapshot_result_property_def_missing_value_type_errors() {
        let value = serde_json::json!({
            "types": [ { "typeName": "T", "properties": [ { "name": "x" } ] } ]
        });
        assert!(matches!(
            parse_schema_snapshot_result(&value),
            Err(ObjectError::MissingField(f)) if f == "result.types[0].properties[0].valueType"
        ));
    }

    #[test]
    fn parse_schema_snapshot_result_non_array_types_errors() {
        let value = serde_json::json!({ "types": 7 });
        assert!(matches!(
            parse_schema_snapshot_result(&value),
            Err(ObjectError::InvalidField(f)) if f == "result.types"
        ));
    }

    #[test]
    fn parse_schema_snapshot_result_non_object_result_errors() {
        let value = serde_json::json!("nope");
        assert!(matches!(
            parse_schema_snapshot_result(&value),
            Err(ObjectError::UnexpectedShape(_))
        ));
    }
}
