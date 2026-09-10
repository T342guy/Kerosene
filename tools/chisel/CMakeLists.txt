# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# Chisel splits in two, and the split is where the tests are.
#
# Everything that is a decision about the map -- the document, the undo stack,
# brush transforms, picking -- is `chiseldoc`, which depends on nothing but the
# map library and builds with no graphics headers at all. It is unit tested and
# runs in CI.
#
# The panel, the viewports and the GPU path are `chisel`, which needs a window.
# The UI is not unit tested; putting the decisions below it is what makes that
# an acceptable answer rather than an excuse.

kerosene_library(chiseldoc
    LAYER tool
    SOURCES
        document.cpp
        tools.cpp
        viewport.cpp
    PUBLIC_DEPS
        kerosene::map)

if(KEROSENE_BUILD_RENDER)
    # Only inside the guard: Shaders.cmake insists on a shader compiler, and the
    # headless preset has neither one nor anything to use it for.
    include(Shaders)

    kerosene_library(chisel
        LAYER tool
        SOURCES
            chisel_panel.cpp
            viewport_render.cpp
        PUBLIC_DEPS
            kerosene::shell
            kerosene::toolcommon
            kerosene::buildpanel
            kerosene::chiseldoc
        PRIVATE_DEPS
            kerosene::asset
            kerosene::bsp
            kerosene::imgui
            SDL3::SDL3)

    kerosene_add_shaders(kerosene_chisel
        SOURCES
            shaders/chisel_line.vert
            shaders/chisel_line.frag
            shaders/chisel_solid.vert
            shaders/chisel_solid.frag)
endif()
