# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(buildpanel
    LAYER tool
    SOURCES
        build_panel.cpp
        builder.cpp
    PUBLIC_DEPS
        kerosene::shell
        kerosene::toolcommon
    PRIVATE_DEPS
        kerosene::cleave
        kerosene::umbra
        kerosene::imgui)
