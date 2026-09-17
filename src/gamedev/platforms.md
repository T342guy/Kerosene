# Platforms & distribution

Where a Kerosene game can run today, where it can be sold, and what each
choice costs. The short version: Linux is the only platform anyone has
tested; everything else is *untested* rather than *known broken*, and no
storefront has any integration yet.

## Operating systems

| Platform | Status | Notes |
|---|---|---|
| Linux | Developed and tested here | Audio needs ALSA headers to *build* (`libasound2-dev` / `alsa-lib-devel`); without them build with `--no-default-features` and everything but sound works. Players need nothing extra |
| Windows | Untested | wgpu, winit and cpal all support it. `kiln --ship` already names the binary `.exe` on Windows. Nobody has run it |
| macOS | Untested | Same dependencies, same caveat. wgpu uses Metal |
| Consoles | No | No SDKs, no plans in the tree |
| Mobile | No | Touch input does not exist |
| Web | No | wgpu can target WebGPU; the file system, audio and process model here assume a desktop |

There is no continuous integration, so "untested" is the whole story rather
than a hedge. If you ship on Windows or macOS you are the first, and the
three-OS build matrix is the top of the engine's own roadmap
(`missing-features.md`, "An order"). `scripts/build-content.sh` is a Linux
shell script, but it is only a wrapper: `kerosene-tools kiln` is what it
calls and that is a program, on every platform.

## Storefronts

Nothing in either licence stops you selling on any of these. Nothing in the
engine helps you with any of them yet.

| Store | What works | What is missing |
|---|---|---|
| itch.io | Zip the `kiln --ship` folder and upload it | A butler push target in Kiln |
| Steam | Upload the same folder as a depot. The overlay hooks Vulkan and DX12 and should work without help | Steamworks entirely: no achievements, stats, cloud saves, Workshop, Steam Input, rich presence. The SDK is proprietary and cannot live in the tree, so this will be a Cargo feature when it exists |
| GOG, Epic | The folder, as above | Everything the store's SDK offers |

Workshop is worth a sentence because it is the one the engine is built for:
open formats plus a `.vault` per map means a subscribed map could mount as
one more VFS layer with no format work at all. That is the plan; none of it
is written.

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
`.keroproj` says `content = "content"`, so the folder works from wherever it
is unpacked and does not care about the working directory.

## What the player needs

* A GPU and driver wgpu can talk to: Vulkan on Linux, and presumably DX12 or
  Metal elsewhere.
* Write access to the content folder on first run, for `engine.kconfig`
  (see [Publishing](publishing.md#you-can-but)).
* Nothing else. There is no runtime, no redistributable, no shared library:
  the game is one static binary and one archive.
