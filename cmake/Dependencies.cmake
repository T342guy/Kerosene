# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# Every third-party dependency, pinned to an exact tag and fetched at configure
# time. Nothing here is expected to be installed on the machine: what the system
# has to provide is the headers SDL binds *to* (Wayland, X11, xkbcommon, Vulkan),
# which scripts/devshell.sh installs into a toolbox container.

include(FetchContent)
set(FETCHCONTENT_QUIET OFF)

if(KEROSENE_BUILD_TESTS)
    FetchContent_Declare(doctest
        GIT_REPOSITORY https://github.com/doctest/doctest.git
        GIT_TAG        v2.4.12
        GIT_SHALLOW    TRUE
        SYSTEM)
    FetchContent_MakeAvailable(doctest)
    # doctest_discover_tests() lives in doctest's own module directory, which
    # FetchContent does not add to the module path for us.
    list(APPEND CMAKE_MODULE_PATH "${doctest_SOURCE_DIR}/scripts/cmake")
endif()

if(KEROSENE_BUILD_RENDER)
    # SDL3 supplies the window, the input, the audio device and SDL_GPU, which
    # is the graphics API: one dependency where Source needed a platform layer
    # plus a shaderapi DLL per graphics API.
    set(SDL_SHARED   ON  CACHE BOOL "" FORCE)
    set(SDL_STATIC   OFF CACHE BOOL "" FORCE)
    set(SDL_TEST_LIBRARY OFF CACHE BOOL "" FORCE)
    set(SDL_INSTALL  OFF CACHE BOOL "" FORCE)

    FetchContent_Declare(SDL3
        GIT_REPOSITORY https://github.com/libsdl-org/SDL.git
        GIT_TAG        release-3.2.24
        GIT_SHALLOW    TRUE
        SYSTEM)
    FetchContent_MakeAvailable(SDL3)
endif()
