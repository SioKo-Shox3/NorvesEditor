cmake_minimum_required(VERSION 3.21)

project(NorvesBridgeCpp LANGUAGES CXX)

# Embeddability gates (Workstream L-P0). Default ON, so standalone configure is
# byte-for-byte equivalent: examples + tests are still built. A parent project
# that does add_subdirectory(bridge/cpp) (e.g. NorvesLib) sets these OFF to keep
# the example mock-engine and the engine-sdk CTest tests out of its build. This
# matters because EXCLUDE_FROM_ALL does NOT remove add_test() registrations, and
# the fixtures test resolves ${CMAKE_SOURCE_DIR}/../spec/fixtures, which is only
# valid when bridge/cpp is the CMake source root.
option(NORVES_BRIDGE_BUILD_EXAMPLES "Build bridge C++ examples (mock-engine)" ON)
option(NORVES_BRIDGE_BUILD_TESTS "Build bridge C++ engine-sdk tests" ON)

# C++23 across the whole tree; no compiler extensions (ADR 0008).
set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Activate CTest infrastructure whenever a subdir that registers tests is built:
# engine-sdk/tests (NORVES_BRIDGE_BUILD_TESTS) and the examples loopback smoke
# (NORVES_BRIDGE_BUILD_EXAMPLES) both call add_test(), and add_test() is a no-op
# without enable_testing() in scope. Gating it this way keeps standalone behavior
# identical (both ON => enable_testing() still called) while a fully nested build
# (both OFF) registers nothing.
if(NORVES_BRIDGE_BUILD_TESTS OR NORVES_BRIDGE_BUILD_EXAMPLES)
    enable_testing()
endif()

# libwebsockets forces /W3 /WX on MSVC. On a Japanese (CP932) Windows host its
# UTF-8 source files trip warning C4819, which /WX turns into a hard error. Tell
# MSVC the sources are UTF-8 so the warning never fires (ADR 0007 / G1 spike).
# DISABLE_WERROR does not help: that path is on the non-MSVC branch. This is a
# host-locale build-config concern; /utf-8 is harmless for our own /W4 sources.
if(MSVC)
    add_compile_options(/utf-8)
endif()

# --- libwebsockets via FetchContent (ADR 0007; pinned tag, no vcpkg) ---------
# Options must be set BEFORE MakeAvailable. Everything we do not need is OFF so
# the MSVC static build stays minimal. Artifacts live under build/_deps and are
# NOT committed; the first configure clones the pinned tag and needs network.
include(FetchContent)

set(LWS_WITH_SSL                    OFF CACHE BOOL "" FORCE)
set(LWS_WITHOUT_TESTAPPS            ON  CACHE BOOL "" FORCE)
set(LWS_WITHOUT_TEST_SERVER         ON  CACHE BOOL "" FORCE)
set(LWS_WITHOUT_TEST_SERVER_EXTPOLL ON  CACHE BOOL "" FORCE)
set(LWS_WITHOUT_TEST_PING           ON  CACHE BOOL "" FORCE)
set(LWS_WITHOUT_TEST_CLIENT         ON  CACHE BOOL "" FORCE)
set(LWS_WITH_MINIMAL_EXAMPLES       OFF CACHE BOOL "" FORCE)
set(LWS_WITH_STATIC                 ON  CACHE BOOL "" FORCE)
set(LWS_WITH_SHARED                 OFF CACHE BOOL "" FORCE)
set(LWS_WITHOUT_EXTENSIONS          ON  CACHE BOOL "" FORCE)
set(LWS_WITH_LIBUV                  OFF CACHE BOOL "" FORCE)
set(LWS_WITH_LIBEV                  OFF CACHE BOOL "" FORCE)
set(LWS_WITH_LIBEVENT              OFF CACHE BOOL "" FORCE)
set(LWS_WITH_GLIB                  OFF CACHE BOOL "" FORCE)
set(LWS_WITH_EXTERNAL_POLL        OFF CACHE BOOL "" FORCE)
set(LWS_WITH_ZLIB                  OFF CACHE BOOL "" FORCE)
set(LWS_WITH_MBEDTLS              OFF CACHE BOOL "" FORCE)

FetchContent_Declare(
    libwebsockets
    GIT_REPOSITORY https://github.com/warmcat/libwebsockets.git
    GIT_TAG        v4.3.3
    GIT_SHALLOW    TRUE
)
FetchContent_MakeAvailable(libwebsockets)

# Vendored third-party (nlohmann/json) as INTERFACE targets.
add_subdirectory(third_party)

# Engine SDK static library + its tests.
add_subdirectory(engine-sdk)

# Examples (owned by Workstream H): the residential WebSocket mock engine the
# editor backend can launch/attach/drive, plus a CTest-registered loopback smoke.
# Gated so a nested (add_subdirectory) parent build can opt out of the example.
if(NORVES_BRIDGE_BUILD_EXAMPLES)
    add_subdirectory(examples)
endif()
