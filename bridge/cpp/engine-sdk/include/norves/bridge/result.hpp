#pragma once

#include <type_traits>
#include <utility>
#include <variant>

// Self-contained Result<T, E> for the engine SDK.
// Depends on <std> only; no third-party headers are included here.
namespace norves::bridge
{

    // Result<T, E> is a success-or-error value type.
    //
    // Contract:
    //   * Construction, copy and move are delegated to T and E. There is no path
    //     that constructs a Result without an active alternative, so a Result is
    //     never empty.
    //   * Assignment is implemented as destroy-then-reconstruct of the underlying
    //     std::variant via emplace using a *copy/move of the source's stored
    //     value*. We deliberately avoid std::variant's converting/forwarding
    //     assignment so that the only assignment paths route through T's / E's own
    //     copy/move constructors. This keeps the exception behaviour entirely
    //     defined by T and E.
    //   * never-valueless is an invariant: by holding T and E (well-behaved value
    //     types) and only ever emplacing already-constructed lvalues/rvalues, we do
    //     not rely on std::variant entering the valueless_by_exception state. This
    //     invariant holds only as long as T and E are well-behaved, nothrow-move
    //     types; a type with a throwing move/copy can make emplace throw and, on
    //     MSVC, leave the std::variant valueless. value()/error() therefore rely on
    //     std::get's bad_variant_access guard against that case.
    //   * T and E must be distinct types so the active alternative is decidable by
    //     type. Same-type T == E is rejected at compile time; use a tag wrapper if
    //     you genuinely need that.
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

        // Factories. These are the only intended ways to build a Result.
        static Result ok(const T& value) { return Result(std::in_place_index<0>, value); }
        static Result ok(T&& value) { return Result(std::in_place_index<0>, std::move(value)); }
        static Result err(const E& error) { return Result(std::in_place_index<1>, error); }
        static Result err(E&& error) { return Result(std::in_place_index<1>, std::move(error)); }

        Result(const Result&) = default;
        Result(Result&&) noexcept(std::is_nothrow_move_constructible_v<T> &&
                                  std::is_nothrow_move_constructible_v<E>) = default;

        // Assignment routes through the stored value's own copy/move constructor via
        // emplace, instead of std::variant's converting assignment.
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

        // value()/error() throw std::bad_variant_access on the wrong alternative
        // (and on a valueless variant), so callers must gate on is_ok()/is_err().
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
