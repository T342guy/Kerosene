# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(math
    LAYER engine
    SOURCES
        angles.cpp
        plane.cpp
        units.cpp
        winding.cpp
    PUBLIC_DEPS
        kerosene::core)
