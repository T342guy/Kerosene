# Configuration

The engine has settings that belong to no single map and no single project:
which renderer to use, how big the window is, whether it syncs to the display.
They live in one file at the top of the content tree:

    engineconf.keroconfig

The file **always exists**. The first program to look for it and not find it
writes one with the defaults in it, so a fresh clone runs before anyone has
edited anything, and a hand-edited file is read back on the next run.

## The file

KeyValues, like everything else a person is expected to edit:

```text
engineconf
{
    "renderer" "vulkan"
    "width"    "1280"
    "height"   "720"
    "vsync"    "1"
}
```

Every key is optional. A key that is absent falls back to its default, and a
key that is wrong (a renderer nobody has heard of, a width that is not a
number) logs a warning and falls back rather than refusing to start. A config
must not be the reason the engine will not boot.

| Key | Default | Meaning |
|---|---|---|
| `renderer` | `vulkan` | `vulkan`, `metal`, `dx12`, `gl`, or `auto`. The backend the window asks for. |
| `width` | `1280` | Window width in pixels. |
| `height` | `720` | Window height in pixels. |
| `vsync` | `1` | `1`/`0` (also `true`/`false`, `yes`/`no`, `on`/`off`). |

## The renderer

The renderer defaults to **Vulkan**. The engine asks wgpu for the Vulkan
backend first, and only falls back to whatever else the machine has when
Vulkan is not there — with a log line saying it fell back, so the choice is
never silent.

`auto` means "whatever wgpu would have picked", which on Linux is usually
Vulkan and elsewhere is Metal or DirectX 12. `metal` and `dx12` exist for the
reverse case: a machine with several backends where someone wants to pin one
for a reason of their own.

Every program that draws a window reads the same setting — the game, Chisel,
and the tool windows — so the renderer choice is made once, in the file, not
per program.
