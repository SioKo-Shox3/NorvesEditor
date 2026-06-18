#pragma once

// @brief Workstream H-A: 常駐型モックエンジン用エンジン側アダプタ。
//
// この MockAdapter は意図的に engine-sdk/tests/ws_test_server.cpp の FakeAdapter を
// 複製している。ws_test_server は G4 テストアセットであり、e2e テストを壊さないよう
// 手を加えない。二者が乖離した場合は H-D 適合ランナーが検出する。
//
// アダプタはエンジン実装の責務であり SDK サーフェスではないため、examples/ に配置する
// （engine-sdk/src ではない）。std と SDK の公開ヘッダのみに依存する: すべての
// ペイロードは型付き DTO の to_json() または JsonValue::parse から構築し、
// サードパーティ JSON 型を直接扱わない。これにより、このディレクトリから
// libwebsockets / nlohmann のインクルードを排除する。

#include "norves/bridge/adapter.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/methods.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <atomic>
#include <optional>
#include <string>
#include <string_view>

namespace norves::mock
{

    // @brief JSON リテラルをパースするか中断する。以下のリテラルはコンパイル時定数であり、
    // パース失敗はランタイム条件ではなくプログラミングエラーを意味する。
    // モックエンジンには壊れたリテラルに対する回復可能なパスはない。
    inline norves::bridge::JsonValue parse_or_die(std::string_view text)
    {
        auto parsed = norves::bridge::JsonValue::parse(text);
        if (parsed.is_err())
        {
            std::exit(2);
        }
        return std::move(parsed).value();
    }

    // @brief モックエンジンアダプタ。レスポンス値は G4 FakeAdapter と 1 対 1 で一致するため、
    // エディタバックエンドは WebSocket 経由（main.cpp）でモックエンジンを駆動した場合でも
    // ループバックスモークの場合でも同一のワイヤー形状を観察する。
    class MockAdapter : public norves::bridge::IBridgeEngineAdapter
    {
    public:
        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError> hello(
            const norves::bridge::JsonValue& /*params*/,
            std::string_view selectedProtocolVersion) override
        {
            norves::bridge::dto::HelloResult result;
            result.sessionId = "sess-mock-1";
            result.protocolVersion = std::string(selectedProtocolVersion);
            result.server =
                norves::bridge::dto::ServerInfo{"MockEngine", std::optional<std::string>{"0.1.0"},
                                                std::optional<std::string>{"mock"}};
            return norves::bridge::Result<norves::bridge::JsonValue,
                                          norves::bridge::BridgeError>::ok(result.to_json());
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
        getCapabilities(const norves::bridge::JsonValue& /*params*/) override
        {
            // スペックポジティブフィクスチャ
            // （methods/bridge.getCapabilities/positive/response-valid.json）の
            // result.capabilities と値等価にする。H-D 適合ランナーが結果全体を
            // 厳密比較してこのメソッドの乖離を検出できるようにする。
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(
                    R"({"capabilities":[)"
                    R"({"name":"runtime.control","version":"0.1","description":"Play/pause/stop control."},)"
                    R"({"name":"log.stream"},)"
                    R"({"name":"viewport.focus"}]})"));
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError> getStatus(
            const norves::bridge::JsonValue& /*params*/) override
        {
            norves::bridge::dto::StatusSnapshot snap;
            snap.engineState = norves::bridge::dto::EngineState::Ready;
            snap.runtimeState = norves::bridge::dto::RuntimeState::Edit;
            snap.engineName = "MockEngine";
            snap.engineVersion = "0.1.0";
            snap.title = "Mock Game";
            return norves::bridge::Result<norves::bridge::JsonValue,
                                          norves::bridge::BridgeError>::ok(snap.to_json());
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError> launchInfo(
            const norves::bridge::JsonValue& /*params*/) override
        {
            // engine.launchInfo は必須（純粋仮想）メソッドのため、
            // METHOD_NOT_SUPPORTED ではなく最小限の成功結果を返す。
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(R"({"launched":true})"));
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError> runtimePlay(
            const norves::bridge::JsonValue& /*params*/) override
        {
            norves::bridge::dto::PlayAck ack;
            ack.accepted = true;
            ack.requestedState = norves::bridge::dto::RuntimeState::Playing;
            return norves::bridge::Result<norves::bridge::JsonValue,
                                          norves::bridge::BridgeError>::ok(ack.to_json());
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError> runtimePause(
            const norves::bridge::JsonValue& /*params*/) override
        {
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(R"({"accepted":true})"));
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError> runtimeStop(
            const norves::bridge::JsonValue& /*params*/) override
        {
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(R"({"accepted":true})"));
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
        runtimeFocusViewport(const norves::bridge::JsonValue& /*params*/) override
        {
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(R"({"focused":true})"));
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError> logSubscribe(
            const norves::bridge::JsonValue& /*params*/) override
        {
            // この ack が送信された後に log.message バーストを発行するよう recv ループに
            // フラグを立てる。ack-before-event の順序を決定論的に維持する
            // （ws_test_server の FakeAdapter と同じ「フラグセット、ack 後に発行」パターン）。
            emit_log_burst.store(true);
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(R"({"subscribed":true})"));
        }

        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
        logUnsubscribe(const norves::bridge::JsonValue& /*params*/) override
        {
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(R"({"unsubscribed":true})"));
        }

        // @brief logSubscribe() によってセットされ、recv ループが消費する。
        // @note handleFrame とループは同一スレッドで実行されるため、シングルスレッドのハンドオフ。
        // クロスメソッドの契約を明示するため atomic にする。
        std::atomic<bool> emit_log_burst{false};
    };

}  // namespace norves::mock
