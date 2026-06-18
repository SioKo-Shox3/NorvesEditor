#pragma once

#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <utility>

/// @file
/// @brief エンジン SDK のための、フレーム指向でブロッキングなトランスポート接続点（seam）。
///
/// @note 依存は <std> のみ。サードパーティヘッダはここに露出しない。これは Rust
///       editor-client の `Transport` トレイト
///       （bridge/crates/norves-bridge-editor-client/src/transport.rs）の C++ 対応物で
///       あり、フレームごとに厳密に 1 つのワイヤーエンベロープを生の JSON テキストとして
///       運ぶ薄いパイプである。Bridge プロトコルに対しては意図的に無知である。すなわち
///       Envelope を決してデコードせず、スキーマ処理も行わない。すべての
///       エンコード/デコードはこれより上位（codec / server）に存在する。
///
/// @note ブロッキングな形状（Rust の async トレイトに対して）は SDK の同期的な世界に
///       合致する。コンシューマは自身のスレッドで recv() を駆動し、フレームが届くか
///       ピアがクローズするまでブロックする。WebSocket トランスポートは後のフェーズで
///       あり、ここではモデル化されていない。現時点ではプロセス内ループバックペア（下記）
///       のみが存在する。
namespace norves::bridge
{

    /// @brief 双方向・フレーム指向のトランスポート。1 フレーム == 1 ワイヤーエンベロープで
    ///        あり、その JSON テキストとして運ばれる。
    ///
    /// @note スレッドモデル: send() と recv() は異なるスレッドから駆動されてよい（典型的な
    ///       パターンは方向ごとに 1 スレッド）。実装は 2 つの方向を独立させ、ブロックした
    ///       recv() がピアの send() を決してブロックしないようにしなければならない。単一の
    ///       エンドポイントオブジェクトは、複数スレッドからの同一メソッドへの並行呼び出しに
    ///       対して安全であることを要求されない（NOT required）。
    class ITransport
    {
    public:
        virtual ~ITransport() = default;

        /// @brief フレーム（JSON テキストとしての完全なワイヤーエンベロープ）を 1 つ送り、
        ///        所有権を move して渡す。フレームが引き渡された（ピア向けにキューされた）
        ///        時点で true を返す。トランスポートがクローズしている（ピアエンドポイントが
        ///        消えている）場合、または送信バッファが満杯（容量上限に達した）の場合は、
        ///        フレームを送らずに false を返す。false の返却はデータを黙って捨てては
        ///        ならない。クローズ/満杯の状態をバックプレッシャーとして表面化させ、
        ///        呼び出し側が再試行または失敗できるようにする。
        virtual bool send(std::string frame) = 0;

        /// @brief 次のフレームを受信し、1 つ利用可能になるかトランスポートがクローズする
        ///        までブロックする。トランスポートがクローズしており、かつドレインすべき
        ///        フレームがもう残っていない場合に限り nullopt を返す（Rust の `Ok(None)`
        ///        によるクリーンな EOF の対応物）。
        virtual std::optional<std::string> recv() = 0;

        /// @brief このエンドポイントをクローズする。close() 後: このエンドポイントの send()
        ///        は false を返す no-op となり、ピアの recv() は——既に飛行中だったフレームを
        ///        ドレインし終えたのち——nullopt を観測するため、その読み取りループを終了
        ///        できる。close() はピアがまだ受信していないフレームを破棄しない。それは
        ///        ストリーム終端を通知するだけである。冪等。
        virtual void close() = 0;

    protected:
        ITransport() = default;
        ITransport(const ITransport&) = default;
        ITransport(ITransport&&) = default;
        ITransport& operator=(const ITransport&) = default;
        ITransport& operator=(ITransport&&) = default;
    };

    /// @brief send/recv がクロス配線された、接続済みのプロセス内ループバック
    ///        エンドポイントを 2 つ生成する。すなわち一方で送られたフレームは他方の recv()
    ///        に届く。Rust の `loopback_pair` の対応物。`capacity` は方向ごとのバッファ
    ///        フレーム数を制限する（基底のキューにより最小 1 にクランプされる）。2 つの
    ///        エンドポイントは独立したオブジェクトであり、いずれも自身のスレッドから
    ///        駆動されてよい。
    ///
    /// @note クローズの意味論: 一方のエンドポイントで close() を呼ぶ（または破棄する）と、
    ///       ピアの recv() はドレイン後に nullopt を返す。実装が方向ごとの共有キューを
    ///       所有し、返されるエンドポイントが唯一のハンドルである。
    /// @param capacity 方向ごとのバッファフレーム数の上限。
    /// @return 互いに接続された 2 つのトランスポートエンドポイント。
    [[nodiscard]] std::pair<std::unique_ptr<ITransport>, std::unique_ptr<ITransport>>
    make_loopback_pair(std::size_t capacity);

}  // namespace norves::bridge
