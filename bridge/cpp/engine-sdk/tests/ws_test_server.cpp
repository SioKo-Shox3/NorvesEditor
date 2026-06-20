// @brief Workstream G / G4 エンドツーエンドテストハーネス: Rust エディタクライアントの
// e2e テスト（ws_roundtrip.rs）が実ローカルソケット経由で駆動するスタンドアロンの
// WebSocket エンジンサーバープロセス。
//
// これはテストハーネスであり、本番コードではない: tests/ 配下でビルドされ（src/ では
// ビルドされない）、Workstream H モックエンジンとは意図的に別の実行可能ファイルとし、
// CTest には登録しない（Rust が直接起動するため、CTest エントリがあると二重実行となる）。
// エンジン SDK にリンクし、呼び出し元が指定したポートにバインドした実際の
// WebSocketServerTransport を立ち上げる。
//
// Rust 側とのライフサイクル契約:
//   * argv: --bridge-port <p> は必須。不正・欠落ポートはハードエラー（非ゼロ終了）と
//     なるため、設定ミスのあるテストは即座に失敗する。
//   * バインド成功後は "READY <port>\n" を stdout に出力してフラッシュする。
//     Rust ハーネスはダイヤル前にこれを待機する。stdout はこの 1 行専用とし、
//     診断はすべて stderr に出力する。
//   * その後、シングル recv ループを実行する: 受信した各ワイヤーフレームを
//     BridgeEngineServer::handleFrame に渡し、レスポンスがあれば返送する。
//     log.subscribe を ack した後は、順序付き複数フレーム配送（G4 バースト要件）を
//     検証するために SEVERAL 個の log.message イベントを連続発行する。
//   * recv() が nullopt を返した場合はクライアントがクローズしたことを意味し、
//     ループを終了してプロセスが 0 で終了する。
//
// G5 追加（テスト専用、src 変更なし）:
//   * --inject-malformed: 最初のインバウンドフレームを処理してそのレスポンスを送信した後、
//     ハーネスは不正形式（非 JSON）のテキストフレームをちょうど 1 つ送信し、
//     その後通常動作を継続する。これにより Rust e2e はディスパッチャがデコード不能な
//     フレームをログして破棄する（非致命的）ことを証明し、後続リクエストへの対応が
//     継続されることを検証できる。
//   * バインドリトライ: Rust の再接続テストによる kill->同一ポート再起動により、
//     一時的なバインド失敗（TIME_WAIT 等）が発生することがある。ハーネスは諦める前に
//     短い sleep を挟みながら数回リトライするため、「kill -> 同一ポート再バインド ->
//     再接続」がフレーキーにならない。これはハーネス専用の対処であり、G3 の
//     WebSocketServerTransport src は変更しない。
//
// 境界ルール（SDK 公開 include/ に libwebsockets 型を含めない）は維持される:
// この TU は SDK 公開ヘッダのみをインクルードし、サードパーティの唯一の
// 表面は ITransport pImpl の背後に隠蔽されている。

#include "norves/bridge/adapter.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/dto/events.hpp"
#include "norves/bridge/dto/methods.hpp"
#include "norves/bridge/error.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/result.hpp"
#include "norves/bridge/server.hpp"
#include "norves/bridge/transport.hpp"
#include "norves/bridge/ws_server_transport.hpp"

#include <atomic>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <thread>

namespace
{

    using norves::bridge::BridgeEngineServer;
    using norves::bridge::BridgeError;
    using norves::bridge::IBridgeEngineAdapter;
    using norves::bridge::ILogSink;
    using norves::bridge::ITransport;
    using norves::bridge::JsonValue;
    using norves::bridge::LogSeverity;
    using norves::bridge::make_websocket_server_transport;
    using norves::bridge::Result;

