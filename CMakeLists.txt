# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
cmake_minimum_required(VERSION 3.28)

project(Kerosene
    VERSION 0.1.0
    DESCRIPTION "A brush-based 3D game engine"
    LANGUAGES CXX)

# Only the top-level project decides global policy; as a subdirectory Kerosene
# should keep its hands off the parent's settings.
if(PROJECT_IS_TOP_LEVEL)
    set(CMAKE_CXX_STANDARD 23)
    set(CMAKE_CXX_STANDARD_REQUIRED ON)
    set(CMAKE_CXX_EXTENSIONS OFF)
    set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

    # Kerosene uses classic headers, not C++20 modules -- GCC's and CMake's
    # module support is not stable enough to bet a codebase on. Saying so turns
    # off the dependency scan CMake otherwise runs over every translation unit
    # looking for imports that are not there.
    set(CMAKE_CXX_SCAN_FOR_MODULES OFF)

    # Everything lands in one place, so running a tool from the build tree does
    # not mean knowing which subdirectory produced it.
    set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")

    if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
        set(CMAKE_BUILD_TYPE Debug CACHE STRING "" FORCE)
    endif()
endif()

option(KEROSENE_BUILD_TESTS   "Build the test suites"            ${PROJECT_IS_TOP_LEVEL})
option(KEROSENE_BUILD_RENDER  "Build the renderer and the runtime; needs SDL3" ON)
option(KEROSENE_WERROR        "Treat warnings as errors"         ON)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")
include(Warnings)
include(KeroseneTargets)
include(Dependencies)

add_subdirectory(src)
add_subdirectory(tools)

# The runtime. It builds without a renderer -- that is the headless server, and
# what a CI playthrough runs.
add_subdirectory(apps/kerosene)

if(KEROSENE_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# The rule that keeps the toolset out of the engine, checked rather than
# trusted. See cmake/KeroseneTargets.cmake.
kerosene_check_layering()
