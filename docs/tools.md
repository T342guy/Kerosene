# Tool reference

Seven programs. None of them is the engine, and none of them depends on it.

---

## Chisel — the world editor

```sh
chisel [map.voidmap] [--content <dir>]
```

Four viewports: 3D, top (x/y), front (x/z) and side (y/z). Hammer's layout,
because brush geometry is axis-aligned far more often than not and an
orthographic view along an axis is the only way to place a vertex exactly
without typing numbers.

| Key | |
|---|---|
| `1` `2` `3` `4` | select, block, entity, texture tool |
| `[` `]` | finer / coarser grid |
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo |
| `Ctrl+S` | save |
| `Delete` | delete selection |
| `Escape` | clear selection, cancel a drag |
| `F9` | compile (fast) and run |
| Middle-drag | pan (2D) / move (3D) |
| Right-drag | look around (3D) |
| Scroll | zoom (2D) / move forward (3D) |
| Shift-click | add to or remove from the selection |

**Building a level.** Draw brushes with the block tool in a 2D view; they snap
to the grid, outward, so a brush is never smaller than the rubber band. Pick a
material from the left panel — picking one with something selected applies it.
Place entities with the entity tool. Select brushes and *tie* them to an entity
(`func_door`, `trigger_multiple`) in the right panel; that is how a door is
made. Wire outputs to inputs in the same panel.

Clicking a brush that belongs to an entity selects the entity, not the brush:
that is what a designer means by "the door".

**Entity properties.** The inspector is driven by the game's class definitions
-- the `.voiddef` files Chisel finds under the content root. For a selected
entity it lists *every* key its class reads, whether or not the entity has been
given a value for one, with the type, the game's default and a line of help.
Keys are edited with a widget suited to what they hold: a colour picker for a
light's colour, checkboxes for a spawnflag field, a menu of the map's entity
names when wiring an output. A key the definitions do not describe is still
shown -- that is how a typo becomes visible rather than silent.

Without a `.voiddef` the inspector can only show the keys an entity already
carries, which for a freshly placed entity is none. Chisel says so in the
status bar rather than looking like a game with no settings.

**The 3D pane** is rasterised in software with a depth buffer, so what hides
what is decided per pixel. It shows shape, scale and selection -- not textures
or lighting, which is what compiling and running the map is for, one keystroke
away.

Chisel runs the compilers as separate programs, looking for them beside its own
executable and then on `PATH`. `map → check tools are installed` says which it
found.

---

## Cleave — the BSP compiler

```sh
cleave map.voidmap [-o out.voidbsp] [--ignore-leaks] [--no-fill] [--dry-run] [-v]
```

`.voidmap` → `.voidbsp` plus a `.voidprt` portal graph for Umbra.

Reports every brush and entity problem in one pass rather than stopping at the
first, because a designer would rather fix five brushes in one cycle than five.

**Leaks.** If the flood fill escapes to the void, the map is not sealed and
Cleave refuses to build it — visibility would be nearly useless and the compile
would take far longer. `--ignore-leaks` builds it anyway and writes a `.voidleak`
trace naming the route out, which is the only practical way to find a one-unit
gap in a large map.

### Tool materials

Compile-time intent is expressed with the texture applied to a face, as in
Source. Everything under `tools/` is a tool material.

| Material | Effect |
|---|---|
| `tools/nodraw` | Solid, never drawn. The workhorse. |
| `tools/clip` | Blocks players; invisible; bullets and sight pass through |
| `tools/npcclip` | Blocks AI only |
| `tools/trigger` | Not solid, but traces find it |
| `tools/skybox` | Draws as sky; where sunlight enters the world |
| `tools/hint` | Forces a BSP split along its plane, then vanishes |
| `tools/skip` | Does nothing — what a hint brush's other faces wear |
| `tools/blocklight` | Casts a shadow without being solid |
| `tools/grate` | Blocks movement and bullets; you can see through it |
| `tools/water` | Water |

An entity's classname overrides its brushes' materials: a `trigger_multiple` is
a trigger whatever its faces are textured with, and a `func_detail` is detail.

**Detail brushes** stay out of the world tree. A handrail modelled from thirty
brushes would otherwise carve the room into thirty slivers, each of which the
visibility compile then has to consider. This is the single biggest lever a
designer has over compile time.

---

## Umbra — the visibility compiler

```sh
umbra map.voidbsp [--portals map.voidprt] [--fast] [--dry-run]
```