    using norves::bridge::dto::EngineState;
    using norves::bridge::dto::HelloResult;
    using norves::bridge::dto::LogLevel;
    using norves::bridge::dto::LogMessageEvent;
    using norves::bridge::dto::PlayAck;
    using norves::bridge::dto::RuntimeState;
    using norves::bridge::dto::ServerInfo;
    using norves::bridge::dto::StatusSnapshot;

    // @brief log.subscribe の ack 後に発行する log.message イベントの数（連続）。
    // @note 複数にすることで Rust 側が順序付き複数フレーム配送（G4 バースト要件）を
    // アサートできる。SDK トランスポートはインオーダー配送を保証する。
    constexpr int LogBurst = 3;

    // @brief JSON リテラルをパースするか、ハーネスを中断する。
    // @note 以下のリテラルはコンパイル時定数であり、パース失敗はランタイム条件ではなく
    // プログラミングエラーを意味する。
    JsonValue ParseOrDie(std::string_view text)
    {
        auto parsed = JsonValue::parse(text);
        if (parsed.is_err())
        {
            std::cerr << "ws_test_server: internal JSON literal failed to parse\n";
            std::exit(2);
        }
        return std::move(parsed).value();
    }

    // @brief コンパクトな JSON オブジェクトテキストから、トップレベルのフィールド値を生の
    // JSON テキスト（文字列なら引用符込み、数値/真偽値/null/配列/オブジェクトはそのまま）で
    // 取り出す。dump() の出力はネストでも有効な JSON なので、対応する括弧/引用のバランスを
    // 取りながら値トークンの終端を求める。見つからなければ nullopt。
    // @note FakeAdapter は mock_adapter.hpp の MockAdapter を意図的に複製しており、この
    // ヘルパも同値の振る舞いを持つ。
    std::optional<std::string> ExtractJsonField(const std::string& objectText, std::string_view key)
    {
        std::string needle = "\"";
        needle += key;
        needle += "\":";
        const std::size_t keyPos = objectText.find(needle);
        if (keyPos == std::string::npos)
        {
            return std::nullopt;
        }
        std::size_t pos = keyPos + needle.size();
        if (pos >= objectText.size())
        {
            return std::nullopt;
        }
        const std::size_t start = pos;
        int depth = 0;
        bool inString = false;
        bool escaped = false;
        for (; pos < objectText.size(); ++pos)
        {
            const char c = objectText[pos];
            if (inString)
            {
                if (escaped)
                {
                    escaped = false;
                }
                else if (c == '\\')
                {
                    escaped = true;
                }
                else if (c == '"')
                {
                    inString = false;
                }
                continue;
            }
            if (c == '"')
            {
                inString = true;
            }
            else if (c == '{' || c == '[')
            {
                ++depth;
            }
            else if (c == '}' || c == ']')
            {
                if (depth == 0)
                {
                    break;  // 親オブジェクトの閉じ括弧に到達。
                }
                --depth;
            }
            else if ((c == ',' || c == ':') && depth == 0)
            {
                break;  // トップレベルの値区切りに到達。
            }
        }
        return objectText.substr(start, pos - start);
    }

    // @brief コンパクトな JSON オブジェクトテキストから、トップレベルの文字列フィールドの値
    // （引用符なし）を取り出す。見つからない / 文字列でないなら nullopt。
    std::optional<std::string> ExtractStringField(const std::string& objectText,
                                                  std::string_view key)
    {
        const std::optional<std::string> raw = ExtractJsonField(objectText, key);
        if (!raw.has_value())
        {
            return std::nullopt;
        }
        const std::string& value = raw.value();
        if (value.size() >= 2 && value.front() == '"' && value.back() == '"')
        {
            return value.substr(1, value.size() - 2);
        }
        return std::nullopt;
    }

