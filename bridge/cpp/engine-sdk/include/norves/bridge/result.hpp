#pragma once

#include <type_traits>
#include <utility>
#include <variant>

/// @file
/// @brief エンジン SDK のための自己完結な Result<T, E>。
/// @note 依存は <std> のみ。サードパーティヘッダはここに含めない。
namespace norves::bridge
{

    /// @brief Result<T, E> は成功値またはエラー値を表す値型。
    ///
    /// 契約:
    ///   * 構築・コピー・move は T と E に委譲される。アクティブな選択肢を持たずに Result を
    ///     構築する経路は存在しないため、Result は決して空にならない。
    ///   * 代入は、ソースに格納された値の *コピー/move* を用いた emplace により、基底の
    ///     std::variant を破棄してから再構築する形で実装する。std::variant の変換/転送代入を
    ///     意図的に避けることで、唯一の代入経路が T / E 自身のコピー/move コンストラクタを
    ///     通るようにする。これにより例外挙動は完全に T と E によって定義される。
    ///   * never-valueless は不変条件である。T と E（行儀の良い値型）を保持し、すでに
    ///     構築済みの左辺値/右辺値のみを emplace することで、std::variant が
    ///     valueless_by_exception 状態に入ることに依存しない。この不変条件は、T と E が
    ///     行儀の良い nothrow-move 型である限りにおいてのみ成立する。throw する move/copy を
    ///     持つ型は emplace を throw させ、MSVC では std::variant を valueless のままにし
    ///     うる。それゆえ value()/error() はそのケースに対して std::get の
    ///     bad_variant_access ガードに依存する。
    ///   * T と E はアクティブな選択肢が型で判定できるよう、異なる型でなければならない。
    ///     同一型 T == E はコンパイル時に拒否される。本当に同じ基底型が必要なら、タグ
    ///     ラッパを使うこと。
    template <typename T, typename E>
    class Result
    {
        static_assert(!std::is_same_v<T, E>,
                      "Result<T, E> requires T and E to be distinct types so the "
                      "active alternative is type-decidable; wrap one in a tag type "
                      "if you need the same underlying type.");

    public:
        using value_type = T;
        using error_type = E;

        /// ファクトリ。Result を構築する唯一の意図された手段である。
        static Result ok(const T& value) { return Result(std::in_place_index<0>, value); }
        static Result ok(T&& value) { return Result(std::in_place_index<0>, std::move(value)); }
        static Result err(const E& error) { return Result(std::in_place_index<1>, error); }
        static Result err(E&& error) { return Result(std::in_place_index<1>, std::move(error)); }

        Result(const Result&) = default;
        Result(Result&&) noexcept(std::is_nothrow_move_constructible_v<T> &&
                                  std::is_nothrow_move_constructible_v<E>) = default;

        /// 代入は std::variant の変換代入ではなく、emplace を介して格納値自身の
        /// コピー/move コンストラクタを通る。
        Result& operator=(const Result& other)
        {
            if (this != &other)
            {
                if (other.is_ok())
                {
                    storage_.template emplace<0>(std::get<0>(other.storage_));
                }
                else
                {
                    storage_.template emplace<1>(std::get<1>(other.storage_));
                }
            }
            return *this;
        }

        Result& operator=(Result&& other) noexcept(std::is_nothrow_move_constructible_v<T> &&
                                                   std::is_nothrow_move_constructible_v<E>)
        {
            if (this != &other)
            {
                if (other.is_ok())
                {
                    storage_.template emplace<0>(std::get<0>(std::move(other.storage_)));
                }
                else
                {
                    storage_.template emplace<1>(std::get<1>(std::move(other.storage_)));
                }
            }
            return *this;
        }

        ~Result() = default;

        [[nodiscard]] bool is_ok() const noexcept { return storage_.index() == 0; }
        [[nodiscard]] bool is_err() const noexcept { return storage_.index() == 1; }

        /// value()/error() は誤った選択肢（および valueless な variant）に対して
        /// std::bad_variant_access を throw するため、呼び出し側は is_ok()/is_err() で
        /// ガードしなければならない。
        [[nodiscard]] T& value() & { return std::get<0>(storage_); }
        [[nodiscard]] const T& value() const& { return std::get<0>(storage_); }
        [[nodiscard]] T&& value() && { return std::get<0>(std::move(storage_)); }

        [[nodiscard]] E& error() & { return std::get<1>(storage_); }
        [[nodiscard]] const E& error() const& { return std::get<1>(storage_); }
        [[nodiscard]] E&& error() && { return std::get<1>(std::move(storage_)); }

    private:
        template <std::size_t I, typename Arg>
        explicit Result(std::in_place_index_t<I> tag, Arg&& arg)
            : storage_(tag, std::forward<Arg>(arg))
        {
        }

        std::variant<T, E> storage_;
    };

}  // namespace norves::bridge
