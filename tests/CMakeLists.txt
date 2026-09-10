# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# One executable per library, so a failure names the library before it names the
# case, and so a slow suite does not have to be run to check a fast one.

include(doctest)

function(kerosene_test name)
    cmake_parse_arguments(ARG "" "" "SOURCES;DEPS" ${ARGN})
    add_executable(test_${name} ${ARG_SOURCES})
    target_include_directories(test_${name} PRIVATE
        "${PROJECT_SOURCE_DIR}/src" "${PROJECT_SOURCE_DIR}/tools")
    target_link_libraries(test_${name} PRIVATE
        ${ARG_DEPS} doctest::doctest kerosene::warnings)
    # Tests that read committed content find it here rather than by guessing
    # where they were run from.
    target_compile_definitions(test_${name} PRIVATE
        KEROSENE_SOURCE_DIR="${PROJECT_SOURCE_DIR}")
    doctest_discover_tests(test_${name})
endfunction()

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
