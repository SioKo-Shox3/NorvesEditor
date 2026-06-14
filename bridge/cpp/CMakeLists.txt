cmake_minimum_required(VERSION 3.21)

project(NorvesBridgeCpp LANGUAGES CXX)

# C++20 across the whole tree; no compiler extensions.
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

enable_testing()

# Vendored third-party (nlohmann/json) as INTERFACE targets.
add_subdirectory(third_party)

# Engine SDK static library + its tests.
add_subdirectory(engine-sdk)

# examples/mock-engine is owned by Workstream H; intentionally NOT added here.
