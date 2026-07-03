// @brief Workstream H-A: 常駐型モックエンジンのループバックスモーク。
//
// 長時間動作する norves_mock_engine（recv() でブロックするため CTest ターゲットには
// できない）の CTest 登録済み対応物。同一の MockAdapter を BridgeEngineServer の
// 背後でエンジンスレッドに配置し、インプロセスのループバックペアで接続し、
// エディタバックエンドが使用する最小限の起動->駆動パスを実行する:
// hello -> runtime.play -> log.subscribe（+ log.message バースト）。
// これにより実際のソケットを立ち上げることなく、アダプタのワイヤー形状と
// recv ループの emit-after-ack 順序を検証する。
//
// std と SDK の公開ヘッダ（test_support.hpp を含む）のみを使用する。
// 公開ヘッダ境界には影響しない。ctest の合否はプロセス終了コードで決まる。
//
// 終了: クライアントがエンドポイントをクローズし、エンジンの recv() が
// nullopt にドレインされてエンジンループが終了するため、エンジンスレッドが
// 決定論的に join される（ハングなし）。

#include "Norves/Bridge/codec.hpp"
#include "Norves/Bridge/Dto/common.hpp"
#include "Norves/Bridge/Dto/events.hpp"
#include "Norves/Bridge/Dto/methods.hpp"
#include "Norves/Bridge/envelope.hpp"
#include "Norves/Bridge/json_value.hpp"
#include "Norves/Bridge/server.hpp"
#include "Norves/Bridge/transport.hpp"

#include <atomic>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <utility>

#include "mock_adapter.hpp"
#include "test_support.hpp"

namespace
{

    using Norves::Bridge::BridgeEngineServer;
    using Norves::Bridge::decode_envelope;
    using Norves::Bridge::Envelope;
    using Norves::Bridge::ITransport;
    using Norves::Bridge::JsonValue;
    using Norves::Bridge::Kind;
    using Norves::Bridge::make_loopback_pair;

    using Norves::Bridge::Dto::HelloResult;
    using Norves::Bridge::Dto::LogLevel;
    using Norves::Bridge::Dto::LogMessageEvent;
    using Norves::Bridge::Dto::PlayAck;
    using Norves::Bridge::Dto::RuntimeState;

    using norves::mock::MockAdapter;

    // @brief 常駐 recv ループの動作に準拠する: レスポンスを送信した後、アダプタが
    // log.subscribe フラグを立てていれば log.message バーストを発行する。
    // クリーン EOF（クライアントクローズ）で終了し、main.cpp のループ終了を
    // close() 経由でミラーする。
    constexpr int LogBurst = 3;

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

    // @brief エンジン読み出しループ。main.cpp の常駐ループと構造的に同一だが
    // ループバックトランスポートで駆動する。recv() がクリーン EOF を返したら終了する。
    void RunEngine(ITransport& engine, BridgeEngineServer& server, MockAdapter& adapter,
                   const std::string& logEventFrame)
    {
        while (true)
        {
            std::optional<std::string> frame = engine.recv();
            if (!frame.has_value())
            {
                return;  // クライアントがクローズし、インバウンドキューがドレインされた。
            }
            std::optional<std::string> response = server.handleFrame(*frame);
            if (response.has_value())
            {
                if (!engine.send(std::move(*response)))
                {
                    return;
                }
            }
            if (adapter.emit_log_burst.exchange(false))
            {
                for (int i = 0; i < LogBurst; ++i)
                {
                    if (!engine.send(std::string(logEventFrame)))
                    {
                        return;
                    }
                }
            }
        }
    }

    // @brief リクエストワイヤーフレームを構築する（SDK テストのヘルパーと同形状）。
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

    void TestMockEngineLoopback()
    {
        auto [client, engine] = make_loopback_pair(16);

        MockAdapter adapter;
        BridgeEngineServer server(adapter);

        LogMessageEvent log;
        log.level = LogLevel::Info;
        log.message = "Game started";
        log.category = "Engine";
        const std::string logEventFrame = server.emitEvent("log.message", log.to_json());

        std::thread engineThread(RunEngine, std::ref(*engine), std::ref(server), std::ref(adapter),
                                 std::cref(logEventFrame));

        // 1. bridge.hello -------------------------------------------------------
        client->send(RequestFrame(
            "req-hello", "bridge.hello",
            R"({"role":"editor","clientName":"NorvesEditor","protocolVersions":["0.1"]})"));
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

        // 2. runtime.play -------------------------------------------------------
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

        // 3. log.subscribe + log.message バースト --------------------------------
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

            // エンジンは ack の後に LogBurst 個の log.message イベントを順番に発行する。
            for (int i = 0; i < LogBurst; ++i)
            {
                std::optional<std::string> event = client->recv();
                NORVES_CHECK(event.has_value());
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
        }

        // 順序ある終了: クライアントのアウトバウンド方向をクローズし、エンジンの
        // recv() が nullopt にドレインされてループが終了した後 join する（ハングなし）。
        client->close();
        engineThread.join();
    }

}  // namespace

int main()
{
    TestMockEngineLoopback();
    return norves::test::summary();
}