    // @brief 最小限の stderr ログシンク。SDK の Warn/Error 診断をテスト失敗時の
    // 子プロセスキャプチャ済み stderr で確認できるようにする。
    // @note stdout は単一の READY 行に予約されているため、診断は stderr のみに出力する。
    class StderrSink : public ILogSink
    {
    public:
        void log(LogSeverity level, std::string_view message) override
        {
            if (level == LogSeverity::Warn || level == LogSeverity::Error)
            {
                std::cerr << "ws_test_server[sink]: " << message << '\n';
            }
        }
    };

    // @brief エンジン側アダプタ（loopback_roundtrip_test.cpp の FakeAdapter をミラー）:
    // スコープ内の各メソッドに型付き DTO で応答する。logSubscribe はさらにフラグをセットし、
    // recv ループが subscribe ack の後に log.message バーストを発行すべきことを通知する
    // （ループバックテストと同じ「フラグセット、ack 後に発行」パターンだが、
    // テストスレッドではなくアダプタが駆動する）。
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
            // 決定論的な単一のケイパビリティディスクリプタを返す。Rust e2e が
            // 空リストではなく具体的なラウンドトリップをアサートできるようにする。
            // 形状は bridge.getCapabilities.result スキーマに準拠:
            // 名前空間付きトークンと MAJOR.MINOR バージョンを持つ capabilityDescriptor。
            return Result<JsonValue, BridgeError>::ok(
                ParseOrDie(R"({"capabilities":[{"name":"runtime.control","version":"0.1"}]})"));
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
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(R"({"launched":true})"));
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
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(R"({"accepted":true})"));
        }

        Result<JsonValue, BridgeError> runtimeStop(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(R"({"accepted":true})"));
        }

        Result<JsonValue, BridgeError> runtimeFocusViewport(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(R"({"focused":true})"));
        }

        Result<JsonValue, BridgeError> logSubscribe(const JsonValue& /*params*/) override
        {
            // recv ループに対し、この ack が送信された後に log.message バーストを発行するよう
            // フラグを立てる。ack-before-event の順序を決定論的に維持するため。
            emit_log_burst.store(true);
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(R"({"subscribed":true})"));
        }

        Result<JsonValue, BridgeError> logUnsubscribe(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(R"({"unsubscribed":true})"));
        }

        // --- オプション（scene / object / schema）-----------------------------
        //
        // @note これら 4 メソッドの返値は mock_adapter.hpp の MockAdapter と 1 対 1 で一致し、
        // かつ対応するスペックポジティブフィクスチャ（methods/scene.getTree |
        // object.getSnapshot | object.setProperty | schema.getSnapshot /
        // positive/response-valid.json）の result と値等価である。両者が乖離すると H-D 適合
        // ランナーが検出する。値コピーでのみ JsonValue を構築する。

        Result<JsonValue, BridgeError> sceneGetTree(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(
                R"({"root":{"id":"n-0","name":"Root","kind":"object","children":[)"
                R"({"id":"n-1","name":"NodeA","kind":"object"},)"
                R"({"id":"n-2","name":"GroupNode","kind":"object","children":[)"
                R"({"id":"n-3","name":"NodeB"}]}]}})"));
        }

        Result<JsonValue, BridgeError> objectGetSnapshot(const JsonValue& /*params*/) override
        {
            std::string fieldOfView = "60";
            const auto it = m_ObjectFieldOfView.find("n-1");
            if (it != m_ObjectFieldOfView.end())
            {
                fieldOfView = it->second;
            }
            std::string snapshot =
                R"({"objectId":"n-1","name":"NodeA","kind":"object","properties":[)"
                R"({"name":"label","value":"Example Name","valueType":"string"},)"
                R"({"name":"fieldOfView","value":)";
            snapshot += fieldOfView;
            snapshot +=
                R"(,"valueType":"number"},)"
                R"({"name":"enabled","value":true,"valueType":"boolean"},)"
                R"({"name":"parent","value":null},)"
                R"({"name":"position","value":[0,1.5,-10],"valueType":"vector3"},)"
                R"({"name":"metadata","value":{"locked":false,"tag":"primary"}}]})";
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(snapshot));
        }

        // @note 状態更新（m_ObjectFieldOfView への書き込み）は mock のシングルスレッド recv
        // ループ前提でのみ安全である。handleFrame はアダプタを同期・同スレッドで呼ぶため、
        // この可変状態にロックは要らない。将来もマルチスレッド化しないこと。
        Result<JsonValue, BridgeError> objectSetProperty(const JsonValue& params) override
        {
            const std::string paramsText = params.dump();
            const std::optional<std::string> propertyName = ExtractStringField(paramsText, "property");
            const std::optional<std::string> valueText = ExtractJsonField(paramsText, "value");

            if (propertyName.has_value() && propertyName.value() == "fieldOfView" &&
                valueText.has_value())
            {
                m_ObjectFieldOfView["n-1"] = valueText.value();
            }

            std::string ack = R"({"accepted":true,"appliedValue":)";
            ack += valueText.has_value() ? valueText.value() : std::string("null");
            ack += "}";
            return Result<JsonValue, BridgeError>::ok(ParseOrDie(ack));
        }

        Result<JsonValue, BridgeError> schemaGetSnapshot(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                ParseOrDie(R"({"types":[)"
                           R"({"typeName":"TypeA","kind":"object","properties":[)"
                           R"({"name":"fieldOfView","valueType":"number"},)"
                           R"({"name":"enabled","valueType":"boolean"}]},)"
                           R"({"typeName":"TypeB","kind":"component"}]})"));
        }

        // @brief logSubscribe() によってセットされ、recv ループが消費する。
        // @note handleFrame
        // とループは同一スレッドで実行されるため、実質シングルスレッドのハンドオフ。
        // クロスメソッドの契約を明示するため、それでも atomic にする。
        std::atomic<bool> emit_log_burst{false};

    private:
        // @brief objectId -> fieldOfView の現在値（JSON 数値テキスト）。objectSetProperty が
        // 更新し objectGetSnapshot が読む。MockAdapter::object_field_of_view と同役割。
        // @note mock のシングルスレッド recv ループ前提でのみ安全。マルチスレッド化しないこと。
        std::map<std::string, std::string> m_ObjectFieldOfView;
    };

    // @brief --bridge-port を読み取る。成功時はポートを返す。不正・欠落時は
    // エラーを出力して nullopt を返す。
    std::optional<std::uint16_t> ParsePort(int argc, char** argv)
    {
        for (int i = 1; i < argc; ++i)
        {
            std::string_view arg = argv[i];
            if (arg == "--bridge-port")
            {
                if (i + 1 >= argc)
                {
                    std::cerr << "ws_test_server: --bridge-port requires a value\n";
                    return std::nullopt;
                }
                const std::string value = argv[i + 1];
                try
                {
                    const unsigned long parsed = std::stoul(value);
                    if (parsed == 0 || parsed > 65535)
                    {
                        std::cerr << "ws_test_server: port out of range: " << value << '\n';
                        return std::nullopt;
                    }
                    return static_cast<std::uint16_t>(parsed);
                }
                catch (const std::exception&)
                {
                    std::cerr << "ws_test_server: invalid port: " << value << '\n';
                    return std::nullopt;
                }
            }
        }
        std::cerr << "ws_test_server: missing required --bridge-port <port>\n";
        return std::nullopt;
    }

    // @brief コマンドライン引数に --inject-malformed が存在するかを返す。
    bool HasInjectMalformed(int argc, char** argv)
    {
        for (int i = 1; i < argc; ++i)
        {
            if (std::string_view(argv[i]) == "--inject-malformed")
            {
                return true;
            }
        }
        return false;
    }

    // @brief WebSocket サーバーをバインドする。一時的なバインド失敗時は短い sleep を
    // 挟みながらリトライする。kill->同一ポート再起動（Rust 再接続テスト）では、
    // OS が前のリスナーを解放する前に一時的にバインドに失敗することがある。
    // リトライによってフレーキーネスを吸収する。すべての試行が失敗した場合のみ nullptr を返す。
    std::unique_ptr<ITransport> BindWithRetry(std::uint16_t port, std::size_t sendCap,
                                              std::size_t recvCap, ILogSink* sink)
    {
        constexpr int MaxAttempts = 20;
        constexpr auto RetryDelay = std::chrono::milliseconds(100);
        for (int attempt = 0; attempt < MaxAttempts; ++attempt)
        {
            std::unique_ptr<ITransport> transport =
                make_websocket_server_transport(port, sendCap, recvCap, sink);
            if (transport != nullptr)
            {
                return transport;
            }
            std::this_thread::sleep_for(RetryDelay);
        }
        return nullptr;
    }

}  // namespace

