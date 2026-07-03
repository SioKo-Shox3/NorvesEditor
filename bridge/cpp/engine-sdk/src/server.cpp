#include "Norves/Bridge/server.hpp"

#include "Norves/Bridge/adapter.hpp"
#include "Norves/Bridge/codec.hpp"
#include "Norves/Bridge/envelope.hpp"
#include "Norves/Bridge/error.hpp"
#include "Norves/Bridge/json_value.hpp"
#include "Norves/Bridge/log_sink.hpp"
#include "Norves/Bridge/result.hpp"
#include "Norves/Bridge/version.hpp"

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "json_value_impl.hpp"

// BridgeEngineServer の実装。nlohmann/json はこの TU に閉じ込められる。公開の
// server.hpp は std + SDK 値型のみを露出する。
//
// 責務:
//   * 受信ワイヤーフレームを（codec.hpp 経由で）デコードする、
//   * bridge.hello のプロトコルバージョンネゴシエーションを所有する、
//   * 他の既知のリクエストを IBridgeEngineAdapter へディスパッチする、
//   * リクエスト id を反映して、レスポンス（result またはワイヤーエラー）をエンコードする。
namespace Norves::Bridge
{

    namespace
    {

        using nlohmann::json;

        // 具体的な nlohmann::json を opaque な JsonValue へラップする（codec.cpp と同じ
        // ヘルパ形状。nlohmann はこの TU 内に留まる）。
        JsonValue Wrap(json value)
        {
            auto impl = std::make_unique<Detail::JsonValueImpl>(std::move(value));
            return make_json_value(std::move(impl));
        }

        // この SDK がエンベロープ上で話すワイヤープロトコルバージョン。ネゴシエーションは
        // ハンドシェイク result のために対応バージョンを選ぶ。エンベロープのバージョン自体は
        // SDK の単一の対応バージョン（alpha では SupportedProtocolVersions[0]）である。
        std::string EnvelopeVersion() { return std::string(SupportedProtocolVersions.front()); }

        // 成功 result を運ぶレスポンス Envelope を構築し、`id` を反映する。result ペイロードが
        // 文字列 "sessionId" を含む JSON オブジェクトである場合、その値はエンベロープレベルでも
        // 反映される（response-valid.json に一致する。そこではエンベロープの sessionId が
        // result.sessionId を反映する）。
        Envelope MakeResultResponse(const std::string& id, JsonValue result)
        {
            Envelope env;
            env.bridge = std::string(BridgeMarker);
            env.version = EnvelopeVersion();
            env.kind = Kind::Response;
            env.id = id;

            const json& payload = peek(result)->json;
            if (payload.is_object())
            {
                const auto it = payload.find("sessionId");
                if (it != payload.end() && it->is_string())
                {
                    env.session_id = it->get<std::string>();
                }
            }

            env.result = std::move(result);
            return env;
        }

        // ワイヤーエラーを運ぶレスポンス Envelope を構築し、`id` を反映する。
        Envelope MakeErrorResponse(const std::string& id, BridgeError error)
        {
            Envelope env;
            env.bridge = std::string(BridgeMarker);
            env.version = EnvelopeVersion();
            env.kind = Kind::Response;
            env.id = id;
            env.error = std::move(error);
            return env;
        }

        // params["protocolVersions"] を、クライアント選好順の文字列リストとして抽出する。
        // 非文字列要素はスキップされる。不在 / 非配列は空リストを生じる（その場合
        // ネゴシエーションは必然的に unsupported として失敗する）。
        std::vector<std::string> OfferedVersions(const JsonValue& params)
        {
            std::vector<std::string> offered;
            const json& obj = peek(params)->json;
            if (!obj.is_object())
            {
                return offered;
            }
            const auto it = obj.find("protocolVersions");
            if (it == obj.end() || !it->is_array())
            {
                return offered;
            }
            for (const auto& element : *it)
            {
                if (element.is_string())
                {
                    offered.push_back(element.get<std::string>());
                }
            }
            return offered;
        }

