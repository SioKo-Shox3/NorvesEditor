#ifndef NORVES_BRIDGE_TEST_SUPPORT_HPP
#define NORVES_BRIDGE_TEST_SUPPORT_HPP

#include <cstdio>
#include <string_view>

// Tiny single-file assertion harness for SDK unit tests.
//
// Contract (ctest pass/fail is decided purely by the process exit code):
//   * Each failing check is reported to stderr and increments a counter.
//   * A test's main() must `return norves::test::summary();` so the process
//     exits non-zero iff any check failed, and zero on full success.
//   * No third-party dependency; std only.
namespace norves::test {

inline int& failure_count() {
    static int count = 0;
    return count;
}

inline void report_failure(std::string_view expr, const char* file, int line) {
    std::fprintf(stderr, "FAIL %s:%d: %.*s\n", file, line,
                 static_cast<int>(expr.size()), expr.data());
    ++failure_count();
}

// Returns the process exit code: 0 when every check passed, 1 otherwise.
inline int summary() {
    if (failure_count() == 0) {
        std::fprintf(stderr, "OK: all checks passed\n");
        return 0;
    }
    std::fprintf(stderr, "FAILED: %d check(s) failed\n", failure_count());
    return 1;
}

}  // namespace norves::test

#define NORVES_CHECK(cond)                                          \
    do {                                                            \
        if (!(cond)) {                                              \
            ::norves::test::report_failure(#cond, __FILE__, __LINE__); \
        }                                                           \
    } while (0)

#define NORVES_CHECK_EQ(a, b)                                                  \
    do {                                                                       \
        if (!((a) == (b))) {                                                   \
            ::norves::test::report_failure(#a " == " #b, __FILE__, __LINE__);  \
        }                                                                      \
    } while (0)

#endif  // NORVES_BRIDGE_TEST_SUPPORT_HPP
