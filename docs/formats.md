# File formats

| Extension | What | Text or binary | Written by | Source analogue |
|---|---|---|---|---|
| `.voidmap` | Editable map source | text (KeyValues) | Chisel | `.vmf` |
| `.voidbsp` | Compiled map | binary, lump directory | Cleave / Umbra / Radiance | `.bsp` |
| `.voidprt` | Portal graph | text | Cleave | `.prt` |
| `.voidleak` | Leak trace | text | Cleave | `.lin` |
| `.voidtex` | Texture | binary | Alchemy | `.vtf` |
| `.voidmat` | Material | text (KeyValues) | Alchemy, by hand | `.vmt` |
| `.voidmdl` | Model | binary | Forge | `.mdl` |
| `.voiddef` | Entity class definitions | text (KeyValues) | the game, by hand | `.fgd` |
| `.voidscript` | Level script | text (Rhai) | by hand | `.nut` (VScript) |
| `.voidsnd` | Sound script | text (KeyValues) | by hand | `game_sounds.txt` |
| `.wav` | Sound | binary (RIFF) | any audio tool | `.wav` |
| `.vault` | Content archive | binary | Vault | `.vpk` |

Text where a person edits or reviews it; binary where the engine loads it.

Every coordinate in every one of them is in **void units** -- one void unit is
one inch, a player is 72 of them tall. Angles are degrees, times are seconds,
and Z is up.

## KeyValues

The text format `.voidmap`, `.voidmat`, `.voiddef` and the compiled entity lump
all use.

```
world
{
    "classname" "worldspawn"
    "skyname"   "sky_void"
    solid
    {
        "id" "1"
        // faces follow
    }
}
```

Two properties drive the implementation. **Keys repeat** — a `world` holds many
`solid` blocks — so entries are an ordered list, never a map; a `HashMap` would
silently eat brushes. And **order is meaningful**, so round-tripping a map
through the editor does not reshuffle it and produce a noisy diff.

Backslashes are literal, so `materials\dev\grid` survives intact. Only the
escapes the writer emits (`\"`, `\\`, `\n`, `\t`) are resolved on read.

## `.voidmap` — editable maps

```
versioninfo { "formatversion" "1" }
world
{
    "id" "1"
    "classname" "worldspawn"
    solid
    {
        "id" "2"
        side
        {
            "id" "3"
            "plane"    "(0 0 0) (0 64 0) (64 64 0)"
            "material" "dev/grid"
            "uaxis"    "[1 0 0 0] 0.25"
            "vaxis"    "[0 -1 0 0] 0.25"
        }
    }
}
entity { "id" "9" "classname" "info_player_start" "origin" "0 0 32" }
```

**Brushes are planes, not vertices.** A solid is the intersection of its faces'
half-spaces. That makes convexity structural — you cannot author a brush with a
hole in it — and it is what makes CSG possible. The cost is that a solid's
actual polygons only exist once something computes them.

Each plane is written as three points, listed **clockwise as seen from the
front of the face**, so the normal is `(p0 - p1) × (p2 - p1)`. That looks like
the wrong cross product and is not: every brush face ever written to a
`.map`-lineage file depends on exactly this ordering.

Faces carry no UVs. They carry two *texture axes* — world vectors a point is
projected onto — which is what makes texturing feel the way it does in a brush
editor: drag a brush and the texture stays locked to world space.

## `.voidbsp` — compiled maps

A header (`VOID`, a version, a 20-slot lump directory) followed by flat arrays
of `#[repr(C)]` records. Every record is padding-free, so loading a lump is a
bounds check and a cast rather than a parse.

| Lump | Holds |
|---|---|
| entities | KeyValues text, one block per entity |
| planes | Pairs: index `n^1` is always the inverse of `n` |
| vertices, edges, surfedges | Shared vertex positions and the ring indirection |
| faces | Renderable polygons, with lightmap extents |
| nodes, leaves | The BSP tree |
| leaffaces, leafbrushes | What each leaf holds |
| models | Model 0 is the world; 1..n are brush entities |
| brushes, brushsides | Convex collision volumes |
| texinfo, texdata, texdata_strings | Materials and their projections |
| visibility | The run-length-encoded PVS |
| lighting | Baked lightmap samples |

