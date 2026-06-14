add_library(norves_bridge_engine_sdk STATIC
    src/version.cpp
    src/json_value.cpp
    src/envelope.cpp
    src/codec.cpp
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

# MSVC: stricter warnings on our own code only (PRIVATE). No /WX yet (staged).
if(MSVC)
    target_compile_options(norves_bridge_engine_sdk PRIVATE /W4)
endif()

add_subdirectory(tests)
