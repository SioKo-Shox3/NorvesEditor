# Vendored nlohmann/json single-header, exposed as an INTERFACE library.
# engine-sdk links this PRIVATELY so the type never leaks through public headers.
add_library(nlohmann_json INTERFACE)
add_library(nlohmann::json ALIAS nlohmann_json)

target_include_directories(nlohmann_json
    INTERFACE
        "${CMAKE_CURRENT_SOURCE_DIR}"
)