**The surfedge indirection.** A face's vertices are reached through a run of
*surfedges*, each a signed index into the edge lump — negative meaning "walk
this edge backwards". Two faces meeting at an edge therefore share one edge
record and one pair of vertex positions, which is what keeps their seam
watertight no matter how the arithmetic rounds.

**Plane pairs.** Planes always live at indices `2k` and `2k+1`, so flipping one
is `index ^ 1`. A node stores one plane index and its two children implicitly
use the plane and its inverse.

**Lightmap samples** are `ColorRgbExp32`: three bytes and a shared exponent.
That is what lets a baked lightmap carry values well above 1.0 — a bright sky,
a lamp against a wall — in four bytes instead of twelve, and it is why the
lighting can be tone-mapped at runtime rather than clipped at bake time.

Every index in the file is validated at load. A dangling one becomes an
out-of-bounds read deep inside the renderer, where the cause is invisible.

## `.voidprt` — the portal graph

Written by Cleave, read by Umbra.

```
VPRT1
<cluster count>
<portal count>
<points> <cluster a> <cluster b> (x y z) (x y z) ...
```

The winding's own plane normal points toward the first cluster listed. Only
portals between two non-solid leaves appear: sight does not travel through
rock, so a portal with a solid side is not a portal.

## `.voidtex` — textures

A 48-byte header (dimensions, format, flags, average colour) then the mip
chain, largest first. Uncompressed RGBA8, RGB8 or R8.

Mipmaps, the average colour Radiance needs for bounce lighting, and sampling
intent are all resolved at build time. Doing them at load costs startup on
every run, and doing them *well* is not something to redo per launch.

## `.voidmat` — materials

```
lit
{
    "$basetexture" "dev/grid"
    "$bumpmap"     "dev/grid_normal"
    "$surfaceprop" "concrete"
}
```

The block name is the shader — `lit`, `unlit`, `sky`, `water`, `ui`. A small
closed set, because every one is a real code path; an open-ended string would
just be a way to fail at draw time instead of load time.

Geometry references *materials*, never textures, so retexturing a level or
making every metal surface reflective is one file change. Unknown parameters
round-trip rather than being dropped: a game will invent keys the engine has
never heard of.

## `.voidmdl` — models

A 64-byte header, then vertices, indices, meshes, bones and a string table.

One model holds several meshes, each with its own material, because a single
object routinely uses more than one. Splitting by material at compile time
means one draw call per mesh instead of sorting at runtime.

Vertices carry four bone influences whether or not the model is skinned. Eight
wasted bytes per vertex on a static prop buys one vertex layout, one shader
path and no branch in the hot loop.

Bones are listed parents-first, so a single forward pass can build world
transforms with no recursion and no sorting.

## The developer texture set

Not a format, but content the tools generate rather than ship, and worth
knowing where it comes from: `alchemy dev-textures` writes `content/art/dev/`
and `content/art/tools/` along with their materials.

`dev/` are 256x256 checkerboards. At the default texture scale of 0.25 world
units per texel that covers 64 vu, so the 4x4 grid on them reads as **16 vu
cells** -- the size most brushwork is done at -- with a fainter 4 vu
subdivision over it. The checker is what makes a stretched texture obvious: a
square that is not square is visible from across a room, where a stretched grid
line is not. Only `dev/measure` carries numbers, because a world-aligned
projection mirrors on opposite walls and text in a tiling texture therefore
reads backwards on half of them.

`tools/` are 128x128, flat, hatched, and labelled with their own name. A colour
alone does not tell `clip` from `playerclip` at a glance.

They are generated rather than committed because they are *defined* by numbers
-- this grid is 16 units, that colour means "blocks players" -- and a
definition that lives in a PNG is one nobody can read or review. A test checks
the set against the compiler's tool-material table in both directions: offering
a tool material the compiler treats as world geometry would silently wall off a
doorway.

