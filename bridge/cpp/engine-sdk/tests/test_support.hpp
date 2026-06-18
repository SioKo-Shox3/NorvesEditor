#pragma once

#include <cstdio>
#include <string_view>

// @brief SDK ユニットテスト用の最小アサーションハーネス（単一ヘッダ）。
//
// 規約（ctest の合否はプロセス終了コードのみで決まる）:
//   * 失敗した検査はそれぞれ stderr に報告され、カウンタをインクリメントする。
//   * テストの main() は `return norves::test::summary();` で終わること。
//     これにより、1 件以上の検査が失敗した場合は非ゼロ、全検査合格の場合はゼロで終了する。
//   * サードパーティへの依存はなく、std のみを使用する。
namespace norves::test
{

    inline int& failure_count()
    {
        static int count = 0;
        return count;
    }

    inline void report_failure(std::string_view expr, const char* file, int line)
    {
        std::fprintf(stderr, "FAIL %s:%d: %.*s\n", file, line, static_cast<int>(expr.size()),
                     expr.data());
        ++failure_count();
    }

    // @brief プロセス終了コードを返す。全検査合格なら 0、1 件以上失敗なら 1。
    inline int summary()
    {
        if (failure_count() == 0)
        {
            std::fprintf(stderr, "OK: all checks passed\n");
            return 0;
        }
        std::fprintf(stderr, "FAILED: %d check(s) failed\n", failure_count());
        return 1;
    }

}  // namespace norves::test

#define NORVES_CHECK(cond)                                             \
    do                                                                 \
    {                                                                  \
        if (!(cond))                                                   \
        {                                                              \
            ::norves::test::report_failure(#cond, __FILE__, __LINE__); \
        }                                                              \
    } while (0)

#define NORVES_CHECK_EQ(a, b)                                                 \
    do                                                                        \
    {                                                                         \
        if (!((a) == (b)))                                                    \
        {                                                                     \
            ::norves::test::report_failure(#a " == " #b, __FILE__, __LINE__); \
        }                                                                     \
    } while (0)
