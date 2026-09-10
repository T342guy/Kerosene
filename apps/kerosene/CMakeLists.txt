# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_executable(kerosene
    SOURCES main.cpp
    DEPS kerosene::engine kerosene::console kerosene::kv)

# The renderer is linked when it was built. Without it this is the dedicated
# server, and it is the same binary either way.
if(KEROSENE_BUILD_RENDER)
    target_link_libraries(kerosene PRIVATE kerosene::render)
    target_compile_definitions(kerosene PRIVATE KEROSENE_HAS_RENDER=1)
else()
    target_compile_definitions(kerosene PRIVATE KEROSENE_HAS_RENDER=0)
endif()