int main(int argc, char** argv)
{
    const std::optional<std::uint16_t> port = ParsePort(argc, argv);
    if (!port.has_value())
    {
        return 2;
    }

    const bool bInjectMalformed = HasInjectMalformed(argc, argv);

    constexpr std::size_t SendCap = 256;
    constexpr std::size_t RecvCap = 256;

    StderrSink sink;
    std::unique_ptr<ITransport> transport = BindWithRetry(*port, SendCap, RecvCap, &sink);
    if (transport == nullptr)
    {
        std::cerr << "ws_test_server: failed to bind WebSocket server on port " << *port << '\n';
        return 3;
    }

    FakeAdapter adapter;
    BridgeEngineServer server(adapter, &sink);

    // log.message イベントフレームを事前に 1 回ビルドする。log.subscribe の ack 後に
    // （LogBurst 回）発行される。
    LogMessageEvent log;
    log.level = LogLevel::Info;
    log.message = "Game started";
    log.category = "Engine";
    const std::string logEventFrame = server.emitEvent("log.message", log.to_json());

    // バインド成功後に準備完了を通知する。これにより Rust ハーネスはリッスン済みの
    // ソケットにのみダイヤルする。stdout はこの 1 行専用とし、親プロセスが即座に
    // 受け取れるようにフラッシュする。
    std::cout << "READY " << *port << '\n';
    std::cout.flush();

    // --inject-malformed が設定されている場合、最初のレスポンス送信後に
    // デコード不能なフレームをちょうど 1 つ送信し、Rust 側がログして破棄が
    // 非致命的であることを証明できるようにする。
    bool bMalformedPending = bInjectMalformed;

    // シングル recv ループ。handleFrame とアダプタはこのスレッドで実行されるため、
    // logSubscribe 内でセットされた emit_log_burst フラグは handleFrame が返った直後に
    // 参照できる。subscribe の ack を先に送信し、その後バーストを送るため、
    // クライアントは ack-before-events の順序で受信する。
    while (true)
    {
        std::optional<std::string> frame = transport->recv();
        if (!frame.has_value())
        {
            break;  // クライアントがクローズし、インバウンドキューがドレインされた: クリーン終了。
        }

        std::optional<std::string> response = server.handleFrame(*frame);
        if (response.has_value())
        {
            if (!transport->send(std::move(*response)))
            {
                break;  // フライト中にピアがいなくなった。
            }
        }

        // 最初の実レスポンスが送信された後（クライアントが確実に接続済み）に
        // 不正形式フレームを 1 つ注入する。ディスパッチャはこれをログして破棄（非致命的）し、
        // その後の通常動作を継続しなければならない。
        if (bMalformedPending)
        {
            bMalformedPending = false;
            if (!transport->send(std::string("{not valid json")))
            {
                break;  // フライト中にピアがいなくなった。
            }
        }

        if (adapter.emit_log_burst.exchange(false))
        {
            for (int i = 0; i < LogBurst; ++i)
            {
                if (!transport->send(std::string(logEventFrame)))
                {
                    break;
                }
            }
        }
    }

    return 0;
}
