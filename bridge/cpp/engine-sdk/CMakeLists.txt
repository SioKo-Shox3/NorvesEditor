add_library(norves_bridge_engine_sdk STATIC
    src/version.cpp
)

target_include_directories(norves_bridge_engine_sdk
    PUBLIC
        "${CMAKE_CURRENT_SOURCE_DIR}/include"
    PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/src"
)

target_compile_features(norves_bridge_engine_sdk PUBLIC cxx_std_20)

# MSVC: stricter warnings on our own code only (PRIVATE). No /WX yet (staged).
if(MSVC)
    target_compile_options(norves_bridge_engine_sdk PRIVATE /W4)
endif()

# NOTE: nlohmann_json is intentionally NOT linked in F1 (no codec yet).
# F2 will add `target_link_libraries(... PRIVATE nlohmann::json)` for src TUs only.

add_subdirectory(tests)
