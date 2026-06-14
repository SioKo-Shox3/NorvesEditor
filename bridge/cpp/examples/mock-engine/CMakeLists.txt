# Workstream H-A: the residential WebSocket mock engine and its loopback smoke.
#
# Both targets link the engine SDK only. libwebsockets and nlohmann/json are
# PRIVATE link deps of norves_bridge_engine_sdk, so they are resolved transitively
# (for the static archive) WITHOUT this directory including their headers: the
# mock engine talks to the WebSocket transport solely through the lws-free
# ws_server_transport.hpp seam. That keeps /W4 clean here and keeps the
# public-header boundary intact (grep lws_|libwebsockets|nlohmann under this dir
# == 0 hits, CI-verifiable).

# Threads::Threads is needed by the watcher thread (main.cpp) and the engine
# thread (loopback_smoke.cpp). engine-sdk/tests already calls find_package, but
# that scope does not leak here, so request it explicitly.
find_package(Threads REQUIRED)

# --- Residential mock engine executable --------------------------------------
# NOT registered with add_test: it is a long-running WebSocket server that blocks
# in recv() until closed, so a CTest entry would hang. The editor backend (and
# the manual smoke) launch this binary directly.
add_executable(norves_mock_engine main.cpp)

target_link_libraries(norves_mock_engine PRIVATE norves_bridge_engine_sdk Threads::Threads)

if(MSVC)
    target_compile_options(norves_mock_engine PRIVATE /W4)
endif()

# --- Loopback smoke (CTest) --------------------------------------------------
# Exercises the same MockAdapter end to end over an in-process loopback pair, so
# it terminates deterministically and is safe to register with CTest (unlike the
# residential server above). test_support.hpp lives in engine-sdk/tests; make it
# resolvable as "test_support.hpp".
add_executable(mock_engine_loopback_smoke loopback_smoke.cpp)

target_link_libraries(mock_engine_loopback_smoke PRIVATE norves_bridge_engine_sdk Threads::Threads)

target_include_directories(mock_engine_loopback_smoke PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}"
    "${CMAKE_CURRENT_SOURCE_DIR}/../../engine-sdk/tests"
)

if(MSVC)
    target_compile_options(mock_engine_loopback_smoke PRIVATE /W4)
endif()

add_test(NAME mock_engine_loopback_smoke COMMAND mock_engine_loopback_smoke)
