cmake_minimum_required(VERSION 3.21)

project(NorvesBridgeCpp LANGUAGES CXX)

# C++20 across the whole tree; no compiler extensions.
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

enable_testing()

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
add_subdirectory(examples)