Computes which clusters can see which, and writes the PVS back into the map.

Reports "clusters visible per cluster" — the single best predictor of frame
rate in a BSP engine. Lower is better.

`--fast` stops after the base estimate: much quicker, leaves far too much
visible, and exactly what you want while a layout is still moving.

---

## Radiance — the lighting compiler

```sh
radiance map.voidbsp [--samples 1-8] [--bounces 0-8] [--scale N]
                  [--ambient-scale N] [--fast] [--dry-run]
```

Bakes static lighting from the map's own light entities.

| Entity | |
|---|---|
| `light` | A point light. `_light` is `"r g b brightness"`. |
| `light_spot` | A cone. `_cone`, `_inner_cone`, `_exponent`, `pitch`. |
| `light_environment` | The sun and the sky. `_light` is the sun, `_ambient` the fill. |

Brightness reads directly at 100 inches with the default quadratic falloff, so
a `_light` of `"255 255 255 200"` delivers 200 units of light at normal room
distance.

`pitch` on a light entity is **upward-positive**, so a lamp shining at the
floor is `pitch -90`.

`--samples` softens shadow edges at quadratic cost. `--bounces 1` is what turns
a room lit by a single lamp from a hard pool of light into something that reads
as an interior.

A map with no lights compiles fine and is then pitch black, which looks like a
broken renderer — so Radiance says so.

---

## Alchemy — textures and materials

```sh
alchemy compile art/grid.png -o materials/dev/grid.voidtex [--normal] [--clamp] [--ui]
alchemy material dev/grid --basetexture dev/grid --shader lit
alchemy batch art -o materials --make-materials
alchemy info materials/dev/grid.voidtex
```

Compiles PNG/JPEG/TGA into `.voidtex` and authors `.voidmat` materials.

Alpha is dropped when an image does not use it, which saves a quarter of the
memory. In `batch` mode, a file ending `_normal` or `_n` is taken to be a
normal map — the convention beats a flag, because batch compiles run
unattended.

---

## Forge — the model compiler

```sh
forge compile art/crate.obj -o models/props/crate.voidmdl
                            [--scale-metres] [--z-up] [--scale N]
                            [--material old=new] [--recompute-normals]
forge info models/props/crate.voidmdl
```

OBJ → `.voidmdl`, splitting by material and welding vertices.

Two conversions happen on the way in, and getting either wrong produces a model
that is subtly rotated or a hundred times too small. OBJ is Y-up with -Z
forward; VoidEngine is Z-up with +X forward. And modelling packages usually
work in metres — `--scale-metres` converts.

Welding is by the full corner tuple, not by position: two faces meeting at a
hard edge legitimately share a position while needing different normals, and
merging them rounds off every corner of the model.

---

## Vault — content archives

```sh
vault pack content -o content.vault [--ext voidtex --ext voidmat] [--exclude tmp]
vault list content.vault [--long]
vault verify content.vault
vault unpack content.vault -o extracted
```

Packs a content tree into one archive. Every entry carries a CRC, checked on
read and by `verify`.

Loose files still shadow packed ones when both are mounted, so a developer can
drop a file next to a shipped archive without repacking.

---

## void — the runtime

```sh
void [+command ...] [--content <dir>] [--vault <file>] [--headless <ticks>]
```

Arguments beginning with `+` are console commands, so any convar is settable
from the command line with no flag needing to exist for it:

```sh
void +map void_start
void +map void_start +sv_gravity 200 +developer 1
void --headless 640 +map void_start
```

`--headless` runs the simulation with no window at all — which is what a
dedicated server is, not a testing mode bolted on the side.

### Useful convars

| Convar | |
|---|---|
| `sv_gravity` `sv_maxspeed` `sv_accelerate` | movement tuning |
| `sv_airaccelerate` `sv_air_max_wishspeed` | air control — see below |
| `sv_jump_height` | jump height in units; the impulse is derived from it |
| `sv_stepsize` | tallest step walked up without jumping (18) |
| `cl_fov` `sensitivity` `m_yaw` `m_pitch` | view and mouse |
| `r_drawworld` `r_fullbright` `r_lightmap` `r_novis` | rendering toggles (cheat) |
| `r_speeds` | per-frame culling and draw statistics |
| `mat_exposure` | overall brightness |
| `developer` | verbosity; `2` also traces entity I/O |

`sv_air_max_wishspeed` is the air-speed cap that makes bunny-hopping and
surfing work. It is 30 by default, and it is not a bug: changing it changes the
game.
