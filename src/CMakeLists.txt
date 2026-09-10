# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# Order is dependency order, so a library is defined before anything links it.
add_subdirectory(core)
add_subdirectory(math)
add_subdirectory(console)
add_subdirectory(kv)
add_subdirectory(map)
add_subdirectory(bsp)
add_subdirectory(entity)
add_subdirectory(game)
add_subdirectory(engine)

# The renderer is optional. Everything above it -- the compilers, the
# simulation, the tests -- builds and runs without it, which is what the nogfx
# preset exercises and what a CI machine with no GPU uses.
if(KEROSENE_BUILD_RENDER)
    add_subdirectory(render)
endif()
add_subdirectory(physics)
