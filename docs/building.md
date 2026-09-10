# Building Kerosene

## What you need

A C++23 compiler (developed against GCC 16), CMake 3.28 or later, and — for the
renderer — the system libraries SDL binds to. SDL3 itself is fetched and built
from source by CMake at a pinned tag, so it is not something to install.

Everything except the renderer builds with **no graphics headers at all**.

## On an image-based Fedora desktop

Kerosene is developed on Bazzite, where the host image carries no development
headers and adding them means layering packages onto the OS and rebooting. A
toolbox container is the way out: the `-devel` packages live in the container,
the host image stays untouched, and the Wayland socket and DRI devices pass
through so a windowed build still runs.

```sh
./scripts/devshell.sh          # create the container, install packages, enter it
```

The script is idempotent — running it again installs only what is missing — and
it takes a command to run non-interactively:

```sh
./scripts/devshell.sh cmake --build --preset debug
```

## On a traditional distribution

Install the equivalents of what `scripts/devshell.sh` lists and skip the script.
On Fedora that is roughly:

```
gcc-c++ cmake ninja-build
wayland-devel wayland-protocols-devel libxkbcommon-devel libdecor-devel
libX11-devel libXext-devel libXrandr-devel libXcursor-devel libXi-devel
vulkan-headers vulkan-loader-devel mesa-libGL-devel mesa-libEGL-devel
glslang glslc spirv-tools
alsa-lib-devel pipewire-devel pulseaudio-libs-devel
libasan libubsan libtsan
```

## Presets

| Preset | What it is |
|---|---|
| `debug` | Everything, with assertions. The one to develop in. |
| `release` | `RelWithDebInfo` with link-time optimisation. |
| `nogfx` | Everything **but** the renderer. Needs no graphics headers, uses Makefiles rather than Ninja, and is what CI runs. |
| `asan` | Address and leak sanitizer. |
| `ubsan` | Undefined-behaviour sanitizer, non-recovering. |
| `tsan` | Thread sanitizer. For the job system and anything sharing state across it. |

```sh
cmake --preset debug
cmake --build --preset debug
ctest --preset debug
```

`ctest` compiles the sample content itself, as a fixture, so a fresh clone runs
green rather than failing on a missing `.kbsp` and leaving you to guess which
step was skipped.

### Which preset to reach for

Run `nogfx` when you are working on the compilers, the geometry, the movement
model or the entity graph — it is the whole test suite, it needs nothing
installed, and it builds in a few seconds. Run `debug` when the change could
affect what is on screen.

Run `tsan` after touching `src/core/jobs.cpp` or anything the compile stages
call from a job. A work-stealing scheduler's bugs do not show up in a passing
test run.

## The build enforces the layering

`cmake/KeroseneTargets.cmake` fails configuration if an engine library reaches
into the toolset, naming the offending edge. Source's design worked because the
compilers were separate from the game, and that boundary is one careless
`#include` from being gone in a commit nobody reads closely — so it is checked
rather than trusted.

The same applies in spirit to the renderer: `kerosene::engine` does not link
`kerosene::render`, which is what makes `--headless` the dedicated server rather
than a mode.

## Warnings

Warnings are errors by default (`-DKEROSENE_WERROR=OFF` to relax it while
bisecting something). The set is deliberately strict, including `-Wconversion`
and `-Wdouble-promotion` — geometry code is exactly where an accidental
`double`-to-`float` hides, and the compile stages run in double precision on
purpose.

One warning is deliberately absent: `-Wuseless-cast`. It fires on casts that are
redundant here and load-bearing elsewhere — `size_t` to `uintptr_t` is the usual
one — so obeying it means writing code that is less portable, not more correct.

## Building the content

```sh
./scripts/build-content.sh
```

This runs `cleave` and then `umbra` over every `.kmap` under `content/maps`.
Maps are committed as sources; the engine loads only compiled `.kbsp`.
