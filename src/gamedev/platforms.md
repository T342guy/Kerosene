# Platforms & distribution

Where a Kerosene game can run today, where it can be sold, and what each
choice costs. The short version: Linux is the only platform anyone has
tested; everything else is *untested* rather than *known broken*, and no
storefront has any integration yet.

## Operating systems

| Platform | Status | Notes |
|---|---|---|
| Linux | Developed and tested here | Audio needs ALSA headers to *build* (`libasound2-dev` / `alsa-lib-devel`; on an image-based system such as Bazzite or Silverblue, `rpm-ostree install alsa-lib-devel`); without them build with `--no-default-features` and everything but sound works. `kerosene-tools doctor` checks. Players need nothing extra |
| Windows | Compiled in CI; never played by hand | wgpu, winit and cpal all support it. CI compiles the workspace and its tests there. `kiln --ship` links the C runtime statically, so players need no Visual C++ redistributable |
| macOS | Compiled in CI; never played by hand | Same dependencies, same caveat. wgpu uses Metal |
| Consoles | No | No SDKs, no plans in the tree |
| Mobile | No | Touch input does not exist |
| Web | No | wgpu can target WebGPU; the file system, audio and process model here assume a desktop |

CI (`.github/workflows/ci.yml`) runs the tests on Linux, and makes a game
with `kerosene-tools new` there and plays it headless. On Windows and macOS
it only compiles, so "compiles" is known there and nothing more: if you ship
on either you are the first to find out whether it runs.

Build each platform's copy on that platform: clone the game there and run
`cargo ship`. `kiln --ship dist --target <triple>` cross-compiles for a
developer who has that target's linker and libraries set up; that setup is
Rust's to document, and building natively is the supported way.
`scripts/build-content.sh` is a Linux
shell script, but it is only a wrapper: `kerosene-tools kiln` is what it
calls and that is a program, on every platform.

## Storefronts

Nothing in the licence stops you selling on any of these. Steam is the one
the engine integrates with; see [Steam](steam.md).

| Store | What works | What is missing |
|---|---|---|
| itch.io | Zip the `kiln --ship` folder and upload it | A butler push target in Kiln |
| Steam | The `steam` feature: achievements, stats, leaderboards, rich presence, cloud files, DLC, the overlay and Workshop mounting. `kiln --ship --steam` installs Valve's library and writes the SteamPipe scripts | Steam Input, lobbies and networking, leaderboard downloads |
| GOG, Epic | The folder, as above | Everything the store's SDK offers |

The Workshop is worth a sentence because it is the store feature the engine
is built for. The formats are open and each map is one `.vault`, so a
subscribed map mounts as one more VFS layer with no format work at all. The
engine does exactly that at startup, and `kerosene-tools workshop upload`
puts a `.vault` up.

## Dedicated servers

`kerosene --headless <ticks>` runs the simulation with no window, and it is
what a dedicated server *is*, not a test mode — but there is no network
stack for it to serve. Until there is, a server is a single-player game with
no screen.

## Packaging

`kiln --ship` produces a folder, and a folder is the distribution: no
installer, no auto-updater, no code signing. Zip it. On Linux the executable
bit is preserved through the copy; on other platforms the archive format has
to preserve it for you.

The game finds its content by climbing from the executable, and the shipped
`.kproj` says `content = "content"`, so the folder works from wherever it
is unpacked and does not care about the working directory.

## What the player needs

* A GPU and driver wgpu can talk to: Vulkan on Linux, and presumably DX12 or
  Metal elsewhere.
* Write access to the content folder on first run, for `engine.kcfg`
  (see [Publishing](publishing.md#you-can-but)).
* Nothing else. There is no runtime, no redistributable, no shared library:
  the game is one static binary and one archive.
