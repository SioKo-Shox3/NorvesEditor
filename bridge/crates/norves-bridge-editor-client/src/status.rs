//! `engine.getStatus` result domain types (sans-I/O).
//!
//! Extracts a [`StatusSnapshot`] from the `result` value of an
//! `engine.getStatus` response. Wire shape follows
//! `engine.getStatus.result.schema.json`: `engineState` and `runtimeState` are
//! required; `engineName`, `engineVersion`, `title` are optional.

use norves_bridge_core::{EngineState, RuntimeState};
use serde_json::{Map, Value};

/// Engine-served snapshot of engine and runtime status.
///
/// The editor backend separately owns process state; that is not part of this
/// engine-served snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSnapshot {
    /// Engine lifecycle state from the engine's own perspective.
    pub engine_state: EngineState,
    /// Runtime play/pause/stop state reported by the engine.
    pub runtime_state: RuntimeState,
    /// Optional engine integration name.
    pub engine_name: Option<String>,
    /// Optional engine version.
    pub engine_version: Option<String>,
    /// Optional current engine/game window title.
    pub title: Option<String>,
}

/// Failure while extracting a [`StatusSnapshot`] from a result value.
#[derive(Debug, thiserror::Error)]
pub enum StatusError {
    /// A required field was missing.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// A field was present but had the wrong type or an invalid enum value.
    #[error("invalid field: {0}")]
    InvalidField(String),

    /// The result payload did not have the expected JSON shape.
    #[error("unexpected payload shape: {0}")]
    UnexpectedShape(String),
}

/// Extracts a [`StatusSnapshot`] from an `engine.getStatus` `result` value.
///
/// Required: `engineState`, `runtimeState`, each deserialized into the core
/// enum. Optional string fields are kept when present.
pub fn parse_status_result(result: &Value) -> Result<StatusSnapshot, StatusError> {
    let obj = result
        .as_object()
        .ok_or_else(|| StatusError::UnexpectedShape("status result is not an object".to_owned()))?;

    let engine_state = required_enum::<EngineState>(obj, "engineState")?;
    let runtime_state = required_enum::<RuntimeState>(obj, "runtimeState")?;
    let engine_name = optional_str(obj, "engineName")?.map(str::to_owned);
    let engine_version = optional_str(obj, "engineVersion")?.map(str::to_owned);
    let title = optional_str(obj, "title")?.map(str::to_owned);

    Ok(StatusSnapshot {
        engine_state,
        runtime_state,
        engine_name,
        engine_version,
        title,
    })
}

/// Deserializes the value at `key` into `T`, erroring if absent or invalid.
fn required_enum<T: serde::de::DeserializeOwned>(
    obj: &Map<String, Value>,
    key: &str,
) -> Result<T, StatusError> {
    let value = obj
        .get(key)
        .ok_or_else(|| StatusError::MissingField(key.to_owned()))?;
    T::deserialize(value).map_err(|_| StatusError::InvalidField(key.to_owned()))
}

/// Returns the string at `key` if present; errors if present but not a string.
fn optional_str<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, StatusError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| StatusError::InvalidField(key.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_result_extracts_fields() {
        // Mirrors fixtures/methods/engine.getStatus/positive/response-valid.json result.
        let value = serde_json::json!({
            "engineState": "ready",
            "runtimeState": "edit",
            "engineName": "MockEngine",
            "engineVersion": "0.1.0",
            "title": "Mock Game"
        });
        let snapshot = parse_status_result(&value).expect("parses");
        assert_eq!(snapshot.engine_state, EngineState::Ready);
        assert_eq!(snapshot.runtime_state, RuntimeState::Edit);
        assert_eq!(snapshot.engine_name.as_deref(), Some("MockEngine"));
        assert_eq!(snapshot.engine_version.as_deref(), Some("0.1.0"));
        assert_eq!(snapshot.title.as_deref(), Some("Mock Game"));
    }

    #[test]
    fn parse_status_result_minimal_required_only() {
        let value = serde_json::json!({ "engineState": "running", "runtimeState": "playing" });
        let snapshot = parse_status_result(&value).expect("parses");
        assert_eq!(snapshot.engine_state, EngineState::Running);
        assert_eq!(snapshot.runtime_state, RuntimeState::Playing);
        assert_eq!(snapshot.engine_name, None);
        assert_eq!(snapshot.engine_version, None);
        assert_eq!(snapshot.title, None);
    }

    #[test]
    fn parse_status_result_missing_engine_state_errors() {
        let value = serde_json::json!({ "runtimeState": "edit" });
        assert!(matches!(
            parse_status_result(&value),
            Err(StatusError::MissingField(f)) if f == "engineState"
        ));
    }

    #[test]
    fn parse_status_result_missing_runtime_state_errors() {
        let value = serde_json::json!({ "engineState": "ready" });
        assert!(matches!(
            parse_status_result(&value),
            Err(StatusError::MissingField(f)) if f == "runtimeState"
        ));
    }

    #[test]
    fn parse_status_result_invalid_enum_errors() {
        let value = serde_json::json!({ "engineState": "bogus", "runtimeState": "edit" });
        assert!(matches!(
            parse_status_result(&value),
            Err(StatusError::InvalidField(f)) if f == "engineState"
        ));
    }
}
