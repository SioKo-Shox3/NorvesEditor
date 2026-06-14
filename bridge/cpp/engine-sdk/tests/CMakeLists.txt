# Each test is an independent executable registered with CTest.

add_executable(smoke_test smoke_test.cpp)

target_link_libraries(smoke_test PRIVATE norves_bridge_engine_sdk)

# Make test_support.hpp resolvable as "test_support.hpp".
target_include_directories(smoke_test PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

if(MSVC)
    target_compile_options(smoke_test PRIVATE /W4)
endif()

add_test(NAME smoke_test COMMAND smoke_test)

# Fixture-driven codec conformance test. Uses nlohmann/json for value-equal
# comparison; this is a TEST TU only, so the public-header boundary is unaffected.
add_executable(fixtures_roundtrip_test fixtures_roundtrip_test.cpp)

target_link_libraries(fixtures_roundtrip_test
    PRIVATE
        norves_bridge_engine_sdk
        nlohmann::json
)

target_include_directories(fixtures_roundtrip_test PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

# bridge/cpp is the CMake source root, so fixtures live at ../spec/fixtures.
target_compile_definitions(fixtures_roundtrip_test
    PRIVATE
        "NORVES_FIXTURES_DIR=\"${CMAKE_SOURCE_DIR}/../spec/fixtures\""
)

if(MSVC)
    target_compile_options(fixtures_roundtrip_test PRIVATE /W4)
endif()

add_test(NAME fixtures_roundtrip_test COMMAND fixtures_roundtrip_test)

# Dispatch / version-negotiation test for BridgeEngineServer (F3). Uses the SDK
# public headers only (incl. JsonValue::parse for value-equal comparison), so it
# does not link nlohmann directly and the public-header boundary is unaffected.
add_executable(dispatch_test dispatch_test.cpp)

target_link_libraries(dispatch_test PRIVATE norves_bridge_engine_sdk)

target_include_directories(dispatch_test PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

if(MSVC)
    target_compile_options(dispatch_test PRIVATE /W4)
endif()

add_test(NAME dispatch_test COMMAND dispatch_test)

# BoundedFrameQueue test (F4). Uses std::thread; SDK public headers only, so it
# does not link nlohmann directly and the public-header boundary is unaffected.
add_executable(bounded_queue_test bounded_queue_test.cpp)

target_link_libraries(bounded_queue_test PRIVATE norves_bridge_engine_sdk)

target_include_directories(bounded_queue_test PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

find_package(Threads REQUIRED)
target_link_libraries(bounded_queue_test PRIVATE Threads::Threads)

if(MSVC)
    target_compile_options(bounded_queue_test PRIVATE /W4)
endif()

add_test(NAME bounded_queue_test COMMAND bounded_queue_test)

# F5 end-to-end loopback round-trip test. Drives the typed DTOs and the loopback
# transport against a BridgeEngineServer on a second thread. Uses std::thread and
# the SDK public headers only (incl. JsonValue::parse), so it does not link
# nlohmann directly and the public-header boundary is unaffected.
add_executable(loopback_roundtrip_test loopback_roundtrip_test.cpp)

target_link_libraries(loopback_roundtrip_test PRIVATE norves_bridge_engine_sdk)

target_include_directories(loopback_roundtrip_test PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

target_link_libraries(loopback_roundtrip_test PRIVATE Threads::Threads)

if(MSVC)
    target_compile_options(loopback_roundtrip_test PRIVATE /W4)
endif()

add_test(NAME loopback_roundtrip_test COMMAND loopback_roundtrip_test)
