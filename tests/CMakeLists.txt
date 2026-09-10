# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# One executable per library, so a failure names the library before it names the
# case, and so a slow suite does not have to be run to check a fast one.

include(doctest)

function(kerosene_test name)
    cmake_parse_arguments(ARG "" "" "SOURCES;DEPS;PROPERTIES" ${ARGN})
    add_executable(test_${name} ${ARG_SOURCES})
    target_include_directories(test_${name} PRIVATE
        "${PROJECT_SOURCE_DIR}/src" "${PROJECT_SOURCE_DIR}/tools")
    target_link_libraries(test_${name} PRIVATE
        ${ARG_DEPS} doctest::doctest kerosene::warnings)
    # Tests that read committed content find it here rather than by guessing
    # where they were run from.
    target_compile_definitions(test_${name} PRIVATE
        KEROSENE_SOURCE_DIR="${PROJECT_SOURCE_DIR}")
    if(ARG_PROPERTIES)
        doctest_discover_tests(test_${name} PROPERTIES ${ARG_PROPERTIES})
    else()
        doctest_discover_tests(test_${name})
    endif()
endfunction()

# The level tests read a compiled .kbsp, which is a build product rather than
# something committed. This runs the toolset over the sample content first, as a
# CTest fixture, so `ctest` works from a fresh clone instead of failing with a
# missing file and leaving the reader to guess which step was skipped.
add_test(NAME content_build
    COMMAND kerosene-tools cleave "${PROJECT_SOURCE_DIR}/content/maps/kero_start.kmap")
set_tests_properties(content_build PROPERTIES FIXTURES_SETUP kerosene_content)

add_test(NAME content_vis
    COMMAND kerosene-tools umbra "${PROJECT_SOURCE_DIR}/content/maps/kero_start.kbsp")
set_tests_properties(content_vis PROPERTIES
    FIXTURES_SETUP kerosene_content
    DEPENDS content_build)

kerosene_test(core
    SOURCES core_test.cpp
    DEPS kerosene::core)

kerosene_test(math
    SOURCES math_test.cpp
    DEPS kerosene::math)

kerosene_test(kv
    SOURCES kv_test.cpp
    DEPS kerosene::kv)

kerosene_test(map
    SOURCES map_test.cpp
    DEPS kerosene::map)

kerosene_test(cleave
    SOURCES cleave_test.cpp
    DEPS kerosene::cleave)

kerosene_test(umbra
    SOURCES umbra_test.cpp
    DEPS kerosene::umbra)

kerosene_test(level
    SOURCES level_test.cpp
    DEPS kerosene::bsp kerosene::kv
    PROPERTIES FIXTURES_REQUIRED kerosene_content)
