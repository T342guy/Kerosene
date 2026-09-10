# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_executable(kerosene
    SOURCES main.cpp
    DEPS kerosene::engine kerosene::console kerosene::kv)
