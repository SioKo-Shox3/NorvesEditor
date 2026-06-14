//! `log.message` event params domain types (sans-I/O).
//!
//! Extracts a [`LogMessage`] from the `params` value of a `log.message` event.
//! Wire shape follows `log.message.params.schema.json`: `level` and `message`
//! are required; `category` and `timestamp` are optional.

use norves_bridge_core::LogLevel;
use serde::Deserialize as _;
use serde_json::{Map, Value};

/// A single engine log line forwarded over the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct LogMessage {
    /// Severity of the log line.
    pub level: LogLevel,
    /// Log text.
    pub message: String,
    /// Optional log category/channel, e.g. `Engine`, `Render`, `Audio`.
    pub category: Option<String>,
    /// Optional engine-side timestamp (ISO 8601 string).
    pub timestamp: Option<String>,
}

/// Failure while extracting a [`LogMessage`] from an event params value.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    /// A required field was missing.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// A field was present but had the wrong type or an invalid enum value.
    #[error("invalid field: {0}")]
    InvalidField(String),

    /// The params payload did not have the expected JSON shape.
    #[error("unexpected payload shape: {0}")]
    UnexpectedShape(String),
}

/// Extracts a [`LogMessage`] from a `log.message` event `params` value.
///
/// Required: `level` (deserialized into [`LogLevel`]) and `message`. Optional
/// `category` / `timestamp` are kept when present.
pub fn parse_log_message(params: &Value) -> Result<LogMessage, LogError> {
    let obj = params
        .as_object()
        .ok_or_else(|| LogError::UnexpectedShape("log params is not an object".to_owned()))?;

    let level_value = obj
        .get("level")
        .ok_or_else(|| LogError::MissingField("level".to_owned()))?;
    let level = LogLevel::deserialize(level_value)
        .map_err(|_| LogError::InvalidField("level".to_owned()))?;

    let message = required_str(obj, "message")?.to_owned();
    let category = optional_str(obj, "category")?.map(str::to_owned);
    let timestamp = optional_str(obj, "timestamp")?.map(str::to_owned);

    Ok(LogMessage {
        level,
        message,
        category,
        timestamp,
    })
}

/// Returns the string at `key`, erroring if absent or not a string.
fn required_str<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a str, LogError> {
    match obj.get(key) {
        None => Err(LogError::MissingField(key.to_owned())),
        Some(value) => value
            .as_str()
            .ok_or_else(|| LogError::InvalidField(key.to_owned())),
    }
}

/// Returns the string at `key` if present; errors if present but not a string.
fn optional_str<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, LogError> {
    match obj.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| LogError::InvalidField(key.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_log_message_extracts_fields() {
        // Mirrors fixtures/events/log.message/positive/event-engine-valid.json params.
        let value = serde_json::json!({
            "level": "info",
            "category": "Engine",
            "message": "Game started",
            "timestamp": "2026-06-14T10:00:01Z"
        });
        let log = parse_log_message(&value).expect("parses");
        assert_eq!(log.level, LogLevel::Info);
        assert_eq!(log.message, "Game started");
        assert_eq!(log.category.as_deref(), Some("Engine"));
        assert_eq!(log.timestamp.as_deref(), Some("2026-06-14T10:00:01Z"));
    }

    #[test]
    fn parse_log_message_minimal_required_only() {
        let value = serde_json::json!({ "level": "error", "message": "boom" });
        let log = parse_log_message(&value).expect("parses");
        assert_eq!(log.level, LogLevel::Error);
        assert_eq!(log.message, "boom");
        assert_eq!(log.category, None);
        assert_eq!(log.timestamp, None);
    }

    #[test]
    fn parse_log_message_missing_level_errors() {
        let value = serde_json::json!({ "message": "boom" });
        assert!(matches!(
            parse_log_message(&value),
            Err(LogError::MissingField(f)) if f == "level"
        ));
    }

    #[test]
    fn parse_log_message_missing_message_errors() {
        let value = serde_json::json!({ "level": "info" });
        assert!(matches!(
            parse_log_message(&value),
            Err(LogError::MissingField(f)) if f == "message"
        ));
    }

    #[test]
    fn parse_log_message_invalid_level_errors() {
        let value = serde_json::json!({ "level": "fatal", "message": "boom" });
        assert!(matches!(
            parse_log_message(&value),
            Err(LogError::InvalidField(f)) if f == "level"
        ));
    }
}
