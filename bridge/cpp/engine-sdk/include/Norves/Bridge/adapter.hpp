#pragma once

#include "Norves/Bridge/error.hpp"
#include "Norves/Bridge/json_value.hpp"
#include "Norves/Bridge/result.hpp"

#include <optional>
#include <string>
#include <string_view>

/// @file
/// @brief エンジンアダプタインターフェース。デコード済みの Bridge リクエストを
///        BridgeEngineServer がエンジンロジックへディスパッチできるよう、エンジン統合が
///        実装する接続点（seam）。
///
/// @note 依存は <std> と SDK 自身の値型のみ。サードパーティヘッダはここに含めない。
///       すべてのペイロード（リクエストの params、成功時の result、error.data）は opaque な
///       JsonValue として運ばれ、この層はペイロード内容を解釈しない。メソッドごとの型付き
///       DTO は後のフェーズである。型付きアクセスを望むアダプタは JsonValue を自身で
///       パースする。
///
/// @note スレッドアフィニティ（必読 / REQUIRED）:
///   * SDK は各アダプタメソッドを、BridgeEngineServer::handleFrame を呼んだのと同じ
///     スレッド上で同期的に呼び出す。SDK はアダプタを呼ぶためにスレッドを生成することは
///     決してなく、自身と並行してアダプタを呼ぶことも決してない。
///   * アダプタ実装がエンジンメインスレッド上でのみ触れてよい状態を保持する場合、その
///     エンジンメインスレッド上で handleFrame を駆動する責任は組み込み側（EMBEDDER）に
///     ある。SDK はマーシャリングを提供しない。
///   * アダプタはエンジンのライブメモリを JsonValue を通して返してはならない（MUST NOT）。
///     まずエンジン状態をスナップショット/DTO 値へ変換する
///     （docs/memory-buffer-policy.md）。返された JsonValue は呼び出し後、呼び出し側が
///     所有する。
///   * `params` は呼び出しの間のみ借用される。アダプタはその参照を呼び出し終了後に
///     保持してはならない。
namespace Norves::Bridge
{

    /// @brief 純粋仮想のエンジンアダプタ。対象範囲内のメソッドは純粋仮想である（エンジンは
    ///        それらを実装しなければならない / MUST）。オプションの scene/object/schema
    ///        メソッドは非純粋であり、METHOD_NOT_SUPPORTED を報告するデフォルトを持つ。
    ///        これはプロトコルの開かれたメソッドレジストリを反映している。すなわちエンジンは
    ///        対応するものだけをオーバーライドする。
    class IBridgeEngineAdapter
    {
    public:
        virtual ~IBridgeEngineAdapter() = default;

        // --- ハンドシェイク ------------------------------------------------------

        /// @brief bridge.hello。`selectedProtocolVersion` は、クライアントが提示した
        ///        protocolVersions（クライアントの選好順）と、この SDK の
        ///        SupportedProtocolVersions との積を取ってサーバが既に選んだバージョンである。
        ///        アダプタは result ペイロード（sessionId / protocolVersion / server /
        ///        オプションの capabilities）を JsonValue として構築し、
        ///        `selectedProtocolVersion` を result の protocolVersion フィールドに
        ///        収める責任を負う。
        virtual Result<JsonValue, BridgeError> hello(const JsonValue& params,
                                                     std::string_view selectedProtocolVersion) = 0;

        /// @brief bridge.getCapabilities。
        virtual Result<JsonValue, BridgeError> getCapabilities(const JsonValue& params) = 0;

        // --- エンジン状態 / ローンチ ---------------------------------------------

        /// @brief engine.getStatus。
        virtual Result<JsonValue, BridgeError> getStatus(const JsonValue& params) = 0;

        /// @brief engine.launchInfo。
        virtual Result<JsonValue, BridgeError> launchInfo(const JsonValue& params) = 0;

        // --- ランタイム制御 ------------------------------------------------------

        /// @brief runtime.play。
        virtual Result<JsonValue, BridgeError> runtimePlay(const JsonValue& params) = 0;

        /// @brief runtime.pause。
        virtual Result<JsonValue, BridgeError> runtimePause(const JsonValue& params) = 0;

        /// @brief runtime.stop。
        virtual Result<JsonValue, BridgeError> runtimeStop(const JsonValue& params) = 0;

