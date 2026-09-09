# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(map
    LAYER engine
    SOURCES map.cpp
    PUBLIC_DEPS kerosene::kv kerosene::math)
