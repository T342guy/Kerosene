# File formats

Every format Kerosene defines is its own, named and byte-tagged so it cannot be
mistaken for anyone else's. Kerosene cannot open Source or Quake content and
does not try to.

| Extension | Contents | Written by |
|---|---|---|
| `.kmap` | the editable map — text | Chisel, by hand |
| `.kbsp` | the compiled map | Cleave, then Umbra, then Radiance |
| `.kprt` | the portal graph — text | Cleave, read by Umbra |
| `.kleak` | a leak path — text | Cleave, on failure only |
| `.kproj` | the project file | by hand |
| `.kcfg` | configuration | the engine, or by hand |
| `.ktex` `.kmat` `.kmdl` `.kaud` `.vault` | textures, materials, models, sounds, archives | not yet implemented |

Compiled output is a build product. It is regenerable from the sources beside
it, which is why it is not committed and why version mismatches are a hard
error with a "recompile the map" message rather than a best-effort read.

## KeyValues

The text format `.kmap`, `.kmat`, `.kproj` and `.kcfg` all share. Named blocks
nest; leaves are quoted pairs.

```
world
{
    "classname" "worldspawn"
    solid
    {
        side
        {
            "plane"    "(512 -8 0) (-8 -8 0) (-8 264 0)"
            "material" "dev/grid"
            "uaxis"    "[1 0 0 0] 0.5"
        }
    }
}
```

Entries are held **in file order**, not in a map. Repeated names are ordinary —
a solid has six `side` blocks — and brush sides are referred to by position, so
a parser that merged or reordered them would corrupt maps rather than reject
them.

Comments run from `//` to the end of a line. Escapes are `\"`, `\\`, `\n` and
`\t`; a backslash before anything else is literal, so a Windows path pasted into
a material key survives.

Every parse failure carries a line and column, formatted the way a C compiler
has since forever, because "the map failed to parse" is not a diagnostic.

## `.kmap`

A map is a list of brushes and entities. **A brush is stored as a list of
planes, not vertices**, and each plane as the three points that wind it
counter-clockwise seen from the front.

Planes rather than vertices because a brush is then the intersection of its
sides' half-spaces: convexity is structural, so there is no way to author a
concave brush and no validation pass that rejects one. CSG operates on planes,
and deriving them from vertices that have already been rounded is how a wall
ends up very slightly not being a wall. Three points rather than a
normal-and-distance pair because three points survive a text round-trip exactly.

A side also carries its material, its two texture axes (`[x y z shift] scale`),
its lightmap scale and its smoothing group. An entity carries every key as
written — including ones this build does not implement, which is what lets the
game code gain a feature without every map being recompiled — plus a
`connections` block holding its I/O wiring.

## `.kbsp`

A lumped binary file: a header of (offset, length) pairs, then the lumps.
Little-endian, fixed-width, and free of padding by construction, so a lump is a
memory image of an array.

The shape is Quake's and Source's, and it is worth copying for what it makes
possible rather than what it looks like: lumps mean the three compile stages can
each add to a file the previous one wrote — Cleave the geometry, Umbra the
visibility, Radiance the lighting — without any of them knowing the others'
formats. That is what lets a stage be re-run, replaced or skipped. An unvised,
unlit map still loads and plays; it just draws everything and looks flat.

Magic `KBSP`, version 1.

| Lump | Contents |
|---|---|
| Entities | KeyValues text, exactly as `.kmap` wrote it, minus the brushes |
| Planes | normal, distance, axial type |
| Vertices | positions, welded |
| FaceVertices | index array; a face names a range in it |
| Faces | plane, facing, vertex range, texinfo, lightmap range, surface flags |
| Nodes | plane, two children, bounds |
| Leaves | cluster, contents, bounds, face range, brush range |
| LeafFaces, LeafBrushes | index arrays |
| Brushes | side range, contents, owning entity |
| BrushSides | plane (with facing in the low bit), texinfo, bevel flag |
| TexInfo | two texture axes, flags, material offset, lightmap scale |
| Materials | NUL-separated names |
| Visibility | the PVS and the PAS, run-length encoded. Written by Umbra |
| Lighting | lightmaps. Written by Radiance |
| Models | submodels; model 0 is the world |

A node's children are encoded as Quake did: non-negative is a node index,
negative is the leaf `-(child + 1)`. That makes a node's two children one array
with no discriminant and no indirection, which is what keeps a tree descent
tight.

Brushes are kept alongside the faces derived from them. The faces are what is
drawn; the brushes are what is collided with. Keeping both is what lets the
runtime trace an *arbitrary* box against the level: the brush's planes, plus the
bevel planes Cleave adds, are exactly the supporting planes of the Minkowski sum
for any box size. Quake and Source precompute a handful of fixed hull sizes and
snap every entity to the nearest.

A brush also records which entity it came from. Without it a trace can tell you
that you are standing in *a* trigger but not *which* one.

### Visibility

Two vectors per cluster, not one: what a cluster can **see**, and what can be
**heard** from it. The audible set is a looser flood, one portal hop further,
because sound goes round corners. Source keeps this distinction and it is why
audio does not cut out when you step behind a pillar.

Both are run-length encoded on zero bytes — a cluster in a large level sees a
small fraction of it — and an empty or unreadable lump decodes to all-ones, so
an unvised level draws everything. A visibility bug should look slow, not
broken.

## `.kprt`

The portal graph, as text, written by Cleave and read by Umbra.

```
KPRT1
<cluster count>
<portal count>
<point count> <front cluster> <back cluster> (x y z) (x y z) ...
```

Text on purpose: it is small, it is the input to a stage someone might want to
replace, and a visibility bug is far easier to chase when you can read the
portals in an editor.

## `.kleak`

A polyline, written only when a level is not sealed. It runs from the entity the
flood escaped from to the hole it escaped through, shortest path first.

```
// Kerosene leak path.
128 128 96
128 128 128
136 128 400
```
