//! Strict `bridge.getCapabilities` result extraction.

use norves_bridge_core::CapabilityDescriptor;
use serde::Deserialize;
use serde_json::Value;

/// Authoritative capabilities advertised by one live engine session.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesResult {
    pub capabilities: Vec<CapabilityDescriptor>,
}

/// Failure while extracting a `bridge.getCapabilities` result.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("invalid bridge.getCapabilities result: {0}")]
    InvalidResult(String),
}

/// Strictly extracts a [`CapabilitiesResult`] from a
/// `bridge.getCapabilities` `result` value.
pub fn parse_capabilities_result(result: &Value) -> Result<CapabilitiesResult, CapabilityError> {
    serde_json::from_value(result.clone())
        .map_err(|err| CapabilityError::InvalidResult(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_accept_strict_descriptor_array() {
        let value = serde_json::json!({
            "capabilities": [{
                "name": "asset.reload",
                "version": "0.2",
                "description": "Reload the asset manifest."
            }]
        });

        let result = parse_capabilities_result(&value).expect("parses");
        assert_eq!(result.capabilities.len(), 1);
        assert_eq!(result.capabilities[0].name.as_str(), "asset.reload");
        assert_eq!(
            result.capabilities[0]
                .version
                .as_ref()
                .map(norves_bridge_core::VersionString::as_str),
            Some("0.2")
        );
    }

    #[test]
    fn capabilities_reject_missing_non_array_and_result_extra_fields() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({ "capabilities": {} }),
            serde_json::json!({ "capabilities": [], "extra": true }),
        ] {
            assert!(
                parse_capabilities_result(&value).is_err(),
                "unexpectedly accepted {value}"
            );
        }
    }

    #[test]
    fn capabilities_reject_invalid_descriptor_fields() {
        for descriptor in [
            serde_json::json!({ "name": "Asset.reload" }),
            serde_json::json!({ "name": "asset.reload", "version": "0.2.0" }),
            serde_json::json!({ "name": "asset.reload", "extra": true }),
        ] {
            let value = serde_json::json!({ "capabilities": [descriptor] });
            assert!(
                parse_capabilities_result(&value).is_err(),
                "unexpectedly accepted {value}"
            );
        }
    }
}
