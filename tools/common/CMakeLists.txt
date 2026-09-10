# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
kerosene_library(toolcommon
    LAYER tool
    SOURCES
        cli.cpp
        project.cpp
    PUBLIC_DEPS kerosene::core kerosene::kv)