## `.voiddef` — entity class definitions

What an editor needs to know about the game's entities: for each class, the
keys it reads, the inputs it answers to and the outputs it fires. The engine
never reads this — an entity there is a bag of whatever keys the map carries,
and that is deliberate. The file exists so that a person placing a `func_door`
is shown that `speed` and `lip` are things, instead of an empty panel.

It is the FGD relationship, and it is what keeps Chisel a separate program from
the game: the editor reads the game's file rather than linking its code.

```
base
{
    "name" "Entity"
    key   { "name" "targetname" "label" "Name" "type" "target_source"
            "help" "What other entities call this one." }
    input { "name" "Kill" "help" "Remove this entity from the map." }
}

class
{
    "name" "func_door"
    "kind" "brush"
    "base" "Entity"
    "help" "A brush that slides open and shut."
    key {
        "name" "spawnflags" "label" "Flags" "type" "flags" "default" "0"
        choice { "value" "1" "label" "Starts open" }
    }
    key    { "name" "speed" "type" "float" "default" "100" "help" "Units per second." }
    input  { "name" "SetSpeed" "parameter" "units per second" }
    output { "name" "OnFullyOpen" }
}
```

`kind` is `point`, `brush` or `any`, and decides which menu a class appears in.
A class may name several `base` blocks; their keys, inputs and outputs come
first, and a key the class redefines wins. Key types are `string`, `int`,
`float`, `bool`, `vec3`, `angles`, `color`, `target_source`,
`target_destination`, `material`, `model`, `choices` and `flags` — the set is
closed, so an unrecognised type is an error at load rather than a text box at
edit time.

Files are merged in sorted path order and a later definition of a class
replaces an earlier one, so a mod can drop its own file beside the game's.

A `default` is what the *game* assumes when a key is absent. Chisel shows it
greyed rather than writing it into the map, so a `.voidmap` only carries the
keys someone chose — which is what makes a diff between two saves readable.

`content/voidengine.voiddef` describes the sample game, and a test in
`void-game` checks it against the class registry in both directions: an input
the game handles and the file does not offer is a build failure, and so is an
input the file offers that nothing handles.

## `.voidscript` — level scripts

A map's script, loaded automatically when the level starts if it is named
after the map. The language is [Rhai](https://rhai.rs); the API, the hooks and
the limits are in [`scripting.md`](scripting.md).

```rhai
fn on_map_start() {
    for door in find_by_class("func_door") {
        print(`${door} at ${door.origin}`);
    }
}
```

Nothing about the format is special — it is source text the engine hands to a
VM. It is listed here because it is content the engine loads by name and packs
into a `.vault` with everything else.

## `.voidsnd` — sound scripts

What a sound name means: which file, how loud, how far it carries. The format
and the model behind it are in [`audio.md`](audio.md).

## `.vault` — content archives

```
[ header 40 bytes ][ directory, sorted by path ][ data blob ]
```

Each directory record is `path_len:u16, path, crc32:u32, offset:u64, size:u64`.
Paths are stored already normalised, so a lookup is a binary search with no
per-query allocation.

Reads are checked against the stored CRC. A corrupt archive — a truncated
download, a bad copy — is far better caught at the read than as a garbled
texture three subsystems later.

Output is byte-for-byte reproducible: insertion order does not leak into the
file, which matters for patching and for diffing releases.

## Virtual paths

Asset paths arrive in every spelling — `Materials\Dev\Grid`,
`materials/dev/grid`, `./materials//dev/grid` — and all resolve to one key:
lowercase, forward slashes, no redundant parts.

Paths that climb above the search root are **refused**, not clamped. Map and
material files are untrusted content that ships between users, and
`../../../../etc/passwd` in a `$basetexture` must not resolve to anything.

Search paths are consulted in the order they were added, first hit wins, and
loose files shadow packed ones — so during development you drop a file next to
a shipped archive and it takes effect immediately, with no repack.
