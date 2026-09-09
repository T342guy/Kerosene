# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# Target helpers, and the layering check.
#
# Source's real design achievement was that Hammer, vbsp, vvis and vrad are
# separate from the game rather than compiled into it. That boundary is easy to
# state and easy to erode -- one #include from the engine into a compiler and it
# is gone, quietly, in a commit nobody reads closely. So it is a build rule
# here: engine libraries may not depend on toolset code, and the configure step
# fails if one does.

define_property(GLOBAL PROPERTY KEROSENE_ENGINE_LIBS
    BRIEF_DOCS "Engine libraries; may not depend on the toolset")
define_property(GLOBAL PROPERTY KEROSENE_TOOL_LIBS
    BRIEF_DOCS "Toolset libraries; free to depend on the engine")

# kerosene_library(<name> LAYER <engine|tool> SOURCES ... [PUBLIC_DEPS ...] [PRIVATE_DEPS ...])
#
# Creates target kerosene_<name> with alias kerosene::<name>. Headers are found
# from src/, so an include reads <math/winding.hpp>: the directory says which
# library owns it and nothing repeats the project's own name.
function(kerosene_library name)
    cmake_parse_arguments(ARG "" "LAYER" "SOURCES;PUBLIC_DEPS;PRIVATE_DEPS" ${ARGN})

    if(NOT ARG_LAYER MATCHES "^(engine|tool)$")
        message(FATAL_ERROR "kerosene_library(${name}): LAYER must be engine or tool")
    endif()

    set(target "kerosene_${name}")
    add_library(${target} STATIC ${ARG_SOURCES})
    add_library(kerosene::${name} ALIAS ${target})

    target_include_directories(${target} PUBLIC
        "${PROJECT_SOURCE_DIR}/src")
    if(ARG_LAYER STREQUAL "tool")
        target_include_directories(${target} PUBLIC "${PROJECT_SOURCE_DIR}/tools")
    endif()

    target_link_libraries(${target}
        PUBLIC ${ARG_PUBLIC_DEPS}
        PRIVATE ${ARG_PRIVATE_DEPS} kerosene::warnings)

    if(ARG_LAYER STREQUAL "engine")
        set_property(GLOBAL APPEND PROPERTY KEROSENE_ENGINE_LIBS ${target})
    else()
        set_property(GLOBAL APPEND PROPERTY KEROSENE_TOOL_LIBS ${target})
    endif()
endfunction()

# kerosene_executable(<name> SOURCES ... [DEPS ...])
function(kerosene_executable name)
    cmake_parse_arguments(ARG "" "" "SOURCES;DEPS" ${ARGN})
    add_executable(${name} ${ARG_SOURCES})
    target_include_directories(${name} PRIVATE
        "${PROJECT_SOURCE_DIR}/src" "${PROJECT_SOURCE_DIR}/tools")
    target_link_libraries(${name} PRIVATE ${ARG_DEPS} kerosene::warnings)
endfunction()

# Walk a target's link closure, collecting Kerosene targets into <out>.
function(_kerosene_link_closure target out)
    set(seen "")
    set(queue "${target}")
    while(queue)
        list(POP_FRONT queue current)
        if(NOT TARGET ${current})
            continue()
        endif()
        get_target_property(aliased ${current} ALIASED_TARGET)
        if(aliased)
            set(current ${aliased})
        endif()
        if(current IN_LIST seen)
            continue()
        endif()
        list(APPEND seen ${current})

        set(deps "")
        foreach(prop LINK_LIBRARIES INTERFACE_LINK_LIBRARIES)
            get_target_property(value ${current} ${prop})
            if(value)
                list(APPEND deps ${value})
            endif()
        endforeach()
        # Generator expressions cannot be resolved at configure time; a
        # conditional dependency on a tool would be a strange thing to write, so
        # skipping them is safe and keeps the check simple.
        foreach(dep ${deps})
            if(NOT dep MATCHES "\\$<")
                list(APPEND queue ${dep})
            endif()
        endforeach()
    endwhile()
    list(REMOVE_ITEM seen ${target})
    set(${out} "${seen}" PARENT_SCOPE)
endfunction()

# Fails configuration if any engine library reaches a toolset library.
function(kerosene_check_layering)
    get_property(engine_libs GLOBAL PROPERTY KEROSENE_ENGINE_LIBS)
    get_property(tool_libs   GLOBAL PROPERTY KEROSENE_TOOL_LIBS)
    if(NOT engine_libs OR NOT tool_libs)
        return()
    endif()

    set(violations "")
    foreach(lib ${engine_libs})
        _kerosene_link_closure(${lib} closure)
        foreach(dep ${closure})
            if(dep IN_LIST tool_libs)
                list(APPEND violations "  ${lib} -> ${dep}")
            endif()
        endforeach()
    endforeach()

    if(violations)
        list(JOIN violations "\n" report)
        message(FATAL_ERROR
            "Engine libraries must not depend on the toolset. Offenders:\n"
            "${report}\n"
            "The toolset is separate from the engine on purpose; move the shared "
            "code into an engine library instead of linking upward.")
    endif()
endfunction()
