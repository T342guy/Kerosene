# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(chisel
    LAYER tool
    SOURCES
        chisel_panel.cpp
    PUBLIC_DEPS
        kerosene::shell
        kerosene::toolcommon
        kerosene::map
    PRIVATE_DEPS
        kerosene::imgui)
