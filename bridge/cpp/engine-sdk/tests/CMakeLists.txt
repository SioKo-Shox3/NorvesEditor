# Each test is an independent executable registered with CTest.

add_executable(smoke_test smoke_test.cpp)

target_link_libraries(smoke_test PRIVATE norves_bridge_engine_sdk)

# Make test_support.hpp resolvable as "test_support.hpp".
target_include_directories(smoke_test PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

if(MSVC)
    target_compile_options(smoke_test PRIVATE /W4)
endif()

add_test(NAME smoke_test COMMAND smoke_test)
