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
add_subdirectory(physics)
