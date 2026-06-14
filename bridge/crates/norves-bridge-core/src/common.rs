//! Shared enums and value newtypes reused across Bridge method/event payloads.
//!
//! These mirror the `$defs` of `common.schema.json`. Enum wire strings and
//! newtype patterns are kept in lock-step with that schema; changing either is
//! a protocol change requiring review.

use serde::{Deserialize, Serialize};

use crate::error::CodecError;

/// Severity of a log line. Schema: `common.schema.json#/$defs/logLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// `trace`
    Trace,
    /// `debug`
    Debug,
    /// `info`
    Info,
    /// `warn`
    Warn,
    /// `error`
    Error,
}

/// Engine lifecycle state from the engine's own perspective.
/// Schema: `common.schema.json#/$defs/engineState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineState {
    /// `initializing`
    Initializing,
    /// `ready`
    Ready,
    /// `running`
    Running,
    /// `error`
    Error,
}

/// Runtime play/pause/stop state reported by the engine.
/// Schema: `common.schema.json#/$defs/runtimeState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeState {
    /// `edit`
    Edit,
    /// `playing`
    Playing,
    /// `paused`
    Paused,
    /// `stopped`
    Stopped,
    /// `unknown`
    Unknown,
}

/// State of the engine's external native viewport window.
/// Schema: `common.schema.json#/$defs/viewportState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewportState {
    /// `focused`
    Focused,
    /// `visible`
    Visible,
    /// `hidden`
    Hidden,
    /// `minimized`
    Minimized,
    /// `unknown`
    Unknown,
}

/// Which side produced an event or error.
/// Schema: `common.schema.json#/$defs/origin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    /// `engine`
    #[serde(rename = "engine")]
    Engine,
    /// `editor-backend`
    #[serde(rename = "editor-backend")]
    EditorBackend,
}

/// Returns true if `value` matches `^[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9]+$`
/// (namespaced lowerCamel.member token shared by methods, events, and
/// capability tokens).
pub(crate) fn is_namespaced_token(value: &str) -> bool {
    let Some((head, tail)) = value.split_once('.') else {
        return false;
    };
    if tail.contains('.') {
        return false;
    }
    let mut head_chars = head.chars();
    match head_chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    if !head_chars.all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    !tail.is_empty() && tail.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Returns true if `value` matches `^[0-9]+\.[0-9]+$`.
pub(crate) fn is_version_string(value: &str) -> bool {
    let Some((major, minor)) = value.split_once('.') else {
        return false;
    };
    !major.is_empty()
        && !minor.is_empty()
        && major.chars().all(|c| c.is_ascii_digit())
        && minor.chars().all(|c| c.is_ascii_digit())
}

/// Opaque identifier of a scene object/node. Schema: non-empty string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ObjectId(String);

impl ObjectId {
    /// Returns the underlying identifier string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ObjectId {
    type Error = CodecError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Err(CodecError::InvalidField {
                field: "objectId",
                value,
            })
        } else {
            Ok(ObjectId(value))
        }
    }
}

impl From<ObjectId> for String {
    fn from(value: ObjectId) -> Self {
        value.0
    }
}

/// Namespaced capability token, e.g. `runtime.control`.
/// Schema pattern: `^[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9]+$`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CapabilityToken(String);

impl CapabilityToken {
    /// Returns the underlying token string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CapabilityToken {
    type Error = CodecError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_namespaced_token(&value) {
            Ok(CapabilityToken(value))
        } else {
            Err(CodecError::InvalidField {
                field: "capabilityToken",
                value,
            })
        }
    }
}

impl From<CapabilityToken> for String {
    fn from(value: CapabilityToken) -> Self {
        value.0
    }
}

/// Protocol or product version string in `MAJOR.MINOR` form.
/// Schema pattern: `^[0-9]+\.[0-9]+$`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct VersionString(String);

impl VersionString {
    /// Returns the underlying version string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for VersionString {
    type Error = CodecError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_version_string(&value) {
            Ok(VersionString(value))
        } else {
            Err(CodecError::InvalidVersion(value))
        }
    }
}

