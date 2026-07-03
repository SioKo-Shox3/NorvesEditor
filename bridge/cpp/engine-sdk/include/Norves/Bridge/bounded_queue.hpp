#pragma once

#include "Norves/Bridge/log_sink.hpp"
#include "Norves/Bridge/ownership.hpp"

#include <condition_variable>
#include <cstddef>
#include <deque>
#include <mutex>
#include <optional>
#include <string>

/// @file
/// @brief エンジン SDK のための、有界かつスレッドセーフな所有ワイヤーフレームのキュー。
///
/// @note 依存は <std> と SDK 自身の ILogSink / OwnedFrame のみ。サードパーティヘッダは
///       ここに露出しない。ベンダリングされた JSON ライブラリはこのヘッダから決して到達
///       できない。
namespace Norves::Bridge
{

    /// @brief 満杯のキューに push() が到達したときにキューが行うこと。
    ///
    ///   DropOldest（デフォルト）: 先頭（最古）のフレームを退避させ、新しいものを末尾に
    ///                            追加する。push() は成功（true）を報告する。Rust
    ///                            ディスパッチャの有界ブロードキャストの
    ///                            「lag => 最古を捨てる」姿勢を反映しており、遅いコンシューマ
    ///                            はプロデューサを失速させるのではなく、最も古いデータを失う。
    ///   DropNewest:             到着したフレームを破棄し、キューをそのまま保つ。
    ///                            push() は失敗（false）を報告する。
    ///   Reject:                 到着したフレームを破棄し、キューをそのまま保つ。
    ///                            push() は失敗（false）を報告する。現時点では DropNewest と
    ///                            同じ観測可能な効果を持つが、呼び出し側が意図
    ///                            （「決して上書きしない」vs「バックプレッシャー」）を
    ///                            表現できるよう、また将来のバリアント（例: エラー伝播）が
    ///                            呼び出し箇所を変えずに分岐できるよう、別物として保たれている。
    ///
    /// @note すべてのドロップは（シンクが供給されていれば）ILogSink へ Warn で報告される。
    enum class OverflowPolicy
    {
        DropOldest,
        DropNewest,
        Reject
    };

    /// @brief BoundedFrameQueue は OwnedFrame の単一の有界 FIFO であり、1 つ以上の
    ///        プロデューサスレッドと 1 つ以上のコンシューマスレッドで共有される。
    ///
    /// @note スレッドセーフティ: あらゆる観測可能な操作は、全状態を保護する内部の
    ///       std::mutex をロックする。wait_and_pop() は std::condition_variable で
    ///       ブロックする。このキューはプロデューサとコンシューマが *異なる* スレッドで
    ///       走ること（例: SDK の送信イベントプロデューサ対トランスポートライタ、または
    ///       F5 ループバックコンシューマ）を想定して設計されている。すべての public メンバは
    ///       並行に呼び出して安全である。
    ///
    /// @note 容量: 構築時に固定される。有界バッファにとって容量 0 は無意味なので 1 に
    ///       クランプされる（キューは常に少なくとも 1 フレーム分を保持する）。適切なサイズは
    ///       チャネルに依存するため、ここにコンパイル時のデフォルト容量はない。Rust
    ///       リファレンスはコマンドチャネルに 64、イベントブロードキャストに 256 を使う
    ///       （bridge/crates/.../dispatcher.rs:
    ///       COMMAND_CHANNEL_CAPACITY / EVENT_BROADCAST_CAPACITY）。それらが対応する SDK
    ///       チャネルにとって推奨される桁の目安である。
    ///
    /// @note 寿命: コピー不可かつ move 不可（mutex と condition variable を所有する）。
    ///       それが生きる場所で構築し、参照 / ポインタで渡すこと。
    ///
    /// @note 所有権: ownership.hpp を参照。push() は呼び出し側のフレームを move して入れ、
    ///       pop() / wait_and_pop() はフレームを move して出す。オーバーフロー時、
    ///       シャットダウン時、または破棄時に、キューはまだ保持しているフレームを解放する。
    class BoundedFrameQueue
    {
    public:
        /// @brief `capacity` は最小 1 にクランプされる。`policy` は満杯時の挙動を決める。
        ///        `sink` は所有しない（NON-OWNED）診断シンクであり、nullptr でよく
        ///        （その場合キューは沈黙する）、非 null の場合はこのキューより長生きしなければ
        ///        ならない。シンクは push() を呼んだスレッド上で呼び出される。
        /// @param capacity フレーム容量の上限（最小 1 にクランプされる）。
        /// @param policy 満杯時のオーバーフローポリシー。
        /// @param sink 所有しない診断シンク。nullptr でよい。
        explicit BoundedFrameQueue(std::size_t capacity,
                                   OverflowPolicy policy = OverflowPolicy::DropOldest,
                                   ILogSink* sink = nullptr);

        ~BoundedFrameQueue() = default;

        BoundedFrameQueue(const BoundedFrameQueue&) = delete;
        BoundedFrameQueue& operator=(const BoundedFrameQueue&) = delete;
        BoundedFrameQueue(BoundedFrameQueue&&) = delete;
        BoundedFrameQueue& operator=(BoundedFrameQueue&&) = delete;

        /// @brief `frame` をエンキューし、所有権を move して入れる。フレームが現在キューに
        ///        入っているかを返す。すなわち格納された場合（空きを作るために古いフレームを
        ///        退避させた DropOldest のケースを含む）は true、フレームがドロップされた
        ///        場合（満杯キューでの DropNewest / Reject）またはキューがシャットダウン済み
        ///        の場合は false。shutdown() 後は false を返す no-op である。
        bool push(OwnedFrame frame);

        /// @brief 先頭フレームを取り除いて返す。キューが空なら nullopt。決してブロック
        ///        しない。shutdown() 後も呼び出して安全（残りのフレームをドレインする）。
        std::optional<OwnedFrame> pop();

        /// @brief 先頭フレームを取り除いて返す。1 つ利用可能になるかキューが
        ///        シャットダウンされるまでブロックする。キューがシャットダウン済みかつ空の
        ///        場合に限り nullopt を返す。shutdown() 後はまず残りのフレームをドレインし、
        ///        その後 nullopt を返す。スプリアスウェイクアップは待機述語で処理される。
        std::optional<OwnedFrame> wait_and_pop();

        /// @brief キューをクローズする。wait_and_pop() でブロックしているすべてのスレッドを
        ///        起こし、残りのフレームをドレインしたのち nullopt を観測できるようにする。
        ///        以後のすべての push() を false を返す no-op にする。冪等。
        void shutdown();

        [[nodiscard]] std::size_t size() const;
        [[nodiscard]] std::size_t capacity() const;
        [[nodiscard]] bool closed() const;

    private:
        void warn(std::string_view message);

        mutable std::mutex m_Mutex;
        std::condition_variable m_NotEmpty;
        std::deque<OwnedFrame> m_Frames;
        std::size_t m_Capacity;
        OverflowPolicy m_Policy;
        ILogSink* m_Sink;  // 所有しない。null でよい
        bool m_bClosed = false;
    };

}  // namespace Norves::Bridge
