#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#
# Create and enter the Kerosene development container.
#
# Kerosene is developed on an image-based Fedora desktop (Bazzite/Silverblue),
# where the host carries no development headers and gaining them means layering
# packages onto the OS image and rebooting. A toolbox container is the way out:
# the -devel packages live in the container, the host image stays untouched, and
# the Wayland socket and DRI devices pass through, so a windowed build still
# runs. Nothing here is Bazzite-specific -- on a traditional distribution,
# install the same packages and skip the script.
set -euo pipefail

CONTAINER="${KEROSENE_CONTAINER:-kerosene-dev}"
IMAGE="${KEROSENE_IMAGE:-registry.fedoraproject.org/fedora-toolbox:44}"

PACKAGES=(
    # Toolchain
    gcc-c++ cmake ninja-build git

    # Sanitizer runtimes, for the asan/ubsan/tsan presets. GCC can compile with
    # -fsanitize= without these, but the link fails, so they belong here rather
    # than in the "install it when you need it" pile.
    libasan libubsan libtsan

    # What SDL binds to for windowing and input. SDL itself is built from
    # source by CMake FetchContent; these are the system libraries underneath.
    wayland-devel wayland-protocols-devel libxkbcommon-devel libdecor-devel
    libX11-devel libXext-devel libXrandr-devel libXcursor-devel libXi-devel
    libXfixes-devel libXScrnSaver-devel

    # Graphics. SDL_GPU targets Vulkan on Linux; the GL headers are for SDL's
    # fallback paths and for anything that wants a GL context later.
    vulkan-headers vulkan-loader-devel mesa-libGL-devel mesa-libEGL-devel

    # Shader compilation, at build time -- the runtime never needs these.
    glslang glslc spirv-tools

    # Audio backends.
    alsa-lib-devel pipewire-devel pulseaudio-libs-devel
)

if ! command -v toolbox >/dev/null 2>&1; then
    echo "toolbox not found. On a traditional distribution you do not need this" >&2
    echo "script: install the equivalent -devel packages and build directly." >&2
    exit 1
fi

if ! toolbox list --containers 2>/dev/null | grep -qw "$CONTAINER"; then
    echo "==> creating toolbox '$CONTAINER' from $IMAGE"
    # --assumeyes because the image download otherwise stops on a y/n prompt,
    # which a script run from a build pipeline will never answer. Without it
    # toolbox exits 0 having created nothing, and the failure only shows up as
    # a missing compiler much later.
    toolbox create --assumeyes --image "$IMAGE" --container "$CONTAINER"
fi

if ! toolbox list --containers 2>/dev/null | grep -qw "$CONTAINER"; then
    echo "toolbox '$CONTAINER' was not created. Try running the create by hand:" >&2
    echo "    toolbox create --image $IMAGE --container $CONTAINER" >&2
    exit 1
fi

# Idempotent: dnf reports already-installed packages as nothing to do, so
# re-running this after adding a package to the list installs only the new one.
echo "==> installing development packages in '$CONTAINER'"
toolbox run --container "$CONTAINER" sudo dnf install -y --setopt=install_weak_deps=False "${PACKAGES[@]}"

if [[ $# -gt 0 ]]; then
    exec toolbox run --container "$CONTAINER" "$@"
fi

echo
echo "==> entering '$CONTAINER'. Build with:"
echo "      cmake --preset debug && cmake --build --preset debug && ctest --preset debug"
exec toolbox enter "$CONTAINER"