        // ネゴシエーション: 提示されたバージョン（クライアント選好順）のうち、
        // SupportedProtocolVersions にも含まれる最初のもの。積が空の場合は std::nullopt。
        std::optional<std::string> NegotiateVersion(const std::vector<std::string>& offered)
        {
            for (const auto& candidate : offered)
            {
                for (const auto& supported : SupportedProtocolVersions)
                {
                    if (candidate == supported)
                    {
                        return candidate;
                    }
                }
            }
            return std::nullopt;
        }

        // PROTOCOL_VERSION_UNSUPPORTED の error.data ペイロードを構築する:
        //   { "offered": <クライアントが提示した配列>, "supported": <SupportedProtocolVersions> }
        JsonValue VersionUnsupportedData(const std::vector<std::string>& offered)
        {
            json data = json::object();
            data["offered"] = offered;
            json supported = json::array();
            for (const auto& version : SupportedProtocolVersions)
            {
                supported.push_back(std::string(version));
            }
            data["supported"] = std::move(supported);
            return Wrap(std::move(data));
        }

    }  // namespace

    // --- Impl --------------------------------------------------------------------

    struct BridgeEngineServer::Impl
    {
        IBridgeEngineAdapter& m_Adapter;
        ILogSink* m_LogSink;

        Impl(IBridgeEngineAdapter& adapterRef, ILogSink* sink)
            : m_Adapter(adapterRef), m_LogSink(sink)
        {
        }

        void log(LogSeverity level, std::string_view message) const
        {
            if (m_LogSink != nullptr)
            {
                m_LogSink->log(level, message);
            }
        }

        // デコード済みのリクエストエンベロープを正しいハンドラへルーティングし、
        // レスポンスエンベロープを返す。
        Envelope dispatch(const Envelope& request)
        {
            const std::string id = *request.id;  // 存在する。リクエストに対して検証済み。
            const std::string& method = *request.method;

            // アダプタの契約は `const JsonValue&` を取る。リクエストが params を省略した
            // 場合は JSON null 値を供給する。
            const JsonValue emptyParams;
            const JsonValue& params = request.params.has_value() ? *request.params : emptyParams;

            if (method == "bridge.hello")
            {
                return handle_hello(id, params);
            }

            // 既知のメソッド -> アダプタディスパッチ。各分岐は Ok->result、Err->error へ
            // 対応付け、id を反映する。
            if (method == "bridge.getCapabilities")
            {
                return finish(id, m_Adapter.getCapabilities(params));
            }
            if (method == "engine.getStatus")
            {
                return finish(id, m_Adapter.getStatus(params));
            }
            if (method == "engine.launchInfo")
            {
                return finish(id, m_Adapter.launchInfo(params));
            }
            if (method == "runtime.play")
            {
                return finish(id, m_Adapter.runtimePlay(params));
            }
            if (method == "runtime.pause")
            {
                return finish(id, m_Adapter.runtimePause(params));
            }
            if (method == "runtime.stop")
            {
                return finish(id, m_Adapter.runtimeStop(params));
            }
            if (method == "runtime.focusViewport")
            {
                return finish(id, m_Adapter.runtimeFocusViewport(params));
            }
            if (method == "log.subscribe")
            {
                return finish(id, m_Adapter.logSubscribe(params));
            }
            if (method == "log.unsubscribe")
            {
                return finish(id, m_Adapter.logUnsubscribe(params));
            }
            if (method == "scene.getTree")
            {
                return finish(id, m_Adapter.sceneGetTree(params));
            }
            if (method == "scene.createObject")
            {
                return finish(id, m_Adapter.sceneCreateObject(params));
            }
            if (method == "scene.deleteObject")
            {
                return finish(id, m_Adapter.sceneDeleteObject(params));
            }
            if (method == "scene.reparentObject")
            {
                return finish(id, m_Adapter.sceneReparentObject(params));
            }
            if (method == "scene.duplicateObject")
            {
                return finish(id, m_Adapter.sceneDuplicateObject(params));
            }
            if (method == "object.getSnapshot")
            {
                return finish(id, m_Adapter.objectGetSnapshot(params));
            }
            if (method == "object.setProperty")
            {
                return finish(id, m_Adapter.objectSetProperty(params));
            }
            if (method == "schema.getSnapshot")
            {
                return finish(id, m_Adapter.schemaGetSnapshot(params));
            }
            if (method == "asset.resolve")
            {
                return finish(id, m_Adapter.assetResolve(params));
            }
            if (method == "asset.getManifest")
            {
                return finish(id, m_Adapter.assetGetManifest(params));
            }
            if (method == "viewport.getThumbnail")
            {
                return finish(id, m_Adapter.viewportGetThumbnail(params));
            }

            // 未知のメソッド（ディスパッチテーブルにない）-> METHOD_NOT_SUPPORTED。
            log(LogSeverity::Debug, std::string("unknown method: ") + method);
            return MakeErrorResponse(
                id, BridgeError{std::string(ErrorMethodNotSupported),
                                std::string("Unknown method: ") + method, std::nullopt});
        }

