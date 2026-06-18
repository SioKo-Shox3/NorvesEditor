// @brief C++ エンジン SDK の F5 エンドツーエンド ループバックラウンドトリップテスト。
//
// Rust エディタクライアントの loopback_roundtrip.rs の C++ 類似物。
// クライアントトランスポートとエンジントランスポートを make_loopback_pair
// （インプロセス、WebSocket 不使用、F4 BoundedFrameQueue ベース）で接続し、
// エンジン側の BridgeEngineServer を専用スレッドで動作させ、
// 4 つのワイヤーパスをエンドツーエンドで検証する:
//   1. bridge.hello       （型付き HelloParams -> HelloResult）
//   2. engine.getStatus   （-> StatusSnapshot）
//   3. runtime.play       （空 params -> PlayAck）
//   4. log.message イベント  （エンジン emitEvent -> クライアント decode_envelope + DTO）
//
// また、型付き DTO 契約も直接検証する: from_json(to_json(x)) == x（各 DTO）、
// および未知キーが再帰的に拒否されること（bridge.hello.result の入れ子 `server`
// オブジェクトを含む）。
//
// std とSDKの公開ヘッダのみを使用する。境界ルール（include/ に nlohmann を
// 含めない）には影響しない。ctest の合否はプロセス終了コードで決まる。

#include "norves/bridge/adapter.hpp"
#include "norves/bridge/codec.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/events.hpp"
#include "norves/bridge/dto/methods.hpp"
#include "norves/bridge/envelope.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"
#include "norves/bridge/server.hpp"
#include "norves/bridge/transport.hpp"

#include <atomic>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <utility>

#include "test_support.hpp"

namespace
{

    using norves::bridge::BridgeEngineServer;
    using norves::bridge::BridgeError;
    using norves::bridge::decode_envelope;
    using norves::bridge::Envelope;
    using norves::bridge::IBridgeEngineAdapter;
    using norves::bridge::ITransport;
    using norves::bridge::JsonValue;
    using norves::bridge::Kind;
    using norves::bridge::make_loopback_pair;
    using norves::bridge::Result;

    using norves::bridge::dto::EngineState;
    using norves::bridge::dto::HelloParams;
    using norves::bridge::dto::HelloResult;
    using norves::bridge::dto::LogLevel;
    using norves::bridge::dto::LogMessageEvent;
    using norves::bridge::dto::PlayAck;
    using norves::bridge::dto::RuntimeState;
    using norves::bridge::dto::ServerInfo;
    using norves::bridge::dto::StatusSnapshot;

    JsonValue ParseOrFail(std::string_view text)
    {
        auto parsed = JsonValue::parse(text);
        if (parsed.is_err())
        {
            ::norves::test::report_failure("JsonValue::parse failed", __FILE__, __LINE__);
            return JsonValue();
        }
        return std::move(parsed).value();
    }

    // @brief エンジン側アダプタ: ラウンドトリップ対象の 3 メソッドに対し、
    // to_json() でシリアライズした型付き DTO を返す。それ以外のメソッドは
    // デフォルトの METHOD_NOT_SUPPORTED にフォールスルーする（このテストでは呼び出さない）。
    class FakeAdapter : public IBridgeEngineAdapter
    {
    public:
        Result<JsonValue, BridgeError> hello(const JsonValue& /*params*/,
                                             std::string_view selectedProtocolVersion) override
        {
            HelloResult result;
            result.sessionId = "sess-mock-1";
            result.protocolVersion = std::string(selectedProtocolVersion);
            result.server = ServerInfo{"MockEngine", std::optional<std::string>{"0.1.0"},
                                       std::optional<std::string>{"mock"}};
            return Result<JsonValue, BridgeError>::ok(result.to_json());
        }

        Result<JsonValue, BridgeError> getCapabilities(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                ParseOrFail(R"({"capabilities":[{"name":"runtime.control"}]})"));
        }

        Result<JsonValue, BridgeError> getStatus(const JsonValue& /*params*/) override
        {
            StatusSnapshot snap;
            snap.engineState = EngineState::Ready;
            snap.runtimeState = RuntimeState::Edit;
            snap.engineName = "MockEngine";
            snap.engineVersion = "0.1.0";
            snap.title = "Mock Game";
            return Result<JsonValue, BridgeError>::ok(snap.to_json());
        }

