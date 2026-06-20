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
#include <cstddef>
#include <map>
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
                    R"({"name":"viewport.focus"},)"
                    R"({"name":"scene.query"},)"
                    R"({"name":"object.query"},)"
                    R"({"name":"object.edit"},)"
                    R"({"name":"scene.liveUpdate"},)"
                    R"({"name":"viewport.thumbnail"}]})"));
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

        // --- オプション（scene / object / schema）-----------------------------
        //
        // @note これら 4 メソッドの返値は、それぞれ対応するスペックポジティブフィクスチャ
        // （methods/scene.getTree | object.getSnapshot | object.setProperty |
        // schema.getSnapshot / positive/response-valid.json）の result と値等価にする。
        // H-D 適合ランナーが result 全体を厳密比較してこのメソッドの乖離を検出できるように
        // するため。返値は値コピーでのみ JsonValue を構築し、エンジン内部ポインタや span を
        // 一切渡さない（docs/memory-buffer-policy.md / adapter.hpp のスレッド・所有権規約）。

        // @brief scene.getTree。静的デモシーン（Root -> NodeA / GroupNode -> NodeB）を返す。
        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError> sceneGetTree(
            const norves::bridge::JsonValue& /*params*/) override
        {
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(
                    R"({"root":{"id":"n-0","name":"Root","kind":"object","children":[)"
                    R"({"id":"n-1","name":"NodeA","kind":"object"},)"
                    R"({"id":"n-2","name":"GroupNode","kind":"object","children":[)"
                    R"({"id":"n-3","name":"NodeB"}]}]}})"));
        }

        // @brief object.getSnapshot。params.objectId に対応するプロパティバッグを返す。
        //
        // n-1 経路（適合テスト対象）は従来どおり: 可変プロパティ（fieldOfView）の現在値を
        // インメモリ静的マップ object_field_of_view から引き、デモテンプレートに差し込む。
        // これにより objectSetProperty による更新が後続の getSnapshot に反映される。n-1 の
        // 返値は正典フィクスチャ（object.getSnapshot/positive/response-valid.json）と値等価で
        // あり、H-D 適合の exact-match を一切壊さない。
        //
        // 他の既知ノード（n-0 Root / n-2 GroupNode / n-3 NodeB。scene.getTree のツリーと整合）
        // には小さなデモプロパティ集合を返す。これにより Outliner で任意ノードを選ぶと Inspector
        // が表示される（per-node 化）。未知 id は空の propertyBag を返す。すべて値コピーのみで
        // JsonValue を構築し、エンジン内部ポインタや span を渡さない（memory-buffer-policy）。
        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
        objectGetSnapshot(const norves::bridge::JsonValue& params) override
        {
            const std::string paramsText = params.dump();
            const std::optional<std::string> objectId = extract_string_field(paramsText, "objectId");
            const std::string id = objectId.value_or("n-1");

            // n-1: 適合フィクスチャと値等価の経路（温存）。fieldOfView は可変。
            if (id == "n-1")
            {
                std::string fieldOfView = "60";
                const auto it = object_field_of_view.find("n-1");
                if (it != object_field_of_view.end())
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
                return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                    ok(parse_or_die(snapshot));
            }

            // 他の既知ノード: scene.getTree のツリー（Root/GroupNode/NodeB）と整合する小さな
            // デモプロパティ集合。conformance には現れない additive な経路。
            const std::optional<std::string> demo = demo_snapshot_for(id);
            if (demo.has_value())
            {
                return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                    ok(parse_or_die(demo.value()));
            }

            // 未知 id: 空の propertyBag（必須フィールドのみ）。
            std::string empty = R"({"objectId":")";
            empty += id;
            empty += R"(","properties":[]})";
            return norves::bridge::Result<norves::bridge::JsonValue,
                                          norves::bridge::BridgeError>::ok(parse_or_die(empty));
        }

        // @brief object.setProperty。{accepted:true, appliedValue:<echo>} を返し、インメモリ
        // 静的マップを更新する。appliedValue は params.value をそのままエコーする。
        // @note 状態更新（object_field_of_view への書き込み）は mock のシングルスレッド recv
        // ループ前提でのみ安全である。handleFrame はアダプタを同期・同スレッドで呼ぶため
        // （adapter.hpp のスレッドアフィニティ規約）、この可変状態にロックは要らない。mock を
        // 将来もマルチスレッド化しないこと。Phase 6 の object.changed emit はこの更新済みマップを
        // 同スレッドで読む土台となる。
        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
        objectSetProperty(const norves::bridge::JsonValue& params) override
        {
            // params から objectId / property 名 / value（JSON テキスト）を取り出し、可変プロパティ
            // なら値コピーで内部マップを更新する。JsonValue は opaque なため params 全体を dump し、
            // fieldOfView 宛ての setProperty の場合のみ value を抜き出して objectId をキーにマップへ
            // 写す。フィクスチャ（object.setProperty/positive）は objectId:"n-1",
            // property:"fieldOfView", value:75 で {accepted:true, appliedValue:75} を期待し、
            // n-1/fieldOfView の更新が後続 getSnapshot に 75 として反映される（適合の前提）。
            const std::string paramsText = params.dump();
            const std::optional<std::string> objectId = extract_string_field(paramsText, "objectId");
            const std::optional<std::string> propertyName = extract_string_field(paramsText, "property");
            const std::optional<std::string> valueText = extract_json_field(paramsText, "value");

            if (objectId.has_value() && propertyName.has_value() &&
                propertyName.value() == "fieldOfView" && valueText.has_value())
            {
                object_field_of_view[objectId.value()] = valueText.value();
            }

            // Phase 6: 受理した setProperty の後にライブ更新イベントを発行するよう recv ループに
            // フラグを立てる（logSubscribe と同じ「フラグセット、ack 後に発行」パターン）。
            // 変更対象 id を記録し、object.changed の params を更新済みマップから同スレッドで
            // 組み立てられるようにする。emit は ack の後に行われるため、レスポンスを id で相関し
            // イベントを別扱いする conformance ランナーの exact-match を壊さない。
            if (objectId.has_value())
            {
                last_changed_object_id = objectId.value();
            }
            emit_object_changed.store(true);
            emit_scene_tree_changed.store(true);

            std::string ack = R"({"accepted":true,"appliedValue":)";
            ack += valueText.has_value() ? valueText.value() : std::string("null");
            ack += "}";
            return norves::bridge::Result<norves::bridge::JsonValue,
                                          norves::bridge::BridgeError>::ok(parse_or_die(ack));
        }

        // @brief Phase 6: object.changed イベントの params を構築する。更新済みのインメモリ
        // マップから objectGetSnapshot を同スレッドで読み、{objectId, name, kind, properties} の
        // スナップショットをそのまま params とする（events/object.changed.params.schema.json と
        // 整合）。値コピーのみで JsonValue を構築し、エンジン内部ポインタや span を渡さない
        // （memory-buffer-policy）。シングルスレッド recv ループ前提（objectSetProperty の @note）。
        norves::bridge::JsonValue object_changed_params()
        {
            std::string params = R"({"objectId":")";
            params += last_changed_object_id.empty() ? std::string("n-1") : last_changed_object_id;
            params += R"("})";
            auto snapshot = objectGetSnapshot(parse_or_die(params));
            if (snapshot.is_err())
            {
                std::exit(2);
            }
            return std::move(snapshot).value();
        }

        // @brief Phase 6: scene.treeChanged イベントの params を構築する。変更されたノードの
        // スナップショット DTO（changedNodes）を 1 件返す（events/scene.treeChanged.params.schema.json
        // と整合）。最小トリガとして、setProperty 後に変更ノード 1 件を通知するのみ。値コピーのみ。
        static norves::bridge::JsonValue scene_tree_changed_params()
        {
            return parse_or_die(
                R"({"changedNodes":[{"id":"n-1","name":"NodeA","kind":"object"}],)"
                R"("fullRefreshRequired":false})");
        }

        // @brief schema.getSnapshot。型記述子（typeName + properties[{name,valueType}]）を返す。
        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
        schemaGetSnapshot(const norves::bridge::JsonValue& /*params*/) override
        {
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(
                    R"({"types":[)"
                    R"({"typeName":"TypeA","kind":"object","properties":[)"
                    R"({"name":"fieldOfView","valueType":"number"},)"
                    R"({"name":"enabled","valueType":"boolean"}]},)"
                    R"({"typeName":"TypeB","kind":"component"}]})"));
        }

        // @brief viewport.getThumbnail。小さなテスト用 PNG（2x2、base64 後でも 100 バイト程度
        // で 256 KiB ハードキャップの遥か内）の固定スナップショットを返す。返値は正典フィクスチャ
        // （viewport.getThumbnail/positive/response-valid.json）の result と値等価であり、
        // H-D 適合ランナーが result 全体を厳密比較してこのメソッドの乖離を検出できるようにする。
        // base64 文字列は値コピーで JsonValue を構築し、エンジンのフレームバッファや内部
        // ポインタ・span を一切渡さない（docs/memory-buffer-policy.md の large-payload 戦略:
        // PNG / 最大 640x360 / 256 KiB / 最大 1 fps の pull 型）。
        norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>
        viewportGetThumbnail(const norves::bridge::JsonValue& /*params*/) override
        {
            return norves::bridge::Result<norves::bridge::JsonValue, norves::bridge::BridgeError>::
                ok(parse_or_die(
                    R"({"imageBase64":")"
                    R"(iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR42mNwaDgARAwQCgAoDgYBqzvMVQAAAABJRU5ErkJggg==)"
                    R"(","mimeType":"image/png","width":2,"height":2})"));
        }

        // @brief logSubscribe() によってセットされ、recv ループが消費する。
        // @note handleFrame とループは同一スレッドで実行されるため、シングルスレッドのハンドオフ。
        // クロスメソッドの契約を明示するため atomic にする。
        std::atomic<bool> emit_log_burst{false};

        // @brief Phase 6: objectSetProperty() によってセットされ、recv ループが消費する。
        // setProperty の ack 後に object.changed / scene.treeChanged を 1 回ずつ発行する。
        // @note emit_log_burst と同じシングルスレッドのハンドオフ契約。
        std::atomic<bool> emit_object_changed{false};
        std::atomic<bool> emit_scene_tree_changed{false};

    private:
        // @brief objectId -> fieldOfView の現在値（JSON 数値テキスト）。objectSetProperty が
        // 更新し objectGetSnapshot が読む。
        // @note mock のシングルスレッド recv ループ前提でのみ安全（上記 objectSetProperty の
        // @note 参照）。マルチスレッド化しないこと。
        std::map<std::string, std::string> object_field_of_view;

        // @brief Phase 6: 直近の objectSetProperty が対象とした objectId。object.changed の
        // params 構築時に同スレッドで読む。シングルスレッド recv ループ前提。
        std::string last_changed_object_id;

        // @brief n-1 以外の既知ノード（scene.getTree のツリーと整合）に対する小さなデモ
        // スナップショット JSON を返す。conformance には現れない additive 経路であり、n-1 の
        // exact-match を一切壊さない。未知 id では nullopt。
        static std::optional<std::string> demo_snapshot_for(const std::string& id)
        {
            if (id == "n-0")
            {
                return std::string(
                    R"({"objectId":"n-0","name":"Root","kind":"object","properties":[)"
                    R"({"name":"visible","value":true,"valueType":"boolean"}]})");
            }
            if (id == "n-2")
            {
                return std::string(
                    R"({"objectId":"n-2","name":"GroupNode","kind":"object","properties":[)"
                    R"({"name":"label","value":"Group","valueType":"string"},)"
                    R"({"name":"childCount","value":1,"valueType":"number"}]})");
            }
            if (id == "n-3")
            {
                return std::string(
                    R"({"objectId":"n-3","name":"NodeB","kind":"object","properties":[)"
                    R"({"name":"enabled","value":false,"valueType":"boolean"}]})");
            }
            return std::nullopt;
        }

        // @brief コンパクトな JSON オブジェクトテキストから、トップレベルの文字列フィールドの値
        // （引用符なし）を取り出す。フィクスチャ駆動の決定論的入力に対する最小限のスキャナで
        // あり、汎用 JSON パーサではない（examples/ からは opaque な JsonValue しか触れないため、
        // dump() したコンパクト表現を読む）。見つからなければ nullopt。
        static std::optional<std::string> extract_string_field(const std::string& objectText,
                                                               std::string_view key)
        {
            const std::optional<std::string> raw = extract_json_field(objectText, key);
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

        // @brief コンパクトな JSON オブジェクトテキストから、トップレベルのフィールド値を生の
        // JSON テキスト（文字列なら引用符込み、数値/真偽値/null/配列/オブジェクトはそのまま）で
        // 取り出す。dump() の出力はネストでも有効な JSON なので、対応する括弧/引用のバランスを
        // 取りながら値トークンの終端を求める。見つからなければ nullopt。
        static std::optional<std::string> extract_json_field(const std::string& objectText,
                                                            std::string_view key)
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
    };

}  // namespace norves::mock
