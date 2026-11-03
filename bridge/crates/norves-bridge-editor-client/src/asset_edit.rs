//! 資産の編集結果を厳格に解析し、能力を確認して型付きの要求を送る。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use norves_bridge_core::{
    BridgeError, CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope, VersionString,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{CapabilitiesResult, DispatchHandle, RequestError};

/// 資産の編集に必要な能力名。
pub const ASSET_EDIT_CAPABILITY: &str = "asset.edit";
/// エンジンが資産の編集を拒否したときのコード。
pub const ASSET_EDIT_REJECTED_CODE: &str = "ENGINE_ASSET_EDIT_REJECTED";

// プロセス内の全クライアントで要求 ID を一意にする。
static NEXT_ASSET_EDIT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// 資産のプロパティを複製したスナップショット。
#[derive(Debug, Clone, PartialEq)]
pub struct AssetProperties {
    pub logical_path: String,
    pub properties: Vec<AssetProperty>,
}

/// プロパティの値と、その編集に必要なメタデータ。
#[derive(Debug, Clone, PartialEq)]
pub struct AssetProperty {
    /// エンジンが定める不透明な位置。解釈せず、そのまま編集要求へ渡す。
    pub path: String,
    pub value_type: String,
    pub value: Value,
    pub unit: Option<String>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub description: Option<String>,
}

/// プロパティの変更前と、適用後の値のスナップショット。
#[derive(Debug, Clone, PartialEq)]
pub struct AssetSetPropertyResult {
    pub previous_value: Value,
    /// 欄の省略だけが `None`。明示された `null` は `Some(Value::Null)`。
    pub applied_value: Option<Value>,
}

/// 同期保存が完了したことを示す応答。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSaveResult {
    pub saved: bool,
}

/// エンジンが返した編集拒否の理由。未知のコードも保持する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetEditRejectReason {
    TypeMismatch,
    OutOfRange,
    UnknownProperty,
    UnknownAsset,
    SaveFailed,
    Other(String),
}

/// エンジンによる拒否の詳細。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEditRejection {
    pub reason: AssetEditRejectReason,
    pub path: Option<String>,
    pub message: String,
}

/// 資産の編集で生じた能力不足、拒否、通信、解析の失敗。
#[derive(Debug, thiserror::Error)]
pub enum AssetEditError {
    #[error("資産を編集する能力が広告されていません")]
    CapabilityNotNegotiated,
    #[error("資産の編集が拒否されました: {}", .0.message)]
    Rejected(AssetEditRejection),
    #[error("エンジンのエラー ({code}): {message}")]
    Engine { code: String, message: String },
    #[error("資産の編集要求に失敗しました: {0}")]
    Request(#[from] RequestError),
    #[error("資産の編集結果が不正です: {0}")]
    InvalidResult(String),
}

/// `asset.getProperties` の結果をスキーマに従って解析する。
pub fn parse_asset_properties_result(result: &Value) -> Result<AssetProperties, AssetEditError> {
    let obj = strict_object(result, &["logicalPath", "properties"])?;
    let logical_path = required_string(obj, "logicalPath")?;
    let properties = required_value(obj, "properties")?
        .as_array()
        .ok_or_else(|| invalid("properties は配列である必要があります"))?
        .iter()
        .map(parse_asset_property)
        .collect::<Result<_, _>>()?;
    Ok(AssetProperties {
        logical_path,
        properties,
    })
}

fn parse_asset_property(value: &Value) -> Result<AssetProperty, AssetEditError> {
    let obj = strict_object(
        value,
        &[
            "path",
            "valueType",
            "value",
            "unit",
            "minimum",
            "maximum",
            "description",
        ],
    )?;
    Ok(AssetProperty {
        path: required_string(obj, "path")?,
        value_type: required_string(obj, "valueType")?,
        value: required_value(obj, "value")?.clone(),
        unit: optional_field(obj, "unit")?,
        minimum: optional_field(obj, "minimum")?,
        maximum: optional_field(obj, "maximum")?,
        description: optional_field(obj, "description")?,
    })
}

/// `asset.setProperty` の結果を解析する。変更前の値は必須。
pub fn parse_asset_set_property_result(
    result: &Value,
) -> Result<AssetSetPropertyResult, AssetEditError> {
    let obj = strict_object(result, &["previousValue", "appliedValue"])?;
    Ok(AssetSetPropertyResult {
        previous_value: required_value(obj, "previousValue")?.clone(),
        applied_value: obj.get("appliedValue").cloned(),
    })
}

/// `asset.save` の結果を解析し、保存完了を示さない応答を拒否する。
pub fn parse_asset_save_result(result: &Value) -> Result<AssetSaveResult, AssetEditError> {
    let obj = strict_object(result, &["saved"])?;
    if required_value(obj, "saved")?.as_bool() != Some(true) {
        return Err(invalid("saved は true である必要があります"));
    }
    Ok(AssetSaveResult { saved: true })
}

fn invalid(message: impl Into<String>) -> AssetEditError {
    AssetEditError::InvalidResult(message.into())
}

fn strict_object<'a>(
    value: &'a Value,
    fields: &[&str],
) -> Result<&'a Map<String, Value>, AssetEditError> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("オブジェクトである必要があります"))?;
    for key in obj.keys() {
        if !fields.contains(&key.as_str()) {
            return Err(invalid(format!("未知の欄です: {key}")));
        }
    }
    Ok(obj)
}

