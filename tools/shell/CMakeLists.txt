# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(shell
    LAYER tool
    SOURCES shell.cpp
    PUBLIC_DEPS kerosene::core
    PRIVATE_DEPS kerosene::imgui SDL3::SDL3)
