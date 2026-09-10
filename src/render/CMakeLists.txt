# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
include(Shaders)

kerosene_library(render
    LAYER engine
    SOURCES renderer.cpp
    PUBLIC_DEPS kerosene::bsp kerosene::math
    PRIVATE_DEPS SDL3::SDL3)

kerosene_add_shaders(kerosene_render
    SOURCES
        shaders/world.vert
        shaders/world.frag)
