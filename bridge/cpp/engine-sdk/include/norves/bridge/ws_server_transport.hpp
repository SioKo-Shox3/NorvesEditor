#pragma once

#include "norves/bridge/log_sink.hpp"
#include "norves/bridge/transport.hpp"

#include <cstddef>
#include <cstdint>
#include <memory>

/// @file
/// @brief エンジン SDK のための WebSocket サーバトランスポート（Workstream G / G3）。
///
/// @note このヘッダは WebSocket サーバエンドポイントを、単一のファクトリを通して素の
///       ITransport として露出する。依存は <std> と SDK 自身の ITransport / ILogSink の
///       みであり、サードパーティの WebSocket 型（基底のコンテキスト / 接続ハンドル）は
///       ここに一切現れない（NO）。サードパーティライブラリは ITransport の pImpl の背後に
///       隠され、対応する .cpp に PRIVATE でリンクされる。境界（include/ 配下の
///       サードパーティトークン == 0 ヒット）は CI によって強制される。ADR 0007 を参照。
///
/// @note スレッドモデル（規範。.cpp で強制される）: トランスポートは WebSocket イベント
///       ループを走らせ、すべての基底ハンドルに排他的に触れる 1 つの（ONE）サービス
///       スレッドを所有する。ITransport::send()（任意の外部スレッド）はフレームを
///       エンキューしてサービススレッドを起こすだけである。ITransport::recv()
///       （コンシューマスレッド）は受信キューでブロックする。close()（任意のスレッド）は
///       サービススレッドへ通知してキューを閉じるだけであり、実際のティアダウンは
///       サービススレッド上で起こる。
namespace norves::bridge
{

    /// @brief 127.0.0.1:`port` で待ち受ける WebSocket サーバトランスポートを生成する
    ///        （ループバックのみ。localhost のみという alpha のスコープに合わせ、0.0.0.0 は
    ///        決して使われない）。TLS はオフ。
    ///
    /// @note 成功時は、サービススレッドが既に走り、単一のエディタクライアント接続を
    ///       受け入れている準備済みの ITransport を返す。バインド / コンテキスト生成の
    ///       失敗時（例: ポートが使用中）には nullptr を返し、`log_sink` が非 null の場合は
    ///       理由を記述する 1 行の Warn を発する。
    ///
    /// @note `send_capacity` / `recv_capacity` は方向ごとのフレームキューを制限する。送信
    ///       キューはバックプレッシャーを使う（満杯のキューは send() を false にする）。
    ///       受信キューはオーバーフローを FATAL として扱う。受信フレームをドロップすると
    ///       リクエスト/レスポンスの相関が壊れるため、満杯の受信キューは接続をクローズし
    ///       （Error がログされる）、回復は上位層（再接続）に委ねる。`recv_capacity` は
    ///       余裕をもって選ぶこと。
    ///
    /// @note 返り値は std::unique_ptr<ITransport> であり、それを通して基底の WebSocket
    ///       ハンドルは一切観測できない。close()（または破棄）はサーバをティアダウンし、
    ///       recv() を nullopt へドレインさせ、send() を false にする。
    /// @param port 待ち受けポート（127.0.0.1 にバインドされる）。
    /// @param send_capacity 送信方向のフレームキュー容量の上限。
    /// @param recv_capacity 受信方向のフレームキュー容量の上限。
    /// @param log_sink オプションのログシンク。nullptr でよい。
    /// @return 準備済みの ITransport。バインド / コンテキスト生成失敗時は nullptr。
    [[nodiscard]] std::unique_ptr<ITransport> make_websocket_server_transport(
        std::uint16_t port, std::size_t send_capacity, std::size_t recv_capacity,
        ILogSink* log_sink = nullptr);

}  // namespace norves::bridge
