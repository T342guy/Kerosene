# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
find_package(Threads REQUIRED)

kerosene_library(core
    LAYER engine
    SOURCES
        assert.cpp
        arena.cpp
        jobs.cpp
        log.cpp
    PUBLIC_DEPS
        Threads::Threads)
