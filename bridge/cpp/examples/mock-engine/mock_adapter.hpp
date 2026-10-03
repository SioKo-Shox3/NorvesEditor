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

#include "Norves/Bridge/adapter.hpp"
#include "Norves/Bridge/Dto/common.hpp"
#include "Norves/Bridge/Dto/methods.hpp"
#include "Norves/Bridge/error.hpp"
#include "Norves/Bridge/json_value.hpp"
#include "Norves/Bridge/result.hpp"

#include <algorithm>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace norves::mock
{

    // @brief JSON リテラルをパースするか中断する。以下のリテラルはコンパイル時定数であり、
    // パース失敗はランタイム条件ではなくプログラミングエラーを意味する。
    // モックエンジンには壊れたリテラルに対する回復可能なパスはない。
    inline Norves::Bridge::JsonValue parse_or_die(std::string_view text)
    {
        auto parsed = Norves::Bridge::JsonValue::parse(text);
        if (parsed.is_err())
        {
            std::exit(2);
        }
        return std::move(parsed).value();
    }

    // @brief モックエンジンアダプタ。レスポンス値は G4 FakeAdapter と 1 対 1 で一致するため、
    // エディタバックエンドは WebSocket 経由（main.cpp）でモックエンジンを駆動した場合でも
    // ループバックスモークの場合でも同一のワイヤー形状を観察する。
    class MockAdapter : public Norves::Bridge::IBridgeEngineAdapter
    {
    public:
        enum class Profile
        {
            Default,
            McpEdit,
        };

        explicit MockAdapter(Profile profile = Profile::Default) : profile_(profile)
        {
            if (is_mcp_edit_profile())
            {
                initialize_editable_scene();
            }
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError> hello(
            const Norves::Bridge::JsonValue& /*params*/,
            std::string_view selectedProtocolVersion) override
        {
            Norves::Bridge::Dto::HelloResult result;
            result.sessionId = "sess-mock-1";
            result.protocolVersion = std::string(selectedProtocolVersion);
            result.server =
                Norves::Bridge::Dto::ServerInfo{"MockEngine", std::optional<std::string>{"0.1.0"},
                                                std::optional<std::string>{"mock"}};
            return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                          Norves::Bridge::BridgeError>::ok(result.to_json());
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        getCapabilities(const Norves::Bridge::JsonValue& /*params*/) override
        {
            if (is_mcp_edit_profile())
            {
                // 編集用の既存 capability を広告し、ライブ更新を使わないプロフィールにする。
                return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                              Norves::Bridge::BridgeError>::
                    ok(parse_or_die(
                        R"({"capabilities":[)"
                        R"({"name":"runtime.control","version":"0.1","description":"Play/pause/stop control."},)"
                        R"({"name":"log.stream"},)"
                        R"({"name":"viewport.focus"},)"
                        R"({"name":"scene.query"},)"
                        R"({"name":"object.query"},)"
                        R"({"name":"object.edit"},)"
                        R"({"name":"scene.edit"},)"
                        R"({"name":"viewport.thumbnail"},)"
                        R"({"name":"component.edit"},)"
                        R"({"name":"asset.read"}]})"));
            }

            // スペックポジティブフィクスチャ
            // （methods/bridge.getCapabilities/positive/response-valid.json）の
            // result.capabilities と値等価にする。H-D 適合ランナーが結果全体を
            // 厳密比較してこのメソッドの乖離を検出できるようにする。
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(
                    R"({"capabilities":[)"
                    R"({"name":"runtime.control","version":"0.1","description":"Play/pause/stop control."},)"
                    R"({"name":"log.stream"},)"
                    R"({"name":"viewport.focus"},)"
                    R"({"name":"scene.query"},)"
                    R"({"name":"object.query"},)"
                    R"({"name":"object.edit"},)"
                    R"({"name":"scene.liveUpdate"},)"
                    R"({"name":"viewport.thumbnail"},)"
                    R"({"name":"component.edit"}]})"));
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError> getStatus(
            const Norves::Bridge::JsonValue& /*params*/) override
        {
            Norves::Bridge::Dto::StatusSnapshot snap;
            snap.engineState = Norves::Bridge::Dto::EngineState::Ready;
            snap.runtimeState = Norves::Bridge::Dto::RuntimeState::Edit;
            snap.engineName = "MockEngine";
            snap.engineVersion = "0.1.0";
            snap.title = "Mock Game";
            return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                          Norves::Bridge::BridgeError>::ok(snap.to_json());
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError> launchInfo(
            const Norves::Bridge::JsonValue& /*params*/) override
        {
            // engine.launchInfo は必須（純粋仮想）メソッドのため、
            // METHOD_NOT_SUPPORTED ではなく最小限の成功結果を返す。
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(R"({"launched":true})"));
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError> runtimePlay(
            const Norves::Bridge::JsonValue& /*params*/) override
        {
            Norves::Bridge::Dto::PlayAck ack;
            ack.accepted = true;
            ack.requestedState = Norves::Bridge::Dto::RuntimeState::Playing;
            return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                          Norves::Bridge::BridgeError>::ok(ack.to_json());
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError> runtimePause(
            const Norves::Bridge::JsonValue& /*params*/) override
        {
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(R"({"accepted":true})"));
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError> runtimeStop(
            const Norves::Bridge::JsonValue& /*params*/) override
        {
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(R"({"accepted":true})"));
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        runtimeFocusViewport(const Norves::Bridge::JsonValue& /*params*/) override
        {
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(R"({"focused":true})"));
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError> logSubscribe(
            const Norves::Bridge::JsonValue& /*params*/) override
        {
            // この ack が送信された後に log.message バーストを発行するよう recv ループに
            // フラグを立てる。ack-before-event の順序を決定論的に維持する
            // （ws_test_server の FakeAdapter と同じ「フラグセット、ack 後に発行」パターン）。
            emit_log_burst.store(true);
            if (is_mcp_edit_profile())
            {
                const std::string subscriptionId =
                    "mock-sub-" + std::to_string(next_subscription_ordinal++);
                active_subscriptions.insert(subscriptionId);
                std::string ack = R"({"subscriptionId":")";
                ack += subscriptionId;
                ack += R"("})";
                return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                              Norves::Bridge::BridgeError>::
                    ok(parse_or_die(ack));
            }
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(R"({"subscribed":true})"));
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        logUnsubscribe(const Norves::Bridge::JsonValue& params) override
        {
            if (is_mcp_edit_profile())
            {
                const std::string paramsText = params.dump();
                const auto subscriptionId = extract_string_field(paramsText, "subscriptionId");
                const bool removed = subscriptionId.has_value() &&
                                     active_subscriptions.erase(subscriptionId.value()) != 0;
                return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                              Norves::Bridge::BridgeError>::
                    ok(parse_or_die(removed ? R"({"ok":true})" : R"({"ok":false})"));
            }
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
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
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError> sceneGetTree(
            const Norves::Bridge::JsonValue& /*params*/) override
        {
            if (is_mcp_edit_profile())
            {
                return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                              Norves::Bridge::BridgeError>::
                    ok(parse_or_die("{\"root\":" + editable_node_json("n-0") + "}"));
            }
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(
                    R"({"root":{"id":"n-0","name":"Root","kind":"object","children":[)"
                    R"({"id":"n-1","name":"NodeA","kind":"object"},)"
                    R"({"id":"n-2","name":"GroupNode","kind":"object","children":[)"
                    R"({"id":"n-3","name":"NodeB"}]}]}})"));
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        sceneCreateObject(const Norves::Bridge::JsonValue& params) override
        {
            if (!is_mcp_edit_profile())
            {
                return Norves::Bridge::IBridgeEngineAdapter::sceneCreateObject(params);
            }
            const std::string paramsText = params.dump();
            const auto requestedParent = extract_string_field(paramsText, "parentId");
            const auto requestedKind = extract_string_field(paramsText, "kind");
            const std::string parentId = requestedParent.value_or("n-0");
            const std::string kind = requestedKind.value_or("object");
            if (kind.empty() || editable_nodes.find(parentId) == editable_nodes.end())
            {
                return accepted_result(false);
            }

            const std::string objectId = allocate_object_id();
            const std::string name = "Mock Object " + std::to_string(next_object_ordinal - 1);
            add_editable_node(objectId, name, kind, parentId);
            return accepted_result(true, std::optional<std::string>{objectId});
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        sceneDeleteObject(const Norves::Bridge::JsonValue& params) override
        {
            if (!is_mcp_edit_profile())
            {
                return Norves::Bridge::IBridgeEngineAdapter::sceneDeleteObject(params);
            }
            const std::string paramsText = params.dump();
            const auto objectId = extract_string_field(paramsText, "objectId");
            if (!objectId.has_value() || objectId.value() == "n-0" ||
                editable_nodes.find(objectId.value()) == editable_nodes.end())
            {
                return accepted_result(false);
            }

            const auto parentId = editable_nodes.at(objectId.value()).parentId;
            if (parentId.has_value())
            {
                auto& siblings = editable_nodes.at(parentId.value()).children;
                siblings.erase(std::remove(siblings.begin(), siblings.end(), objectId.value()),
                               siblings.end());
            }
            remove_editable_subtree(objectId.value());
            return accepted_result(true);
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        sceneReparentObject(const Norves::Bridge::JsonValue& params) override
        {
            if (!is_mcp_edit_profile())
            {
                return Norves::Bridge::IBridgeEngineAdapter::sceneReparentObject(params);
            }
            const std::string paramsText = params.dump();
            const auto objectId = extract_string_field(paramsText, "objectId");
            const auto requestedParent = extract_string_field(paramsText, "newParentId");
            const std::string parentId = requestedParent.value_or("n-0");
            if (!objectId.has_value() || objectId.value() == "n-0" ||
                editable_nodes.find(objectId.value()) == editable_nodes.end() ||
                editable_nodes.find(parentId) == editable_nodes.end() ||
                would_create_cycle(objectId.value(), parentId))
            {
                return accepted_result(false);
            }

            EditableNode& node = editable_nodes.at(objectId.value());
            if (node.parentId.has_value())
            {
                auto& siblings = editable_nodes.at(node.parentId.value()).children;
                siblings.erase(std::remove(siblings.begin(), siblings.end(), objectId.value()),
                               siblings.end());
            }
            node.parentId = parentId;
            editable_nodes.at(parentId).children.push_back(objectId.value());
            return accepted_result(true);
        }

        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        sceneDuplicateObject(const Norves::Bridge::JsonValue& params) override
        {
            if (!is_mcp_edit_profile())
            {
                return Norves::Bridge::IBridgeEngineAdapter::sceneDuplicateObject(params);
            }
            const std::string paramsText = params.dump();
            const auto objectId = extract_string_field(paramsText, "objectId");
            const auto requestedParent = extract_string_field(paramsText, "newParentId");
            if (!objectId.has_value() || objectId.value() == "n-0" ||
                editable_nodes.find(objectId.value()) == editable_nodes.end())
            {
                return accepted_result(false);
            }
            const std::string parentId = requestedParent.value_or(
                editable_nodes.at(objectId.value()).parentId.value_or("n-0"));
            if (editable_nodes.find(parentId) == editable_nodes.end())
            {
                return accepted_result(false);
            }

            return accepted_result(
                true, std::optional<std::string>{clone_editable_subtree(objectId.value(), parentId)});
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
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        objectGetSnapshot(const Norves::Bridge::JsonValue& params) override
        {
            const std::string paramsText = params.dump();
            const std::optional<std::string> objectId = extract_string_field(paramsText, "objectId");
            const std::string id = objectId.value_or("n-1");

            if (is_mcp_edit_profile())
            {
                return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                              Norves::Bridge::BridgeError>::
                    ok(parse_or_die(editable_snapshot_text(id)));
            }

            // 本文は snapshot_text が一元管理する（object.changed の params も同じ本文を
            // components 抜きで綴るため。分けて持つと、一方だけ直した結果イベントが空の
            // propertyBag を運ぶ）。
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(snapshot_text(id, true)));
        }

        // @brief object.setProperty。{accepted:true, appliedValue:<echo>} を返し、インメモリ
        // 静的マップを更新する。appliedValue は params.value をそのままエコーする。
        // @note 状態更新（object_field_of_view への書き込み）は mock のシングルスレッド recv
        // ループ前提でのみ安全である。handleFrame はアダプタを同期・同スレッドで呼ぶため
        // （adapter.hpp のスレッドアフィニティ規約）、この可変状態にロックは要らない。mock を
        // 将来もマルチスレッド化しないこと。Phase 6 の object.changed emit はこの更新済みマップを
        // 同スレッドで読む土台となる。
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        objectSetProperty(const Norves::Bridge::JsonValue& params) override
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

            if (is_mcp_edit_profile())
            {
                const auto node = objectId.has_value() ? editable_nodes.find(objectId.value()) :
                                                         editable_nodes.end();
                if (node == editable_nodes.end() || !propertyName.has_value() ||
                    propertyName->empty() || !valueText.has_value())
                {
                    return accepted_result(false);
                }
                auto property = node->second.properties.find(propertyName.value());
                if (property == node->second.properties.end())
                {
                    node->second.property_order.push_back(propertyName.value());
                    node->second.properties.emplace(
                        propertyName.value(), EditableProperty{valueText.value(), std::nullopt});
                }
                else
                {
                    property->second.value = valueText.value();
                }

                std::string ack = R"({"accepted":true,"appliedValue":)";
                ack += valueText.value();
                ack += "}";
                return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                              Norves::Bridge::BridgeError>::
                    ok(parse_or_die(ack));
            }

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
            return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                          Norves::Bridge::BridgeError>::ok(parse_or_die(ack));
        }

        // @brief component.add。params.objectId のエンティティへ params.kind のコンポーネントを
        // 足す。生成できるのは schema.getSnapshot が instantiable:true で広告している型だけで、
        // それ以外・未知のエンティティ・欄の欠落はすべて accepted:false で返す（プロトコル
        // エラーにしない）。
        // @note object_components を書き換える。シングルスレッド recv ループ前提。
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        componentAdd(const Norves::Bridge::JsonValue& params) override
        {
            const std::string paramsText = params.dump();
            const std::optional<std::string> objectId = extract_string_field(paramsText, "objectId");
            const std::optional<std::string> kind = extract_string_field(paramsText, "kind");
            if (!objectId.has_value() || !kind.has_value())
            {
                return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                              Norves::Bridge::BridgeError>::
                    ok(parse_or_die(R"({"accepted":false})"));
            }

            // 生成可能な型は camera だけ（schema の instantiable と一致させる）。
            const auto entry = object_components.find(objectId.value());
            if (entry == object_components.end() || kind.value() != "camera")
            {
                return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                              Norves::Bridge::BridgeError>::
                    ok(parse_or_die(R"({"accepted":false})"));
            }

            std::string componentId = "component:";
            componentId += objectId.value();
            componentId += ':';
            componentId += std::to_string(next_component_ordinal++);
            entry->second.emplace_back(componentId, kind.value());

            std::string ack = R"({"accepted":true,"componentId":")";
            ack += componentId;
            ack += R"("})";
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(ack));
        }

        // @brief component.remove。params.objectId のコンポーネントを、それが属する
        // エンティティの一覧から外す。見つからなければ accepted:false。
        // @note object_components を書き換える。シングルスレッド recv ループ前提。
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        componentRemove(const Norves::Bridge::JsonValue& params) override
        {
            const std::string paramsText = params.dump();
            const std::optional<std::string> objectId = extract_string_field(paramsText, "objectId");
            if (objectId.has_value())
            {
                for (auto& [entityId, list] : object_components)
                {
                    for (auto it = list.begin(); it != list.end(); ++it)
                    {
                        if (it->first == objectId.value())
                        {
                            list.erase(it);
                            return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                                          Norves::Bridge::BridgeError>::
                                ok(parse_or_die(R"({"accepted":true})"));
                        }
                    }
                }
            }
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(R"({"accepted":false})"));
        }

        // @brief Phase 6: object.changed イベントの params を構築する。更新済みのインメモリ
        // マップから objectGetSnapshot を同スレッドで読み、{objectId, name, kind, properties} の
        // スナップショットをそのまま params とする（events/object.changed.params.schema.json と
        // 整合）。値コピーのみで JsonValue を構築し、エンジン内部ポインタや span を渡さない
        // （memory-buffer-policy）。シングルスレッド recv ループ前提（objectSetProperty の @note）。
        Norves::Bridge::JsonValue object_changed_params()
        {
            const std::string id =
                last_changed_object_id.empty() ? std::string("n-1") : last_changed_object_id;
            // events/object.changed.params.schema.json は additionalProperties:false で
            // components を持たない。コンポーネント一覧は object.getSnapshot の result でだけ
            // 運ぶ契約なので、イベント params は components 抜きで綴る。
            return parse_or_die(snapshot_text(id, false));
        }

        // @brief Phase 6: scene.treeChanged イベントの params を構築する。変更されたノードの
        // スナップショット DTO（changedNodes）を 1 件返す（events/scene.treeChanged.params.schema.json
        // と整合）。最小トリガとして、setProperty 後に変更ノード 1 件を通知するのみ。値コピーのみ。
        static Norves::Bridge::JsonValue scene_tree_changed_params()
        {
            return parse_or_die(
                R"({"changedNodes":[{"id":"n-1","name":"NodeA","kind":"object"}],)"
                R"("fullRefreshRequired":false})");
        }

        // @brief schema.getSnapshot。型記述子（typeName + properties[{name,valueType}]）を返す。
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        schemaGetSnapshot(const Norves::Bridge::JsonValue& /*params*/) override
        {
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
                ok(parse_or_die(
                    R"({"types":[)"
                    R"({"typeName":"TypeA","kind":"object","properties":[)"
                    R"({"name":"fieldOfView","valueType":"number"},)"
                    R"({"name":"enabled","valueType":"boolean"}]},)"
                    R"({"typeName":"TypeB","kind":"component","instantiable":false},)"
                    R"({"typeName":"camera","kind":"component","instantiable":true,"properties":[)"
                    R"({"name":"fieldOfView","valueType":"number"}]}]})"));
        }

        // @brief MCP 試験プロフィール用の資産解決結果を値コピーで返す。
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        assetResolve(const Norves::Bridge::JsonValue& params) override
        {
            if (!is_mcp_edit_profile())
            {
                return Norves::Bridge::IBridgeEngineAdapter::assetResolve(params);
            }

            const auto logicalPath = extract_string_field(params.dump(), "logicalPath");
            const std::string normalizedPath = logicalPath.value_or("");
            const bool hasPath = logicalPath.has_value() && !logicalPath->empty();
            const bool found = normalizedPath == "textures/hero.png";
            const std::string status = found ? "successCooked" :
                                       hasPath ? "cookedEntryMissing" : "invalidRequest";
            const std::string source = found ? "cooked" : "none";
            const std::string result = R"({"status":)" + json_quote(status) +
                                       R"(,"source":)" + json_quote(source) +
                                       R"(,"normalizedLogicalPath":)" + json_quote(normalizedPath) +
                                       "}";
            return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                          Norves::Bridge::BridgeError>::
                ok(parse_or_die(result));
        }

        // @brief MCP 試験プロフィール用のマニフェストを DTO として返す。
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        assetGetManifest(const Norves::Bridge::JsonValue& params) override
        {
            if (!is_mcp_edit_profile())
            {
                return Norves::Bridge::IBridgeEngineAdapter::assetGetManifest(params);
            }

            return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                          Norves::Bridge::BridgeError>::
                ok(parse_or_die(
                    R"({"version":1,"entries":[)"
                    R"({"logicalPath":"textures/hero.png","kind":"texture","variant":"default","format":"png",)"
                    R"("sourceHash":"source-hash","cookedPackage":"packs/textures.ncp","entryName":"textures/hero.png",)"
                    R"("entryType":"texture","cookedHash":"cooked-hash","cookedVersion":1}],)"
                    R"("totalCount":1,"page":0,"pageSize":50})"));
        }

        // @brief viewport.getThumbnail。小さなテスト用 PNG（2x2、base64 後でも 100 バイト程度
        // で 256 KiB ハードキャップの遥か内）の固定スナップショットを返す。返値は正典フィクスチャ
        // （viewport.getThumbnail/positive/response-valid.json）の result と値等価であり、
        // H-D 適合ランナーが result 全体を厳密比較してこのメソッドの乖離を検出できるようにする。
        // base64 文字列は値コピーで JsonValue を構築し、エンジンのフレームバッファや内部
        // ポインタ・span を一切渡さない（docs/memory-buffer-policy.md の large-payload 戦略:
        // PNG / 最大 640x360 / 256 KiB / 最大 1 fps の pull 型）。
        Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        viewportGetThumbnail(const Norves::Bridge::JsonValue& /*params*/) override
        {
            return Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>::
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
        struct EditableProperty
        {
            std::string value;
            std::optional<std::string> valueType;
        };

        struct EditableNode
        {
            std::string id;
            std::string name;
            std::string kind;
            bool includeKind = true;
            std::optional<std::string> parentId;
            std::vector<std::string> children;
            std::vector<std::string> property_order;
            std::map<std::string, EditableProperty> properties;
        };

        Profile profile_ = Profile::Default;
        std::uint64_t next_object_ordinal = 1;
        std::uint64_t next_subscription_ordinal = 1;
        std::set<std::string> active_subscriptions;
        std::map<std::string, EditableNode> editable_nodes;

        bool is_mcp_edit_profile() const { return profile_ == Profile::McpEdit; }

        static Norves::Bridge::Result<Norves::Bridge::JsonValue, Norves::Bridge::BridgeError>
        accepted_result(bool accepted, const std::optional<std::string>& newId = std::nullopt)
        {
            std::string response = accepted ? R"({"accepted":true)" : R"({"accepted":false)";
            if (newId.has_value())
            {
                response += ",\"newId\":";
                response += json_quote(newId.value());
            }
            response += "}";
            return Norves::Bridge::Result<Norves::Bridge::JsonValue,
                                          Norves::Bridge::BridgeError>::
                ok(parse_or_die(response));
        }

        static std::string json_quote(std::string_view value)
        {
            std::string quoted{"\""};
            for (const unsigned char c : value)
            {
                switch (c)
                {
                    case '"':
                        quoted += R"(\")";
                        break;
                    case '\\':
                        quoted += R"(\\)";
                        break;
                    case '\b':
                        quoted += R"(\b)";
                        break;
                    case '\f':
                        quoted += R"(\f)";
                        break;
                    case '\n':
                        quoted += R"(\n)";
                        break;
                    case '\r':
                        quoted += R"(\r)";
                        break;
                    case '\t':
                        quoted += R"(\t)";
                        break;
                    default:
                        if (c < 0x20)
                        {
                            constexpr char Hex[] = "0123456789abcdef";
                            quoted += R"(\u00)";
                            quoted += Hex[(c >> 4) & 0x0f];
                            quoted += Hex[c & 0x0f];
                        }
                        else
                        {
                            quoted += static_cast<char>(c);
                        }
                        break;
                }
            }
            quoted += '"';
            return quoted;
        }

        void initialize_editable_scene()
        {
            add_editable_node("n-0", "Root", "object", std::nullopt);
            add_editable_node("n-1", "NodeA", "object", "n-0");
            add_editable_node("n-2", "GroupNode", "object", "n-0");
            add_editable_node("n-3", "NodeB", "object", "n-2");
            editable_nodes.at("n-3").includeKind = false;

            add_editable_property("n-0", "visible", "true", "boolean");
            add_editable_property("n-1", "label", R"("Example Name")", "string");
            add_editable_property("n-1", "fieldOfView", "60", "number");
            add_editable_property("n-1", "enabled", "true", "boolean");
            add_editable_property("n-1", "parent", "null");
            add_editable_property("n-1", "position", "[0,1.5,-10]", "vector3");
            add_editable_property("n-1", "metadata", R"({"locked":false,"tag":"primary"})");
            add_editable_property("n-2", "label", R"("Group")", "string");
            add_editable_property("n-2", "childCount", "1", "number");
            add_editable_property("n-3", "enabled", "false", "boolean");
        }

        void add_editable_node(std::string id, std::string name, std::string kind,
                               std::optional<std::string> parentId)
        {
            EditableNode node;
            node.id = id;
            node.name = std::move(name);
            node.kind = std::move(kind);
            node.parentId = std::move(parentId);
            editable_nodes.emplace(id, std::move(node));
            if (editable_nodes.at(id).parentId.has_value())
            {
                editable_nodes.at(editable_nodes.at(id).parentId.value()).children.push_back(id);
            }
        }

        void add_editable_property(const std::string& objectId, std::string name,
                                   std::string value,
                                   std::optional<std::string> valueType = std::nullopt)
        {
            EditableNode& node = editable_nodes.at(objectId);
            node.property_order.push_back(name);
            node.properties.emplace(std::move(name),
                                    EditableProperty{std::move(value), std::move(valueType)});
        }

        std::string allocate_object_id()
        {
            return "mcp-node-" + std::to_string(next_object_ordinal++);
        }

        static std::string editable_node_json(const EditableNode& node,
                                              const std::map<std::string, EditableNode>& nodes)
        {
            std::string out = R"({"id":)" + json_quote(node.id);
            if (node.includeKind)
            {
                out += ",\"kind\":" + json_quote(node.kind);
            }
            out += ",\"name\":" + json_quote(node.name);
            if (!node.children.empty())
            {
                out += ",\"children\":[";
                bool first = true;
                for (const auto& childId : node.children)
                {
                    if (!first)
                    {
                        out += ',';
                    }
                    first = false;
                    out += editable_node_json(nodes.at(childId), nodes);
                }
                out += ']';
            }
            out += '}';
            return out;
        }

        std::string editable_node_json(const std::string& objectId) const
        {
            return editable_node_json(editable_nodes.at(objectId), editable_nodes);
        }

        std::string editable_snapshot_text(const std::string& objectId)
        {
            const auto nodeIt = editable_nodes.find(objectId);
            if (nodeIt == editable_nodes.end())
            {
                return R"({"objectId":)" + json_quote(objectId) + ",\"properties\":[]}";
            }
            const EditableNode& node = nodeIt->second;
            std::string out = R"({"objectId":)" + json_quote(node.id) + ",\"name\":" +
                              json_quote(node.name);
            if (node.includeKind)
            {
                out += ",\"kind\":" + json_quote(node.kind);
            }
            out += ",\"properties\":[";
            bool first = true;
            for (const auto& propertyName : node.property_order)
            {
                const auto property = node.properties.find(propertyName);
                if (property == node.properties.end())
                {
                    continue;
                }
                if (!first)
                {
                    out += ',';
                }
                first = false;
                out += R"({"name":)";
                out += json_quote(propertyName);
                out += ",\"value\":";
                out += property->second.value;
                if (property->second.valueType.has_value())
                {
                    out += ",\"valueType\":";
                    out += json_quote(property->second.valueType.value());
                }
                out += '}';
            }
            out += ']';
            out += components_json(objectId);
            out += '}';
            return out;
        }

        bool would_create_cycle(const std::string& objectId, const std::string& parentId) const
        {
            std::string current = parentId;
            while (true)
            {
                if (current == objectId)
                {
                    return true;
                }
                const auto node = editable_nodes.find(current);
                if (node == editable_nodes.end() || !node->second.parentId.has_value())
                {
                    return false;
                }
                current = node->second.parentId.value();
            }
        }

        void remove_editable_subtree(const std::string& objectId)
        {
            const auto node = editable_nodes.find(objectId);
            if (node == editable_nodes.end())
            {
                return;
            }
            const std::vector<std::string> children = node->second.children;
            for (const auto& child : children)
            {
                remove_editable_subtree(child);
            }
            editable_nodes.erase(objectId);
            object_components.erase(objectId);
            object_field_of_view.erase(objectId);
        }

        std::string clone_editable_subtree(const std::string& sourceId,
                                           const std::string& parentId)
        {
            const EditableNode source = editable_nodes.at(sourceId);
            const std::vector<std::string> sourceChildren = source.children;
            const std::string copyId = allocate_object_id();
            EditableNode copy = source;
            copy.id = copyId;
            copy.name += " Copy";
            copy.parentId = parentId;
            copy.children.clear();
            editable_nodes.emplace(copyId, std::move(copy));
            editable_nodes.at(parentId).children.push_back(copyId);
            for (const auto& childId : sourceChildren)
            {
                clone_editable_subtree(childId, copyId);
            }
            return copyId;
        }

        // @brief objectId -> fieldOfView の現在値（JSON 数値テキスト）。objectSetProperty が
        // 更新し objectGetSnapshot が読む。
        // @note mock のシングルスレッド recv ループ前提でのみ安全（上記 objectSetProperty の
        // @note 参照）。マルチスレッド化しないこと。
        std::map<std::string, std::string> object_field_of_view;

        // @brief entityId -> [(componentId, kind)]。component.add / component.remove が
        // 書き換え、objectGetSnapshot の components がここから綴られる。初期値は
        // scene.getTree のデモツリーと整合する（n-2 が 2 件、n-3 は 0 件）。
        // @note object_field_of_view と同じくシングルスレッド recv ループ前提。
        std::map<std::string, std::vector<std::pair<std::string, std::string>>> object_components{
            {"n-2", {{"component:n-2:1", "camera"}, {"component:n-2:2", "script"}}},
            {"n-3", {}},
        };

        // @brief 次に作るコンポーネントの通し番号（component:<entity>:<n> の n）。
        int next_component_ordinal = 3;

        // @brief Phase 6: 直近の objectSetProperty が対象とした objectId。object.changed の
        // params 構築時に同スレッドで読む。シングルスレッド recv ループ前提。
        std::string last_changed_object_id;

        // @brief n-1 以外の既知ノード（scene.getTree のツリーと整合）、コンポーネント、未知 id に
        // 対するスナップショット JSON を綴る。conformance には現れない additive 経路であり、n-1 の
        // exact-match を一切壊さない。
        //
        // with_components が真のときだけ、コンポーネントを持つノードに components を足す。
        // object.changed の params はこの欄を持てない（上記 object_changed_params の注記）ので、
        // 同じ本文を偽で綴り直す。コンポーネント自身のスナップショットは components を持たない
        // （入れ子のコンポーネントは無い）。
        // @brief entityId のコンポーネント一覧を `,"components":[...]` として綴る。
        // 表に無いエンティティは欄ごと出さない（= このエンジンは投影しない、ではなく
        // 「このノードは投影対象でない」を表す。n-0/n-1 が該当）。
        std::string components_json(const std::string& entityId)
        {
            const auto it = object_components.find(entityId);
            if (it == object_components.end())
            {
                return std::string();
            }
            std::string out(R"(,"components":[)");
            bool first = true;
            for (const auto& [componentId, kind] : it->second)
            {
                if (!first)
                {
                    out += ',';
                }
                first = false;
                out += R"({"objectId":")";
                out += componentId;
                out += R"(","kind":")";
                out += kind;
                out += R"("})";
            }
            out += ']';
            return out;
        }

        // @brief componentId の kind を表から引く。見つからなければ空。
        std::string kind_of_component(const std::string& componentId)
        {
            for (const auto& [entityId, list] : object_components)
            {
                for (const auto& [id, kind] : list)
                {
                    if (id == componentId)
                    {
                        return kind;
                    }
                }
            }
            return std::string();
        }

        std::string snapshot_text(const std::string& id, bool with_components)
        {
            if (id == "n-1")
            {
                // 適合フィクスチャ（object.getSnapshot/positive/response-valid.json）と
                // 値等価の経路。components は持たない（exact-match を壊さない）。
                // fieldOfView は可変。
                std::string fieldOfView = "60";
                const auto it = object_field_of_view.find("n-1");
                if (it != object_field_of_view.end())
                {
                    fieldOfView = it->second;
                }
                std::string out(
                    R"({"objectId":"n-1","name":"NodeA","kind":"object","properties":[)"
                    R"({"name":"label","value":"Example Name","valueType":"string"},)"
                    R"({"name":"fieldOfView","value":)");
                out += fieldOfView;
                out +=
                    R"(,"valueType":"number"},)"
                    R"({"name":"enabled","value":true,"valueType":"boolean"},)"
                    R"({"name":"parent","value":null},)"
                    R"({"name":"position","value":[0,1.5,-10],"valueType":"vector3"},)"
                    R"({"name":"metadata","value":{"locked":false,"tag":"primary"}}]})";
                return out;
            }
            if (id == "n-0")
            {
                return std::string(
                    R"({"objectId":"n-0","name":"Root","kind":"object","properties":[)"
                    R"({"name":"visible","value":true,"valueType":"boolean"}]})");
            }
            if (id == "n-2")
            {
                std::string out(
                    R"({"objectId":"n-2","name":"GroupNode","kind":"object","properties":[)"
                    R"({"name":"label","value":"Group","valueType":"string"},)"
                    R"({"name":"childCount","value":1,"valueType":"number"}])");
                if (with_components)
                {
                    out += components_json("n-2");
                }
                out += "}";
                return out;
            }
            if (id == "n-3")
            {
                // コンポーネントを1つも持たないノード: 空配列で「投影はしたが無い」を表す。
                std::string out(
                    R"({"objectId":"n-3","name":"NodeB","kind":"object","properties":[)"
                    R"({"name":"enabled","value":false,"valueType":"boolean"}])");
                if (with_components)
                {
                    out += components_json("n-3");
                }
                out += "}";
                return out;
            }
            // 組み込みのコンポーネントも、表から外されたら解決してはならない
            // （component.remove の後に id が生き残ると、契約と食い違う）。表に
            // 載っているものだけが以下の分岐へ進む。
            const bool inTable = !kind_of_component(id).empty();
            if (id == "component:n-2:1" && inTable)
            {
                // fieldOfView は n-1 と同じく可変。objectSetProperty は objectId をキーに
                // 同じマップを更新するので、コンポーネント宛ての編集も後続の getSnapshot と
                // object.changed へ反映される。
                std::string fieldOfView = "50";
                const auto it = object_field_of_view.find(id);
                if (it != object_field_of_view.end())
                {
                    fieldOfView = it->second;
                }
                std::string out(
                    R"({"objectId":"component:n-2:1","name":"Camera","kind":"camera","properties":[)"
                    R"({"name":"fieldOfView","value":)");
                out += fieldOfView;
                out +=
                    R"(,"valueType":"number"},)"
                    R"({"name":"nearPlane","value":0.1,"valueType":"number"},)"
                    R"({"name":"isActive","value":true,"valueType":"boolean"}]})";
                return out;
            }
            if (id == "component:n-2:2" && inTable)
            {
                return std::string(
                    R"({"objectId":"component:n-2:2","name":"Script","kind":"script","properties":[)"
                    R"({"name":"scriptPath","value":"Scripts/Demo.as","valueType":"string"},)"
                    R"({"name":"scriptClassName","value":"DemoBehaviour","valueType":"string"}]})");
            }

            // component.add で後から足したコンポーネント: 表から kind を引き、その型の
            // 初期プロパティを綴る（camera だけが生成可能なので実質 camera のみ）。
            const std::string addedKind = kind_of_component(id);
            if (!addedKind.empty())
            {
                std::string fieldOfView = "60";
                const auto it = object_field_of_view.find(id);
                if (it != object_field_of_view.end())
                {
                    fieldOfView = it->second;
                }
                std::string out = R"({"objectId":")";
                out += id;
                out += R"(","kind":")";
                out += addedKind;
                out += R"(","properties":[{"name":"fieldOfView","value":)";
                out += fieldOfView;
                out += R"(,"valueType":"number"}]})";
                return out;
            }

            // 未知 id: 空の propertyBag（必須フィールドのみ）。
            std::string empty = R"({"objectId":")";
            empty += id;
            empty += R"(","properties":[]})";
            return empty;
        }

        // @brief コンパクトな JSON オブジェクトテキストから、トップレベルの文字列フィールドの値
        // （引用符なし）を取り出し、JSON 文字列のエスケープを復元する。汎用 JSON パーサでは
        // ないが、examples/ から opaque な JsonValue しか触れないため dump() の出力を読む。
        // 見つからない場合や文字列以外の場合は nullopt。
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
                return decode_json_string(value);
            }
            return std::nullopt;
        }

        static std::optional<std::uint32_t> read_hex_quad(std::string_view text,
                                                          std::size_t& position)
        {
            if (position + 4 > text.size())
            {
                return std::nullopt;
            }
            std::uint32_t value = 0;
            for (std::size_t i = 0; i < 4; ++i)
            {
                const char digit = text[position + i];
                value <<= 4;
                if (digit >= '0' && digit <= '9')
                {
                    value |= static_cast<std::uint32_t>(digit - '0');
                }
                else if (digit >= 'a' && digit <= 'f')
                {
                    value |= static_cast<std::uint32_t>(digit - 'a' + 10);
                }
                else if (digit >= 'A' && digit <= 'F')
                {
                    value |= static_cast<std::uint32_t>(digit - 'A' + 10);
                }
                else
                {
                    return std::nullopt;
                }
            }
            position += 4;
            return value;
        }

        static void append_utf8(std::string& output, std::uint32_t codePoint)
        {
            if (codePoint <= 0x7f)
            {
                output += static_cast<char>(codePoint);
            }
            else if (codePoint <= 0x7ff)
            {
                output += static_cast<char>(0xc0 | (codePoint >> 6));
                output += static_cast<char>(0x80 | (codePoint & 0x3f));
            }
            else if (codePoint <= 0xffff)
            {
                output += static_cast<char>(0xe0 | (codePoint >> 12));
                output += static_cast<char>(0x80 | ((codePoint >> 6) & 0x3f));
                output += static_cast<char>(0x80 | (codePoint & 0x3f));
            }
            else
            {
                output += static_cast<char>(0xf0 | (codePoint >> 18));
                output += static_cast<char>(0x80 | ((codePoint >> 12) & 0x3f));
                output += static_cast<char>(0x80 | ((codePoint >> 6) & 0x3f));
                output += static_cast<char>(0x80 | (codePoint & 0x3f));
            }
        }

        static std::optional<std::string> decode_json_string(std::string_view token)
        {
            if (token.size() < 2 || token.front() != '"' || token.back() != '"')
            {
                return std::nullopt;
            }
            const std::size_t end = token.size() - 1;
            std::string decoded;
            for (std::size_t position = 1; position < end;)
            {
                const char value = token[position++];
                if (value != '\\')
                {
                    decoded += value;
                    continue;
                }
                if (position >= end)
                {
                    return std::nullopt;
                }
                const char escape = token[position++];
                switch (escape)
                {
                    case '"':
                    case '\\':
                    case '/':
                        decoded += escape;
                        break;
                    case 'b':
                        decoded += '\b';
                        break;
                    case 'f':
                        decoded += '\f';
                        break;
                    case 'n':
                        decoded += '\n';
                        break;
                    case 'r':
                        decoded += '\r';
                        break;
                    case 't':
                        decoded += '\t';
                        break;
                    case 'u':
                    {
                        auto codePoint = read_hex_quad(token, position);
                        if (!codePoint.has_value())
                        {
                            return std::nullopt;
                        }
                        if (codePoint.value() >= 0xd800 && codePoint.value() <= 0xdbff)
                        {
                            if (position + 2 > end || token[position] != '\\' ||
                                token[position + 1] != 'u')
                            {
                                return std::nullopt;
                            }
                            position += 2;
                            const auto lowSurrogate = read_hex_quad(token, position);
                            if (!lowSurrogate.has_value() || lowSurrogate.value() < 0xdc00 ||
                                lowSurrogate.value() > 0xdfff)
                            {
                                return std::nullopt;
                            }
                            codePoint = 0x10000 + ((codePoint.value() - 0xd800) << 10) +
                                        (lowSurrogate.value() - 0xdc00);
                        }
                        else if (codePoint.value() >= 0xdc00 && codePoint.value() <= 0xdfff)
                        {
                            return std::nullopt;
                        }
                        append_utf8(decoded, codePoint.value());
                        break;
                    }
                    default:
                        return std::nullopt;
                }
            }
            return decoded;
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