fn required_value<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a Value, AssetEditError> {
    obj.get(key)
        .ok_or_else(|| invalid(format!("必須の欄がありません: {key}")))
}

fn required_string(obj: &Map<String, Value>, key: &str) -> Result<String, AssetEditError> {
    required_value(obj, key)?
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("空でない文字列である必要があります: {key}")))
}

fn optional_field<T: DeserializeOwned>(
    obj: &Map<String, Value>,
    key: &str,
) -> Result<Option<T>, AssetEditError> {
    obj.get(key)
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|err| invalid(format!("{key}: {err}")))
        })
        .transpose()
}

fn engine_error(error: BridgeError) -> AssetEditError {
    if error.code.as_str() != ASSET_EDIT_REJECTED_CODE {
        return AssetEditError::Engine {
            code: error.code.as_str().to_owned(),
            message: error.message,
        };
    }
    let data = error.data.as_ref();
    // 理由が欠けた不完全な拒否応答も、拒否として保持する。
    let reason = data
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let reason = match reason {
        "type_mismatch" => AssetEditRejectReason::TypeMismatch,
        "out_of_range" => AssetEditRejectReason::OutOfRange,
        "unknown_property" => AssetEditRejectReason::UnknownProperty,
        "unknown_asset" => AssetEditRejectReason::UnknownAsset,
        "save_failed" => AssetEditRejectReason::SaveFailed,
        other => AssetEditRejectReason::Other(other.to_owned()),
    };
    let path = data
        .and_then(|value| value.get("path"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    AssetEditError::Rejected(AssetEditRejection {
        reason,
        path,
        message: error.message,
    })
}

/// 作成時に確認した能力を使って、同じセッションへ資産の編集を要求する。
pub struct AssetEditClient {
    handle: DispatchHandle,
    capability_negotiated: bool,
    version: VersionString,
    session_id: Option<String>,
    timeout: Duration,
}

impl AssetEditClient {
    /// 広告された能力を記録する。能力がない場合も作成できるが、要求は送らない。
    pub fn new(
        handle: DispatchHandle,
        capabilities: &CapabilitiesResult,
        version: VersionString,
        session_id: Option<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            handle,
            capability_negotiated: capabilities
                .capabilities
                .iter()
                .any(|capability| capability.name.as_str() == ASSET_EDIT_CAPABILITY),
            version,
            session_id,
            timeout,
        }
    }

    /// 資産のプロパティのスナップショットを取得する。
    pub async fn get_properties(
        &self,
        logical_path: &str,
    ) -> Result<AssetProperties, AssetEditError> {
        self.require_capability()?;
        let params = Map::from_iter([("logicalPath".to_owned(), Value::from(logical_path))]);
        let result = self.request("asset.getProperties", params).await?;
        parse_asset_properties_result(&result)
    }

    /// 不透明なプロパティの位置をそのまま渡し、値を変更する。
    pub async fn set_property(
        &self,
        logical_path: &str,
        path: &str,
        value: Value,
    ) -> Result<AssetSetPropertyResult, AssetEditError> {
        self.require_capability()?;
        let params = Map::from_iter([
            ("logicalPath".to_owned(), Value::from(logical_path)),
            ("path".to_owned(), Value::from(path)),
            ("value".to_owned(), value),
        ]);
        let result = self.request("asset.setProperty", params).await?;
        parse_asset_set_property_result(&result)
    }

    /// 資産を同期保存し、完了応答を待つ。
    pub async fn save(&self, logical_path: &str) -> Result<AssetSaveResult, AssetEditError> {
        self.require_capability()?;
        let params = Map::from_iter([("logicalPath".to_owned(), Value::from(logical_path))]);
        let result = self.request("asset.save", params).await?;
        parse_asset_save_result(&result)
    }

    fn require_capability(&self) -> Result<(), AssetEditError> {
        if self.capability_negotiated {
            Ok(())
        } else {
            Err(AssetEditError::CapabilityNotNegotiated)
        }
    }

    async fn request(
        &self,
        method: &str,
        params: Map<String, Value>,
    ) -> Result<Value, AssetEditError> {
        let next = NEXT_ASSET_EDIT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let id =
            CorrelationId::try_from(format!("asset-edit-{next}")).map_err(RequestError::Encode)?;
        let method = MethodName::try_from(method.to_owned()).map_err(RequestError::Encode)?;
        let request = ValidatedEnvelope::Request {
            version: self.version.clone(),
            id,
            method,
            params: Some(params),
            session_id: self.session_id.clone(),
            seq: None,
        };
        match self.handle.request(request, self.timeout).await? {
            ResponsePayload::Result(result) => Ok(result),
            ResponsePayload::Error(error) => Err(engine_error(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{loopback_pair, Dispatcher, Transport};

    #[tokio::test]
    async fn asset_edit_without_capability_sends_nothing() {
        let (transport, mut peer) = loopback_pair(4);
        let handle = Dispatcher::spawn(transport);
        let capabilities = CapabilitiesResult {
            capabilities: Vec::new(),
        };
        let client = AssetEditClient::new(
            handle.clone(),
            &capabilities,
            VersionString::try_from("0.2".to_owned()).expect("有効なバージョン"),
            None,
            Duration::from_secs(1),
        );
        assert!(matches!(
            client.get_properties("effects/test.json").await,
            Err(AssetEditError::CapabilityNotNegotiated)
        ));
        assert!(matches!(
            client
                .set_property("effects/test.json", "opaque/path", Value::Null)
                .await,
            Err(AssetEditError::CapabilityNotNegotiated)
        ));
        assert!(matches!(
            client.save("effects/test.json").await,
            Err(AssetEditError::CapabilityNotNegotiated)
        ));
        assert!(tokio::time::timeout(Duration::from_millis(30), peer.recv())
            .await
            .is_err());
        handle.shutdown().await;
    }

    #[test]
    fn asset_edit_result_fixtures_follow_schema() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures/methods");
        for method in ["asset.getProperties", "asset.setProperty", "asset.save"] {
            for (directory, expected) in [("positive", true), ("negative", false)] {
                let mut count = 0;
                visit_fixtures(&root.join(method).join(directory), &mut |path| {
                    let fixture: Value =
                        serde_json::from_slice(&std::fs::read(path).expect("fixture を読み込める"))
                            .expect("fixture は JSON");
                    if fixture.get("kind").and_then(Value::as_str) != Some("response") {
                        return;
                    }
                    let Some(result) = fixture.get("result") else {
                        return;
                    };
                    let parsed = match method {
                        "asset.getProperties" => parse_asset_properties_result(result).map(|_| ()),
                        "asset.setProperty" => parse_asset_set_property_result(result).map(|_| ()),
                        "asset.save" => parse_asset_save_result(result).map(|_| ()),
                        _ => unreachable!(),
                    };
                    assert_eq!(parsed.is_ok(), expected, "{}: {parsed:?}", path.display());
                    count += 1;
                });
                assert!(count > 0, "{method}/{directory} の結果を検証した");
            }
        }
    }

    fn visit_fixtures(directory: &Path, visit: &mut impl FnMut(&Path)) {
        let mut paths = std::fs::read_dir(directory)
            .expect("fixture ディレクトリを読み込める")
            .map(|entry| entry.expect("fixture の項目を読み込める").path())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                visit_fixtures(&path, visit);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                visit(&path);
            }
        }
    }
}
