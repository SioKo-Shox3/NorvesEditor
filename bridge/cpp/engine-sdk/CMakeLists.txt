add_library(norves_bridge_engine_sdk STATIC
    src/version.cpp
    src/json_value.cpp
    src/envelope.cpp
    src/codec.cpp
    src/server.cpp
    src/bounded_queue.cpp
    src/dto_codec.cpp
    src/loopback_transport.cpp
    src/ws_server_transport.cpp
)

target_include_directories(norves_bridge_engine_sdk
    PUBLIC
        "${CMAKE_CURRENT_SOURCE_DIR}/include"
    PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/src"
)

target_compile_features(norves_bridge_engine_sdk PUBLIC cxx_std_20)

# nlohmann/json is an implementation detail of the codec / json_value TUs only.
# Linking it PRIVATE keeps the type out of every public header: include/ depends
# on std + the SDK's own value types only. The boundary is grep-verified in CI.
target_link_libraries(norves_bridge_engine_sdk PRIVATE nlohmann::json)

# libwebsockets backs ws_server_transport.cpp only. Linked PRIVATE (and its
# include dir PRIVATE) so the lws types never leak through include/: the boundary
# (grep lws_|libwebsockets|websockets under include/ == 0 hits) is grep-verified.
# The static lws target FetchContent exports is named "websockets" (ADR 0007).
target_link_libraries(norves_bridge_engine_sdk PRIVATE websockets)
target_include_directories(norves_bridge_engine_sdk PRIVATE
    "${libwebsockets_SOURCE_DIR}/include"
    "${libwebsockets_BINARY_DIR}"
)

# MSVC: stricter warnings on our own code only (PRIVATE). No /WX yet (staged).
if(MSVC)
    target_compile_options(norves_bridge_engine_sdk PRIVATE /W4)
endif()

# Tests are gated (Workstream L-P0) so a nested parent build can opt out: this
# also prevents the fixtures test's ${CMAKE_SOURCE_DIR}/../spec/fixtures path from
# resolving against the wrong root when bridge/cpp is not the CMake source root.
if(NORVES_BRIDGE_BUILD_TESTS)
    add_subdirectory(tests)
endif()
