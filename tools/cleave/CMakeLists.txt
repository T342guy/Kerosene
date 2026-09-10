# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(cleave
    LAYER tool
    SOURCES
        brush.cpp
        portal.cpp
        cleave.cpp
        writer.cpp
        tree.cpp
    PUBLIC_DEPS
        kerosene::bsp
        kerosene::map)
