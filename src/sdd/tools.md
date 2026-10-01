# Tools

## Role

The tools are layer 5. They link the engine's data crates (including
`kerosene-map`, which the runtime never does) and produce the compiled content
the runtime loads. The runtime binary (`apps/kerosene`) links **no** tool code.

## The set

| Tool | Does | Source-engine analogue |
|---|---|---|
| `chisel` | Level editor: brushes, entities, 3D view, build | Hammer |
| `cleave` | CSG, BSP, portals, leak detection | `vbsp` |
| `umbra` | Visibility (PVS) | `vvis` |
| `radiance` | Lighting bake | `vrad` |
| `resonance` | Acoustics bake | none |
| `alchemy` | Textures and materials | none |
| `forge` | Models from OBJ/glTF | none |
| `timbre` | Sound compile and edit | none |
| `vault` | Pack and inspect `.vault` archives | none |
| `loupe` | Inspect compiled files | none |
| `kiln` | Build and ship a whole project | none |
| `kerosene-tools` | One egui app hosting all of the above, plus headless subcommands | none |

`kerosene-toolui` provides the shared egui theme and widgets.

## Design decisions

- **One window, many tools.** `kerosene-tools` hosts the tools as tabs (Home,
  Assets, Editor, Models, Sound, Build, Archive) with a command palette and an
  output panel, so a designer does not juggle executables. Each tool also runs
  headlessly (`kerosene-tools cleave <map>`) for scripts and CI.
- **A game can ship its editor.** The `tools` feature of the `kerosene` crate
  re-exports the tools so a game can build its own tools binary.
- **The editor mirrors Hammer 5's layout** to be familiar to Source mappers.
- **Chisel renders on the GPU**, using the same renderer as the runtime; a
  software rasteriser is kept for tests.
- **Stages are shared.** Chisel's build and Kiln call the same
  `toolchain::MapStages`, so there is one definition of "build a map".
- **Re-invocation over discovery.** A tool launches another stage by
  re-invoking its own executable (`toolchain::command`), so a subcommand is
  always present; the runtime is found next to the executable, then on `PATH`.

## See also

- [Tools](../docs/tools.md)
- [Tools and the build](../devnotes/tools-and-build.md)
- [Data and the build pipeline](data-and-pipeline.md)
