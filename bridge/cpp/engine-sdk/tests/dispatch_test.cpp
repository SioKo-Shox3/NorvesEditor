// @brief BridgeEngineServer のディスパッチ / ネゴシエーション適合テスト（F3）。
//
// 検証項目: bridge.hello のバージョンネゴシエーション（成功 + PROTOCOL_VERSION_
// UNSUPPORTED）; id / sessionId のエコー; アダプタ結果のパススルー; 未知のメソッドおよび
// 未実装のオプショナルメソッド -> METHOD_NOT_SUPPORTED; イベントの発行;
// リクエスト以外のフレームおよびデコード不能なフレームがレスポンスを生成しないこと。
//
// std とSDKの公開ヘッダのみを使用する（値等価比較のための JsonValue::parse を含む）。
// 境界ルール（include/ に nlohmann を含めない）には影響しない。

#include "Norves/Bridge/adapter.hpp"
#include "Norves/Bridge/codec.hpp"
#include "Norves/Bridge/envelope.hpp"
#include "Norves/Bridge/error.hpp"
#include "Norves/Bridge/json_value.hpp"
#include "Norves/Bridge/result.hpp"
#include "Norves/Bridge/server.hpp"
#include "Norves/Bridge/version.hpp"

#include <optional>
#include <string>
#include <string_view>

#include "test_support.hpp"

namespace
{

    using Norves::Bridge::BridgeEngineServer;
    using Norves::Bridge::BridgeError;
    using Norves::Bridge::CodecError;
    using Norves::Bridge::decode_envelope;
    using Norves::Bridge::Envelope;
    using Norves::Bridge::IBridgeEngineAdapter;
    using Norves::Bridge::JsonValue;
    using Norves::Bridge::Kind;
    using Norves::Bridge::Result;

    // @brief JSON テキストから JsonValue を構築する。パースエラーが発生した場合は
    // テストを失敗させ（null を返す）、呼び出し元がその値をインラインで使えるようにする。
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

