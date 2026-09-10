# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# GLSL to SPIR-V, at build time, embedded in the binary.
#
# The runtime never needs a shader compiler and never ships one. A shader error
# is a build error, found by whoever changed the shader, rather than a black
# screen on someone else's machine. The cost is that shaders are not hot
# reloadable, which is a fair trade for a renderer this size and is the sort of
# thing to revisit when there are enough shaders to make it hurt.

find_program(KEROSENE_GLSLC NAMES glslc)
find_program(KEROSENE_GLSLANG NAMES glslangValidator glslang)

if(NOT KEROSENE_GLSLC AND NOT KEROSENE_GLSLANG)
    message(FATAL_ERROR
        "No GLSL compiler found. Install shaderc (glslc) or glslang.\n"
        "In the development container: scripts/devshell.sh installs both.")
endif()

# kerosene_add_shaders(<target> SOURCES <file.vert> <file.frag> ...)
#
# Compiles each to SPIR-V and generates a header declaring it as a byte array,
# named after the file: world.vert becomes kero::shaders::world_vert.
function(kerosene_add_shaders target)
    cmake_parse_arguments(ARG "" "" "SOURCES" ${ARGN})

    set(generated_dir "${CMAKE_CURRENT_BINARY_DIR}/shaders")
    file(MAKE_DIRECTORY "${generated_dir}")

    set(headers "")
    foreach(source ${ARG_SOURCES})
        get_filename_component(name "${source}" NAME)
        string(REPLACE "." "_" symbol "${name}")
        set(spirv "${generated_dir}/${name}.spv")
        set(header "${generated_dir}/${symbol}.hpp")

        if(KEROSENE_GLSLC)
            set(compile_command
                "${KEROSENE_GLSLC}" -O --target-env=vulkan1.0
                -o "${spirv}" "${CMAKE_CURRENT_SOURCE_DIR}/${source}")
        else()
            set(compile_command
                "${KEROSENE_GLSLANG}" -V --target-env vulkan1.0
                -o "${spirv}" "${CMAKE_CURRENT_SOURCE_DIR}/${source}")
        endif()

        add_custom_command(
            OUTPUT "${header}"
            COMMAND ${compile_command}
            # The -D values are quoted as whole arguments, not with embedded
            # quotes: VERBATIM escapes what it is given, so "-DX=${y}" arrives
            # intact while -DX="${y}" arrives with the quotes still in the value.
            COMMAND "${CMAKE_COMMAND}"
                "-DSPIRV=${spirv}" "-DHEADER=${header}" "-DSYMBOL=${symbol}"
                -P "${PROJECT_SOURCE_DIR}/cmake/EmbedSpirv.cmake"
            DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/${source}"
                    "${PROJECT_SOURCE_DIR}/cmake/EmbedSpirv.cmake"
            COMMENT "Compiling shader ${name}"
            VERBATIM)
        list(APPEND headers "${header}")
    endforeach()

    add_custom_target(${target}_shaders DEPENDS ${headers})
    add_dependencies(${target} ${target}_shaders)
    target_include_directories(${target} PRIVATE "${generated_dir}")
endfunction()