        // bridge.hello: サーバが所有するバージョンネゴシエーションを行い、その後 result
        // ペイロードについてはアダプタへ委譲する。
        Envelope handle_hello(const std::string& id, const JsonValue& params)
        {
            const std::vector<std::string> offered = OfferedVersions(params);
            const std::optional<std::string> selected = NegotiateVersion(offered);
            if (!selected.has_value())
            {
                log(LogSeverity::Warn, "bridge.hello: no offered protocol version is supported");
                return MakeErrorResponse(
                    id, BridgeError{std::string(ErrorProtocolVersionUnsupported),
                                    "None of the offered protocol versions are supported by this "
                                    "engine.",
                                    VersionUnsupportedData(offered)});
            }
            return finish(id, m_Adapter.hello(params, *selected));
        }

        // アダプタの結果をレスポンスエンベロープへ対応付け、id を反映する。
        Envelope finish(const std::string& id, Result<JsonValue, BridgeError> outcome)
        {
            if (outcome.is_ok())
            {
                return MakeResultResponse(id, std::move(outcome).value());
            }
            return MakeErrorResponse(id, std::move(outcome).error());
        }
    };

    // --- BridgeEngineServer ------------------------------------------------------

    BridgeEngineServer::BridgeEngineServer(IBridgeEngineAdapter& adapter, ILogSink* logSink)
        : m_Impl(std::make_unique<Impl>(adapter, logSink))
    {
    }

    BridgeEngineServer::~BridgeEngineServer() = default;

    BridgeEngineServer::BridgeEngineServer(BridgeEngineServer&&) noexcept = default;

    BridgeEngineServer& BridgeEngineServer::operator=(BridgeEngineServer&&) noexcept = default;

    std::optional<std::string> BridgeEngineServer::handleFrame(std::string_view wire)
    {
        auto decoded = decode_envelope(wire);
        if (decoded.is_err())
        {
            // 回復可能な相関 id がない -> 有効なレスポンスエンベロープを構築できない。
            // 報告してフレームをドロップする。
            m_Impl->log(LogSeverity::Warn,
                        std::string("dropping undecodable frame: ") + decoded.error().message);
            return std::nullopt;
        }

        const Envelope request = std::move(decoded).value();
        if (request.kind != Kind::Request)
        {
            // サーバはリクエストのみを処理する。レスポンス/イベントは我々が応答すべき
            // ものではない。
            m_Impl->log(LogSeverity::Debug, "ignoring non-request frame");
            return std::nullopt;
        }

        const Envelope response = m_Impl->dispatch(request);
        auto encoded = encode_envelope(response);
        if (encoded.is_err())
        {
            // サーバが構築したエンベロープのエンコードは失敗しないはずである。もし失敗
            // すれば、送出すべき有効なものは何もない。
            m_Impl->log(LogSeverity::Error,
                        std::string("failed to encode response: ") + encoded.error().message);
            return std::nullopt;
        }
        return std::move(encoded).value();
    }

    std::string BridgeEngineServer::emitEvent(std::string_view eventName, const JsonValue& params)
    {
        Envelope env;
        env.bridge = std::string(BridgeMarker);
        env.version = EnvelopeVersion();
        env.kind = Kind::Event;
        env.event = std::string(eventName);
        env.params = params;

        auto encoded = encode_envelope(env);
        if (encoded.is_err())
        {
            m_Impl->log(LogSeverity::Error,
                        std::string("failed to encode event: ") + encoded.error().message);
            return std::string();
        }
        return std::move(encoded).value();
    }

}  // namespace Norves::Bridge
