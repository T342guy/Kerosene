# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(physics
    LAYER engine
    SOURCES movement.cpp
    PUBLIC_DEPS kerosene::bsp kerosene::console kerosene::math)
