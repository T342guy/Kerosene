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
    doctest_discover_tests(test_${name})
endfunction()

kerosene_test(core
    SOURCES core_test.cpp
    DEPS kerosene::core)

kerosene_test(math
    SOURCES math_test.cpp
    DEPS kerosene::math)
