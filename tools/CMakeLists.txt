# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# The toolset. Separate from the engine on purpose -- see the layering check in
# cmake/KeroseneTargets.cmake.
#
# The compile stages are libraries, so they build and are tested with no
# graphics headers at all; the tests link them directly and compile the sample
# content in-process. Only the window needs a GPU, and only the window is
# conditional.

add_subdirectory(common)
add_subdirectory(cleave)
add_subdirectory(umbra)

# The shell and the build panel come before Chisel: Chisel's windowed half links
# both -- it runs the compile stages itself, so F9 and the Build panel cannot
# come to disagree about what a compile is.
if(KEROSENE_BUILD_RENDER)
    add_subdirectory(shell)
    add_subdirectory(build)
endif()

add_subdirectory(chisel)

if(KEROSENE_BUILD_RENDER)

    # One application holding every tool. The tools stay separate -- each is a
    # Panel that knows nothing about the others -- so this is a place to put
    # them rather than something they are written against.
    kerosene_executable(kerosene-tools
        SOURCES kerosene-tools.cpp
        DEPS
            kerosene::shell
            kerosene::chisel
            kerosene::buildpanel)
endif()
