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
# something committed. test_content compiles it, in-process, against the
# compiler libraries -- so a fresh clone runs green with no arguments, and the
# whole suite still works with no graphics headers and no GPU.
#
# It is a real test as well as a fixture: it asserts the sample level compiles
# clean and is sealed, which is what makes every suite measured against it
# meaningful.

kerosene_test(core
    SOURCES core_test.cpp
    DEPS kerosene::core)

kerosene_test(math
    SOURCES math_test.cpp
    DEPS kerosene::math)

kerosene_test(content
    SOURCES content_test.cpp
    DEPS kerosene::cleave kerosene::umbra
    PROPERTIES FIXTURES_SETUP kerosene_content)

kerosene_test(chisel
    SOURCES chisel_test.cpp
    DEPS kerosene::chiseldoc)

kerosene_test(asset
    SOURCES asset_test.cpp
    DEPS kerosene::asset)

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

kerosene_test(physics
    SOURCES physics_test.cpp
    DEPS kerosene::physics kerosene::console
    PROPERTIES FIXTURES_REQUIRED kerosene_content)

kerosene_test(engine
    SOURCES engine_test.cpp
    DEPS kerosene::engine kerosene::game kerosene::console
    PROPERTIES FIXTURES_REQUIRED kerosene_content)
