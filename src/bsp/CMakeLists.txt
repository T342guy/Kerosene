# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(bsp
    LAYER engine
    SOURCES
        file.cpp
        surface.cpp
    PUBLIC_DEPS kerosene::kv kerosene::math)
