#pragma once

#include "norves/bridge/codec_error.hpp"
#include "norves/bridge/dto/common.hpp"
#include "norves/bridge/json_value.hpp"
#include "norves/bridge/result.hpp"

#include <optional>
#include <string>
#include <vector>

/// @file
/// @brief F5 ラウンドトリップのための型付きメソッドペイロード DTO（4 つのワイヤー経路の
///        み）: bridge.hello（params + result）、engine.getStatus（result）、runtime.play
///        （result）。これらは Rust editor-client の型付きパースヘルパ
///        （parse_hello_result / parse_status_result, HelloParams）の C++ 対応物。
///
/// @note 依存は <std> と SDK 自身の値/enum 型のみ。サードパーティヘッダはここに露出しない。
///       各 DTO は素の struct である。JSON の構築 / 検証は src/dto_codec.cpp にあり、それが
///       ベンダリングされた JSON ライブラリに触れる唯一の TU である。
///
/// @note 検証契約（F2 エンベロープコーデックの厳格さとスキーマの
///       `additionalProperties: false` に一致する）:
///   * from_json は未知のキーを、すべてのオブジェクト層で再帰的に拒否する
///     （HelloResult のネストした `server` オブジェクトを含む）、
///   * from_json は必須フィールドの欠落、誤った JSON 型、列挙範囲外の文字列を拒否する、
///   * to_json は存在しないオプションフィールドを省略する（未設定の std::optional に対して
///     JSON `null` を決して書かない）ため、任意の整形式 x について
///     from_json(to_json(x)) == x が成り立つ。
///
/// @note スコープ: これら 4 つのペイロードのみ。capabilities（hello）および開かれた値 $def
///       はここでは意図的にモデル化されず、opaque のままである。
namespace norves::bridge::dto
{

    /// @brief bridge.hello のリクエスト params。スキーマの `capabilities` フィールドは
    ///        ラウンドトリップで使われないため、この DTO からは省略される。
    struct HelloParams
    {
        std::string role;
        std::string clientName;
        std::optional<std::string> clientVersion;
        std::vector<std::string> protocolVersions;

        [[nodiscard]] bool operator==(const HelloParams& other) const
        {
            return role == other.role && clientName == other.clientName &&
                   clientVersion == other.clientVersion &&
                   protocolVersions == other.protocolVersions;
        }
        [[nodiscard]] bool operator!=(const HelloParams& other) const { return !(*this == other); }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<HelloParams, CodecError> from_json(const JsonValue& value);
    };

    /// @brief HelloResult 内にネストされたエンジン識別情報
    ///        （bridge.hello.result#/server）。スキーマの `engine` フィールドは自由形式の
    ///        汎用ラベルであり、エンジン固有の型名ではない。
    struct ServerInfo
    {
        std::string name;
        std::optional<std::string> version;
        std::optional<std::string> engine;

        [[nodiscard]] bool operator==(const ServerInfo& other) const
        {
            return name == other.name && version == other.version && engine == other.engine;
        }
        [[nodiscard]] bool operator!=(const ServerInfo& other) const { return !(*this == other); }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<ServerInfo, CodecError> from_json(const JsonValue& value);
    };

    /// @brief bridge.hello のレスポンス result。スキーマの `capabilities` 配列は
    ///        ラウンドトリップで使われないため省略される。
    struct HelloResult
    {
        std::string sessionId;
        std::string protocolVersion;
        ServerInfo server;

        [[nodiscard]] bool operator==(const HelloResult& other) const
        {
            return sessionId == other.sessionId && protocolVersion == other.protocolVersion &&
                   server == other.server;
        }
        [[nodiscard]] bool operator!=(const HelloResult& other) const { return !(*this == other); }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<HelloResult, CodecError> from_json(const JsonValue& value);
    };

    /// @brief engine.getStatus のレスポンス result。
    struct StatusSnapshot
    {
        EngineState engineState = EngineState::Initializing;
        RuntimeState runtimeState = RuntimeState::Unknown;
        std::optional<std::string> engineName;
        std::optional<std::string> engineVersion;
        std::optional<std::string> title;

        [[nodiscard]] bool operator==(const StatusSnapshot& other) const
        {
            return engineState == other.engineState && runtimeState == other.runtimeState &&
                   engineName == other.engineName && engineVersion == other.engineVersion &&
                   title == other.title;
        }
        [[nodiscard]] bool operator!=(const StatusSnapshot& other) const
        {
            return !(*this == other);
        }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<StatusSnapshot, CodecError> from_json(const JsonValue& value);
    };

    /// @brief runtime.play のレスポンス result。リクエスト params は空オブジェクトなので、
    ///        result のみがモデル化される。`requestedState` はオプションの目標ランタイム
    ///        状態。
    struct PlayAck
    {
        bool accepted = false;
        std::optional<RuntimeState> requestedState;

        [[nodiscard]] bool operator==(const PlayAck& other) const
        {
            return accepted == other.accepted && requestedState == other.requestedState;
        }
        [[nodiscard]] bool operator!=(const PlayAck& other) const { return !(*this == other); }

        [[nodiscard]] JsonValue to_json() const;
        [[nodiscard]] static Result<PlayAck, CodecError> from_json(const JsonValue& value);
    };

}  // namespace norves::bridge::dto