        Result<JsonValue, BridgeError> launchInfo(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"launched":true})"));
        }

        Result<JsonValue, BridgeError> runtimePlay(const JsonValue& /*params*/) override
        {
            PlayAck ack;
            ack.accepted = true;
            ack.requestedState = RuntimeState::Playing;
            return Result<JsonValue, BridgeError>::ok(ack.to_json());
        }

        Result<JsonValue, BridgeError> runtimePause(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"runtimeState":"paused"})"));
        }

        Result<JsonValue, BridgeError> runtimeStop(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"runtimeState":"stopped"})"));
        }

        Result<JsonValue, BridgeError> runtimeFocusViewport(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"focused":true})"));
        }

        Result<JsonValue, BridgeError> logSubscribe(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({})"));
        }

        Result<JsonValue, BridgeError> logUnsubscribe(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({})"));
        }
    };

    // @brief ワイヤーリクエストフレームビルダー（dispatch_test のヘルパーと同形状）。
    std::string RequestFrame(std::string_view id, std::string_view method,
                             std::string_view paramsJson)
    {
        std::string frame =
            R"({"bridge":"norves.editor.bridge","version":"0.1","kind":"request","id":")";
        frame += std::string(id);
        frame += R"(","method":")";
        frame += std::string(method);
        frame += R"(")";
        if (!paramsJson.empty())
        {
            frame += R"(,"params":)";
            frame += std::string(paramsJson);
        }
        frame += "}";
        return frame;
    }

    Envelope DecodeOrFail(std::string_view wire)
    {
        auto decoded = decode_envelope(wire);
        if (decoded.is_err())
        {
            ::norves::test::report_failure("decode_envelope failed", __FILE__, __LINE__);
            return Envelope();
        }
        return std::move(decoded).value();
    }

    // @brief エンジン読み出しループ: フレームを取得し、サーバを通じてディスパッチし、
    // レスポンスがあれば返送する。recv() がクリーン EOF（クライアントがクローズ）を
    // 報告したら終了するため、呼び出し元のテストがデターミニスティックに join できる。
    void RunEngine(ITransport& engine, BridgeEngineServer& server, const std::string& logEventFrame,
                   std::atomic<bool>& emitLog)
    {
        while (true)
        {
            std::optional<std::string> frame = engine.recv();
            if (!frame.has_value())
            {
                return;  // ピアがクローズし、インバウンドキューがドレインされた。
            }
            std::optional<std::string> response = server.handleFrame(*frame);
            if (response.has_value())
            {
                if (!engine.send(std::move(*response)))
                {
                    return;  // フライト中にピアがいなくなった。
                }
            }
            // クライアントは log.subscribe を送信する直前に emitLog をセットするため、
            // subscribe フレームを処理するイテレーションでこのフラグを検出する。
            // そのイテレーションでは、上記の subscribe レスポンスを送信した AFTER に
            // ちょうど 1 つの log.message イベントを発行する。レスポンスの後に発行することで、
            // 配送が決定論的になる: ack がクライアントのインバウンドキューにイベントより先に
            // 到達するため、クライアントはその固定順序で recv() する。
            if (emitLog.exchange(false))
            {
                if (!engine.send(std::string(logEventFrame)))
                {
                    return;
                }
            }
        }
    }

    // --- エンドツーエンド ラウンドトリップ -------------------------------------------

    void TestLoopbackRoundTrip()
    {
        auto [client, engine] = make_loopback_pair(16);

        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        // subscribe ack の後にエンジンが発行する log.message イベント。
        LogMessageEvent log;
        log.level = LogLevel::Info;
        log.message = "Game started";
        log.category = "Engine";
        const std::string logEventFrame = server.emitEvent("log.message", log.to_json());

        std::atomic<bool> emitLog{false};
        std::thread engineThread(RunEngine, std::ref(*engine), std::ref(server),
                                 std::cref(logEventFrame), std::ref(emitLog));

        // 1. bridge.hello -------------------------------------------------------
        HelloParams hello;
        hello.role = "editor";
        hello.clientName = "NorvesEditor";
        hello.protocolVersions = {"0.1"};
        client->send(RequestFrame("req-hello", "bridge.hello", hello.to_json().dump()));

        {
            std::optional<std::string> resp = client->recv();
            NORVES_CHECK(resp.has_value());
            if (resp.has_value())
            {
                const Envelope env = DecodeOrFail(*resp);
                NORVES_CHECK(env.kind == Kind::Response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-hello"});
                NORVES_CHECK(env.result.has_value());
                if (env.result.has_value())
                {
                    auto parsed = HelloResult::from_json(*env.result);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok())
                    {
                        const HelloResult& r = parsed.value();
                        NORVES_CHECK_EQ(r.sessionId, std::string{"sess-mock-1"});
                        NORVES_CHECK_EQ(r.protocolVersion, std::string{"0.1"});
                        NORVES_CHECK_EQ(r.server.name, std::string{"MockEngine"});
                        NORVES_CHECK_EQ(r.server.engine, std::optional<std::string>{"mock"});
                    }
                }
            }
        }

        // 2. engine.getStatus ---------------------------------------------------
        client->send(RequestFrame("req-status", "engine.getStatus", ""));
        {
            std::optional<std::string> resp = client->recv();
            NORVES_CHECK(resp.has_value());
            if (resp.has_value())
            {
                const Envelope env = DecodeOrFail(*resp);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-status"});
                NORVES_CHECK(env.result.has_value());
                if (env.result.has_value())
                {
                    auto parsed = StatusSnapshot::from_json(*env.result);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok())
                    {
                        const StatusSnapshot& s = parsed.value();
                        NORVES_CHECK(s.engineState == EngineState::Ready);
                        NORVES_CHECK(s.runtimeState == RuntimeState::Edit);
                        NORVES_CHECK_EQ(s.engineName, std::optional<std::string>{"MockEngine"});
                        NORVES_CHECK_EQ(s.title, std::optional<std::string>{"Mock Game"});
                    }
                }
            }
        }

        // 3. runtime.play（空 params）-------------------------------------------
        client->send(RequestFrame("req-play", "runtime.play", "{}"));
        {
            std::optional<std::string> resp = client->recv();
            NORVES_CHECK(resp.has_value());
            if (resp.has_value())
            {
                const Envelope env = DecodeOrFail(*resp);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-play"});
                NORVES_CHECK(env.result.has_value());
                if (env.result.has_value())
                {
                    auto parsed = PlayAck::from_json(*env.result);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok())
                    {
                        const PlayAck& a = parsed.value();
                        NORVES_CHECK(a.accepted);
                        NORVES_CHECK(a.requestedState.has_value() &&
                                     *a.requestedState == RuntimeState::Playing);
                    }
                }
            }
        }

        // 4. log.message イベント -----------------------------------------------
        // subscribe 後、エンジンが ack の後に 1 イベントを発行する。ack とイベントはどちらも
        // クライアントのインバウンドキューにその順番で到着する。
        emitLog.store(true);
        client->send(RequestFrame("req-logsub", "log.subscribe", ""));
        {
            std::optional<std::string> ack = client->recv();
            NORVES_CHECK(ack.has_value());  // log.subscribe レスポンス。
            if (ack.has_value())
            {
                const Envelope env = DecodeOrFail(*ack);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-logsub"});
                NORVES_CHECK(env.result.has_value());
            }

            std::optional<std::string> event = client->recv();
            NORVES_CHECK(event.has_value());  // log.message イベント。
            if (event.has_value())
            {
                const Envelope env = DecodeOrFail(*event);
                NORVES_CHECK(env.kind == Kind::Event);
                NORVES_CHECK_EQ(env.event, std::optional<std::string>{"log.message"});
                NORVES_CHECK(env.params.has_value());
                if (env.params.has_value())
                {
                    auto parsed = LogMessageEvent::from_json(*env.params);
                    NORVES_CHECK(parsed.is_ok());
                    if (parsed.is_ok())
                    {
                        const LogMessageEvent& e = parsed.value();
                        NORVES_CHECK(e.level == LogLevel::Info);
                        NORVES_CHECK_EQ(e.message, std::string{"Game started"});
                    }
                }
            }
        }

        // 順序ある終了: クライアントのアウトバウンド方向をクローズし、
        // エンジンの recv() がドレインされて nullopt を返し、ループを終了させた後 join する。
        client->close();
        engineThread.join();
    }

    // --- DTO ラウンドトリップ + 未知キー拒否 ----------------------------------------

    void TestDtoRoundTrips()
    {
        {
            HelloParams x;
            x.role = "editor";
            x.clientName = "NorvesEditor";
            x.clientVersion = "0.9.0";
            x.protocolVersions = {"0.1", "1.0"};
            auto back = HelloParams::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            HelloResult x;
            x.sessionId = "s-1";
            x.protocolVersion = "0.1";
            x.server =
                ServerInfo{"E", std::optional<std::string>{"1.2"}, std::optional<std::string>{"e"}};
            auto back = HelloResult::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            ServerInfo x{"OnlyName", std::nullopt, std::nullopt};
            auto back = ServerInfo::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            StatusSnapshot x;
            x.engineState = EngineState::Running;
            x.runtimeState = RuntimeState::Paused;
            x.engineName = "E";
            // engineVersion / title を未設定にして「省略-未設定時」を検証する。
            auto back = StatusSnapshot::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            PlayAck x;
            x.accepted = true;
            x.requestedState = RuntimeState::Playing;
            auto back = PlayAck::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            PlayAck x;  // requestedState 未設定。
            x.accepted = false;
            auto back = PlayAck::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
        {
            LogMessageEvent x;
            x.level = LogLevel::Warn;
            x.message = "careful";
            x.category = "Render";
            x.timestamp = "2026-01-01T00:00:00Z";
            auto back = LogMessageEvent::from_json(x.to_json());
            NORVES_CHECK(back.is_ok());
            NORVES_CHECK(back.is_ok() && back.value() == x);
        }
    }

    void TestUnknownKeyRejected()
    {
        // トップレベルの未知キー。
        {
            auto bad = HelloResult::from_json(ParseOrFail(
                R"({"sessionId":"s","protocolVersion":"0.1","server":{"name":"E"},"extra":1})"));
            NORVES_CHECK(bad.is_err());
        }
        // 入れ子の `server` オブジェクト内の未知キー（再帰的 additionalProperties）。
        {
            auto bad = HelloResult::from_json(ParseOrFail(
                R"({"sessionId":"s","protocolVersion":"0.1","server":{"name":"E","rogue":true}})"));
            NORVES_CHECK(bad.is_err());
        }
        // params 内の未知キー。
        {
            auto bad = HelloParams::from_json(ParseOrFail(
                R"({"role":"editor","clientName":"N","protocolVersions":["0.1"],"caps":[]})"));
            NORVES_CHECK(bad.is_err());
        }
        // ステータススナップショット内の未知キー。
        {
            auto bad = StatusSnapshot::from_json(
                ParseOrFail(R"({"engineState":"ready","runtimeState":"edit","weird":0})"));
            NORVES_CHECK(bad.is_err());
        }
        // ログイベント内の未知キー。
        {
            auto bad = LogMessageEvent::from_json(
                ParseOrFail(R"({"level":"info","message":"m","mystery":1})"));
            NORVES_CHECK(bad.is_err());
        }
        // 必須フィールドの欠落も拒否される。
        {
            auto bad = StatusSnapshot::from_json(ParseOrFail(R"({"engineState":"ready"})"));
            NORVES_CHECK(bad.is_err());
        }
        // 列挙型の範囲外の値は拒否される。
        {
            auto bad = StatusSnapshot::from_json(
                ParseOrFail(R"({"engineState":"booting","runtimeState":"edit"})"));
            NORVES_CHECK(bad.is_err());
        }
    }

}  // namespace

int main()
{
    TestLoopbackRoundTrip();
    TestDtoRoundTrips();
    TestUnknownKeyRejected();
    return norves::test::summary();
}
