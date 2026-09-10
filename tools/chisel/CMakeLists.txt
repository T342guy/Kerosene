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
    PUBLIC_DEPS
        kerosene::map)

if(KEROSENE_BUILD_RENDER)
    kerosene_library(chisel
        LAYER tool
        SOURCES
            chisel_panel.cpp
        PUBLIC_DEPS
            kerosene::shell
            kerosene::toolcommon
            kerosene::chiseldoc
        PRIVATE_DEPS
            kerosene::imgui)
endif()
