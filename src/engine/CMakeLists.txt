# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# The host. Note what is *not* linked here: kerosene::render. The simulation
# runs with or without a display, and the build is what keeps it that way.
kerosene_library(engine
    LAYER engine
    SOURCES host.cpp
    PUBLIC_DEPS
        kerosene::bsp
        kerosene::entity
        kerosene::physics
    PRIVATE_DEPS
        kerosene::console
        kerosene::game)