impl From<VersionString> for String {
    fn from(value: VersionString) -> Self {
        value.0
    }
}

/// Authoritative description of one capability an engine endpoint advertises.
/// Schema: `common.schema.json#/$defs/capabilityDescriptor`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDescriptor {
    /// Capability token.
    pub name: CapabilityToken,
    /// Optional capability version, `MAJOR.MINOR`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionString>,
    /// Optional human-readable explanation of the capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_round_trips_lowercase() {
        for (variant, wire) in [
            (LogLevel::Trace, "\"trace\""),
            (LogLevel::Debug, "\"debug\""),
            (LogLevel::Info, "\"info\""),
            (LogLevel::Warn, "\"warn\""),
            (LogLevel::Error, "\"error\""),
        ] {
            assert_eq!(serde_json::to_string(&variant).unwrap(), wire);
            let back: LogLevel = serde_json::from_str(wire).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn runtime_and_engine_and_viewport_states_parse() {
        assert_eq!(
            serde_json::from_str::<RuntimeState>("\"playing\"").unwrap(),
            RuntimeState::Playing
        );
        assert_eq!(
            serde_json::from_str::<EngineState>("\"initializing\"").unwrap(),
            EngineState::Initializing
        );
        assert_eq!(
            serde_json::from_str::<ViewportState>("\"minimized\"").unwrap(),
            ViewportState::Minimized
        );
    }

    #[test]
    fn origin_uses_kebab_wire_string() {
        assert_eq!(
            serde_json::to_string(&Origin::EditorBackend).unwrap(),
            "\"editor-backend\""
        );
        assert_eq!(
            serde_json::from_str::<Origin>("\"editor-backend\"").unwrap(),
            Origin::EditorBackend
        );
        assert_eq!(
            serde_json::from_str::<Origin>("\"engine\"").unwrap(),
            Origin::Engine
        );
    }

    #[test]
    fn unknown_enum_member_fails() {
        assert!(serde_json::from_str::<LogLevel>("\"fatal\"").is_err());
        assert!(serde_json::from_str::<Origin>("\"frontend\"").is_err());
    }

    #[test]
    fn object_id_rejects_empty() {
        assert!(serde_json::from_str::<ObjectId>("\"\"").is_err());
        assert_eq!(
            serde_json::from_str::<ObjectId>("\"node-1\"")
                .unwrap()
                .as_str(),
            "node-1"
        );
    }

    #[test]
    fn capability_token_pattern_enforced() {
        assert!(serde_json::from_str::<CapabilityToken>("\"runtime.control\"").is_ok());
        assert!(serde_json::from_str::<CapabilityToken>("\"Runtime.control\"").is_err());
        assert!(serde_json::from_str::<CapabilityToken>("\"runtime\"").is_err());
        assert!(serde_json::from_str::<CapabilityToken>("\"a.b.c\"").is_err());
        assert!(serde_json::from_str::<CapabilityToken>("\"runtime.\"").is_err());
    }

    #[test]
    fn version_string_pattern_enforced() {
        assert!(serde_json::from_str::<VersionString>("\"0.1\"").is_ok());
        assert!(serde_json::from_str::<VersionString>("\"10.23\"").is_ok());
        assert!(serde_json::from_str::<VersionString>("\"0.1.0\"").is_err());
        assert!(serde_json::from_str::<VersionString>("\"v0.1\"").is_err());
        assert!(serde_json::from_str::<VersionString>("\".1\"").is_err());
    }

    #[test]
    fn capability_descriptor_rejects_unknown_field() {
        let json = r#"{"name":"runtime.control","extra":true}"#;
        assert!(serde_json::from_str::<CapabilityDescriptor>(json).is_err());
    }

    #[test]
    fn capability_descriptor_optional_fields_round_trip() {
        let json = r#"{"name":"log.stream","version":"1.0","description":"streams logs"}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let desc: CapabilityDescriptor = serde_json::from_str(json).unwrap();
        let reserialized = serde_json::to_value(&desc).unwrap();
        assert_eq!(reserialized, value);
    }
}