    // @brief 偽アダプタ: テストがサーバのディスパッチ / ネゴシエーションを検証するために
    // 固定の JsonValue 結果を返す。実際のエンジンロジックは検証しない。
    // オプショナルメソッドはオーバーライドしないため、デフォルトの
    // METHOD_NOT_SUPPORTED にフォールスルーする。
    class FakeAdapter : public IBridgeEngineAdapter
    {
    public:
        Result<JsonValue, BridgeError> hello(const JsonValue& /*params*/,
                                             std::string_view selectedProtocolVersion) override
        {
            // アダプタはネゴシエート済みバージョンを結果の protocolVersion フィールドに
            // 格納する責務を持つ。
            std::string result =
                std::string(R"({"sessionId":"sess-7f3a","protocolVersion":")") +
                std::string(selectedProtocolVersion) +
                R"(","server":{"name":"FakeEngine","version":"0.1.0","engine":"fake"}})";
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(result));
        }

        Result<JsonValue, BridgeError> getCapabilities(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                ParseOrFail(R"({"capabilities":[{"name":"runtime.control"}]})"));
        }

        Result<JsonValue, BridgeError> getStatus(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"engineState":"ready"})"));
        }

        Result<JsonValue, BridgeError> launchInfo(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"launched":true})"));
        }

        Result<JsonValue, BridgeError> runtimePlay(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"runtimeState":"playing"})"));
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
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"subscribed":true})"));
        }

        Result<JsonValue, BridgeError> logUnsubscribe(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"subscribed":false})"));
        }
        // オプショナルメソッドは意図的にオーバーライドしない。
    };

    // @brief オプショナルな scene/object メソッドをオーバーライドする偽アダプタ。
    // サーバがオプショナルメソッドのディスパッチ分岐を正しく配線していること（result の
    // パススルー）を検証する。FakeAdapter（オーバーライドしない）が
    // METHOD_NOT_SUPPORTED を返すこととは別の経路。
    class OptionalMethodAdapter : public FakeAdapter
    {
    public:
        Result<JsonValue, BridgeError> sceneGetTree(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                ParseOrFail(R"({"root":{"id":"n-0","name":"Root"}})"));
        }

        Result<JsonValue, BridgeError> objectGetSnapshot(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"objectId":"n-1"})"));
        }

        Result<JsonValue, BridgeError> sceneCreateObject(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                ParseOrFail(R"({"accepted":true,"newId":"n-new"})"));
        }

        Result<JsonValue, BridgeError> sceneDeleteObject(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"accepted":true})"));
        }

        Result<JsonValue, BridgeError> sceneReparentObject(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(ParseOrFail(R"({"accepted":true})"));
        }

        Result<JsonValue, BridgeError> sceneDuplicateObject(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                ParseOrFail(R"({"accepted":true,"newId":"n-new"})"));
        }

        Result<JsonValue, BridgeError> viewportGetThumbnail(const JsonValue& /*params*/) override
        {
            return Result<JsonValue, BridgeError>::ok(
                ParseOrFail(R"({"imageBase64":"AAAA","mimeType":"image/png"})"));
        }
    };

    // ワイヤーフレームビルダー -----------------------------------------------------------

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

    // @brief サーバのレスポンスフレームをデコードする。エラー時はテストを失敗させる。
    Envelope DecodeOrFail(std::string_view wire)
    {
        auto decoded = decode_envelope(wire);
        if (decoded.is_err())
        {
            ::norves::test::report_failure("decode_envelope of response failed", __FILE__,
                                           __LINE__);
            return Envelope();
        }
        return std::move(decoded).value();
    }

    // テスト -----------------------------------------------------------------------

    void TestHelloSuccessEchoesIdSessionAndVersion()
    {
        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        const std::string frame = RequestFrame(
            "req-1", "bridge.hello",
            R"({"role":"editor","clientName":"NorvesEditor","protocolVersions":["0.1"]})");
        auto response = server.handleFrame(frame);
        NORVES_CHECK(response.has_value());
        if (!response.has_value())
        {
            return;
        }

        const Envelope env = DecodeOrFail(*response);
        NORVES_CHECK(env.kind == Kind::Response);
        NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-1"});
        NORVES_CHECK(env.result.has_value());
        NORVES_CHECK(!env.error.has_value());
        // エンベロープの sessionId は結果の sessionId からエコーされる。
        NORVES_CHECK_EQ(env.session_id, std::optional<std::string>{"sess-7f3a"});

        // 結果ペイロードはネゴシエート済みバージョンとサーバ識別子を持つ。
        const JsonValue expected = ParseOrFail(
            R"({"sessionId":"sess-7f3a","protocolVersion":"0.1","server":{"name":"FakeEngine","version":"0.1.0","engine":"fake"}})");
        NORVES_CHECK(env.result.has_value() && *env.result == expected);
    }

    void TestHelloVersionUnsupported()
    {
        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        const std::string frame = RequestFrame(
            "req-1", "bridge.hello",
            R"({"role":"editor","clientName":"NorvesEditor","protocolVersions":["2.0"]})");
        auto response = server.handleFrame(frame);
        NORVES_CHECK(response.has_value());
        if (!response.has_value())
        {
            return;
        }

        const Envelope env = DecodeOrFail(*response);
        NORVES_CHECK(env.kind == Kind::Response);
        NORVES_CHECK_EQ(env.id, std::optional<std::string>{"req-1"});
        NORVES_CHECK(env.error.has_value());
        NORVES_CHECK(!env.result.has_value());
        if (!env.error.has_value())
        {
            return;
        }
        NORVES_CHECK_EQ(env.error->code, std::string{"PROTOCOL_VERSION_UNSUPPORTED"});
        NORVES_CHECK(!env.error->message.empty());

        // error.data: offered はクライアントが送信した値、supported は
        // SupportedProtocolVersions（0.2 へ bump 後は "0.2","0.1"）。カノニカルフィクスチャ
        // （response-version-unsupported.json）は offered:["2.0"]、
        // supported:["0.2","0.1"] を使用する。supported は SDK の実際のセットを追跡するため、
        // 構造 + offered + supported の内容を直接アサートする。
        NORVES_CHECK(env.error->data.has_value());
        if (env.error->data.has_value())
        {
            const JsonValue expectedData =
                ParseOrFail(R"({"offered":["2.0"],"supported":["0.2","0.1"]})");
            NORVES_CHECK(*env.error->data == expectedData);
        }
    }

    void TestKnownMethodPassesAdapterResult()
    {
        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        {
            const std::string frame = RequestFrame("s-1", "engine.getStatus", "");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"s-1"});
                NORVES_CHECK(env.result.has_value());
                const JsonValue expected = ParseOrFail(R"({"engineState":"ready"})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }

        {
            const std::string frame = RequestFrame("p-1", "runtime.play", "");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"p-1"});
                const JsonValue expected = ParseOrFail(R"({"runtimeState":"playing"})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }
    }

    void TestUnknownMethodIsMethodNotSupported()
    {
        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        const std::string frame = RequestFrame("u-1", "foo.bar", "");
        auto response = server.handleFrame(frame);
        NORVES_CHECK(response.has_value());
        if (!response.has_value())
        {
            return;
        }
        const Envelope env = DecodeOrFail(*response);
        NORVES_CHECK_EQ(env.id, std::optional<std::string>{"u-1"});
        NORVES_CHECK(env.error.has_value());
        if (env.error.has_value())
        {
            NORVES_CHECK_EQ(env.error->code, std::string{"METHOD_NOT_SUPPORTED"});
        }
    }

    void TestUnimplementedOptionalMethodsAreMethodNotSupported()
    {
        FakeAdapter adapter;  // オプショナルメソッドをオーバーライドしない
        BridgeEngineServer server(adapter);

        const std::string methods[] = {"scene.getTree",         "scene.createObject",
                                       "scene.deleteObject",    "scene.reparentObject",
                                       "scene.duplicateObject", "asset.resolve",
                                       "asset.getManifest"};
        for (std::string_view method : methods)
        {
            const std::string frame = RequestFrame("o-1", method, "");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (!response.has_value())
            {
                continue;
            }
            const Envelope env = DecodeOrFail(*response);
            NORVES_CHECK_EQ(env.id, std::optional<std::string>{"o-1"});
            NORVES_CHECK(env.error.has_value());
            if (env.error.has_value())
            {
                NORVES_CHECK_EQ(env.error->code, std::string{"METHOD_NOT_SUPPORTED"});
            }
        }
    }

    void TestOptionalMethodPassesAdapterResult()
    {
        OptionalMethodAdapter adapter;  // scene/object optional methods をオーバーライドする
        BridgeEngineServer server(adapter);

        {
            const std::string frame = RequestFrame("sc-1", "scene.getTree", "");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"sc-1"});
                NORVES_CHECK(!env.error.has_value());
                const JsonValue expected = ParseOrFail(R"({"root":{"id":"n-0","name":"Root"}})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }

        {
            const std::string frame = RequestFrame("ob-1", "object.getSnapshot", "");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"ob-1"});
                NORVES_CHECK(!env.error.has_value());
                const JsonValue expected = ParseOrFail(R"({"objectId":"n-1"})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }

        {
            const std::string frame = RequestFrame("cr-1", "scene.createObject", R"({"parentId":"n-0"})");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"cr-1"});
                NORVES_CHECK(!env.error.has_value());
                const JsonValue expected = ParseOrFail(R"({"accepted":true,"newId":"n-new"})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }

        {
            const std::string frame = RequestFrame("del-1", "scene.deleteObject", R"({"objectId":"n-1"})");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"del-1"});
                NORVES_CHECK(!env.error.has_value());
                const JsonValue expected = ParseOrFail(R"({"accepted":true})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }

        {
            const std::string frame = RequestFrame("rp-1", "scene.reparentObject", R"({"objectId":"n-1"})");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"rp-1"});
                NORVES_CHECK(!env.error.has_value());
                const JsonValue expected = ParseOrFail(R"({"accepted":true})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }

        {
            const std::string frame = RequestFrame("dp-1", "scene.duplicateObject", R"({"objectId":"n-1"})");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"dp-1"});
                NORVES_CHECK(!env.error.has_value());
                const JsonValue expected = ParseOrFail(R"({"accepted":true,"newId":"n-new"})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }

        {
            const std::string frame = RequestFrame("vp-1", "viewport.getThumbnail", "");
            auto response = server.handleFrame(frame);
            NORVES_CHECK(response.has_value());
            if (response.has_value())
            {
                const Envelope env = DecodeOrFail(*response);
                NORVES_CHECK_EQ(env.id, std::optional<std::string>{"vp-1"});
                NORVES_CHECK(!env.error.has_value());
                const JsonValue expected =
                    ParseOrFail(R"({"imageBase64":"AAAA","mimeType":"image/png"})");
                NORVES_CHECK(env.result.has_value() && *env.result == expected);
            }
        }
    }

    void TestEmitEventRoundTrips()
    {
        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        const JsonValue params = ParseOrFail(R"({"level":"info","message":"hello"})");
        const std::string wire = server.emitEvent("log.message", params);
        NORVES_CHECK(!wire.empty());

        const Envelope env = DecodeOrFail(wire);
        NORVES_CHECK(env.kind == Kind::Event);
        NORVES_CHECK_EQ(env.event, std::optional<std::string>{"log.message"});
        NORVES_CHECK(env.params.has_value() && *env.params == params);
    }

    // @brief JsonValue 自体のサニティチェック: 非自明な値が parse -> dump -> parse の
    // ラウンドトリップを経て元の値と値等価であることを確認する。これはディスパッチパスが
    // result / error / event ペイロードすべてに依存するコーデックを保護する。
    void TestJsonValueParseDumpRoundTrips()
    {
        constexpr std::string_view Source = R"({"a":1,"b":[true,null,"x"],"c":{"d":2.5}})";

        auto first = JsonValue::parse(Source);
        NORVES_CHECK(first.is_ok());
        if (first.is_err())
        {
            return;
        }
        const JsonValue original = std::move(first).value();

        auto second = JsonValue::parse(original.dump());
        NORVES_CHECK(second.is_ok());
        if (second.is_err())
        {
            return;
        }
        const JsonValue reparsed = std::move(second).value();

        NORVES_CHECK(reparsed == original);
    }

    void TestNonRequestFrameReturnsNullopt()
    {
        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        // 有効なレスポンスフレームをサーバに与える: 我々が応答すべきものではない。
        const std::string responseFrame =
            R"({"bridge":"norves.editor.bridge","version":"0.1","kind":"response","id":"req-1","result":{"ok":true}})";
        auto out = server.handleFrame(responseFrame);
        NORVES_CHECK(!out.has_value());
    }

    void TestUndecodableFrameReturnsNullopt()
    {
        FakeAdapter adapter;
        BridgeEngineServer server(adapter);

        auto out = server.handleFrame("{ this is not valid json");
        NORVES_CHECK(!out.has_value());
    }

}  // namespace

int main()
{
    TestHelloSuccessEchoesIdSessionAndVersion();
    TestHelloVersionUnsupported();
    TestKnownMethodPassesAdapterResult();
    TestUnknownMethodIsMethodNotSupported();
    TestUnimplementedOptionalMethodsAreMethodNotSupported();
    TestOptionalMethodPassesAdapterResult();
    TestEmitEventRoundTrips();
    TestJsonValueParseDumpRoundTrips();
    TestNonRequestFrameReturnsNullopt();
    TestUndecodableFrameReturnsNullopt();
    return norves::test::summary();
}
