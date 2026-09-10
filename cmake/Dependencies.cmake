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

    # Dear ImGui, for the toolset window.
    #
    # Immediate mode is the right shape for a tool: a properties panel that
    # rebuilds itself from the document every frame cannot show stale state,
    # which is most of what goes wrong in an editor's UI. It looks like a tool
    # rather than a product, which for an editor is the right way round.
    #
    # The docking branch, because a four-viewport editor wants real dockable
    # panes. ImGui ships no CMakeLists of its own, so the target is built here
    # from its sources plus the two backends -- which is also why it is a
    # handful of files rather than a build system to configure.
    FetchContent_Declare(imgui
        GIT_REPOSITORY https://github.com/ocornut/imgui.git
        GIT_TAG        v1.92.9b-docking
        GIT_SHALLOW    TRUE
        SYSTEM)
    FetchContent_MakeAvailable(imgui)

    add_library(kerosene_imgui STATIC
        "${imgui_SOURCE_DIR}/imgui.cpp"
        "${imgui_SOURCE_DIR}/imgui_draw.cpp"
        "${imgui_SOURCE_DIR}/imgui_tables.cpp"
        "${imgui_SOURCE_DIR}/imgui_widgets.cpp"
        "${imgui_SOURCE_DIR}/imgui_demo.cpp"
        "${imgui_SOURCE_DIR}/backends/imgui_impl_sdl3.cpp"
        "${imgui_SOURCE_DIR}/backends/imgui_impl_sdlgpu3.cpp")
    add_library(kerosene::imgui ALIAS kerosene_imgui)

    # SYSTEM, so ImGui's own warnings are not this project's problem. The
    # warning set here is deliberately strict and tuned for geometry code; a
    # vendored dependency should not have to satisfy it.
    target_include_directories(kerosene_imgui SYSTEM PUBLIC
        "${imgui_SOURCE_DIR}"
        "${imgui_SOURCE_DIR}/backends")
    target_link_libraries(kerosene_imgui PUBLIC SDL3::SDL3)
endif()