        /// @brief runtime.focusViewport。
        virtual Result<JsonValue, BridgeError> runtimeFocusViewport(const JsonValue& params) = 0;

        // --- ログストリーミング --------------------------------------------------

        /// @brief log.subscribe。
        virtual Result<JsonValue, BridgeError> logSubscribe(const JsonValue& params) = 0;

        /// @brief log.unsubscribe。
        virtual Result<JsonValue, BridgeError> logUnsubscribe(const JsonValue& params) = 0;

        // --- オプション（開かれたレジストリ） ------------------------------------
        //
        // これらは METHOD_NOT_SUPPORTED を報告するデフォルト実装を持つ。
        // それらに対応するエンジンは該当メソッドをオーバーライドする。

        /// @brief scene.getTree。
        virtual Result<JsonValue, BridgeError> sceneGetTree(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief scene.createObject。
        virtual Result<JsonValue, BridgeError> sceneCreateObject(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief scene.deleteObject。
        virtual Result<JsonValue, BridgeError> sceneDeleteObject(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief scene.reparentObject。
        virtual Result<JsonValue, BridgeError> sceneReparentObject(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief scene.duplicateObject。
        virtual Result<JsonValue, BridgeError> sceneDuplicateObject(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief object.getSnapshot。
        virtual Result<JsonValue, BridgeError> objectGetSnapshot(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief object.setProperty。
        virtual Result<JsonValue, BridgeError> objectSetProperty(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief schema.getSnapshot。
        virtual Result<JsonValue, BridgeError> schemaGetSnapshot(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief asset.resolve。単一の論理アセットパスについて、解決結果 / 健全性メタデータの
        ///        スナップショットを返す。result は
        ///        { status, source, normalizedLogicalPath, requiresExplicitLog?,
        ///        fallbackAction?, failureKind?, reason? }。アダプタはアセットのライブ
        ///        メモリ、ファイルバッファ、パッケージ内バイト列への参照を JsonValue に
        ///        含めてはならず、値コピーされた DTO のみを返す。
        virtual Result<JsonValue, BridgeError> assetResolve(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief asset.getManifest。エンジンが現在ロードしている asset manifest の
        ///        スナップショットを返す。result は
        ///        { version, entries, totalCount, page?, pageSize? }。entries は
        ///        assetEntry DTO の配列であり、エンジンの manifest 内部ストレージへの参照を
        ///        運んではならない。
        virtual Result<JsonValue, BridgeError> assetGetManifest(const JsonValue& params)
        {
            return not_supported(params);
        }

        /// @brief viewport.getThumbnail。エンジンの外部ビューポートの静止画サムネイルを
        ///        低頻度の pull 型レスポンスとして返す。result は
        ///        { imageBase64, mimeType, width?, height? }。imageBase64 はフレーム
        ///        バッファのスナップショットを base64 化した値であり、エンジンのライブ
        ///        メモリへの参照（ポインタ / span / framebuffer view）を渡してはならない
        ///        （docs/memory-buffer-policy.md の large-payload 戦略: PNG / 最大
        ///        640x360 / 256 KiB ハードキャップ / 最大 1 fps の pull 型）。
        virtual Result<JsonValue, BridgeError> viewportGetThumbnail(const JsonValue& params)
        {
            return not_supported(params);
        }

    protected:
        IBridgeEngineAdapter() = default;
        IBridgeEngineAdapter(const IBridgeEngineAdapter&) = default;
        IBridgeEngineAdapter(IBridgeEngineAdapter&&) = default;
        IBridgeEngineAdapter& operator=(const IBridgeEngineAdapter&) = default;
        IBridgeEngineAdapter& operator=(IBridgeEngineAdapter&&) = default;

        /// オプションメソッドの共有デフォルト実装本体。未実装のオプションメソッドは
        /// METHOD_NOT_SUPPORTED を報告する。
        static Result<JsonValue, BridgeError> not_supported(const JsonValue& /*params*/)
        {
            return Result<JsonValue, BridgeError>::err(
                BridgeError{std::string(ErrorMethodNotSupported),
                            "Method is not supported by this engine adapter.", std::nullopt});
        }
    };

}  // namespace Norves::Bridge
