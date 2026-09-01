# Architecture

How the pieces fit, and why they are arranged this way.

## The shape of the thing

```
                    ┌──────────────────────────────────────────┐
   source content   │             build-time tools             │   runtime
                    └──────────────────────────────────────────┘

   art/*.png ──────────────► alchemy ──────► materials/*.kerotex
                                                    *.keromat  ──┐
   art/*.obj ──────────────► forge ──────► models/*.keromdl ─────┤
                                                              │
   maps/*.keromap ────────────► cleave ──────► maps/*.kerobsp       ├──► kerosene
        ▲                       │                │            │
        │                       └── *.keroprt ──► umbra ──► +vis   │
     chisel                                          │        │
        │                                        radiance     │
        └────────────────────────────────────────► +light ────┘
```

Everything above the line happens once, on a developer's machine or a build
server. Everything below happens sixty-four times a second on a player's.

That division is the single most important thing about a BSP engine, and it is
why the tools are separate programs rather than menu items. Visibility takes
minutes to compute and microseconds to query. Lighting takes minutes to bake
and nothing at all to sample. Neither belongs in the engine.

## Crate dependencies

```
        kerosene-math ──────────────────────────────┐
            │                                   │
            ├── kerosene-kv ── kerosene-map ── cleave   │
            │       │          │        │       │
            │       └── kerosene-bsp ◄──────┘       │
            │              │  ▲                 │
            │              │  └── umbra         │
            │              │  └── radiance      │
            │              ▼                    │
            ├── kerosene-physics                    │
            ├── kerosene-entity ── kerosene-game        │
            ├── kerosene-vfs ── kerosene-asset ─────────┤
            │                   │               │
            └── kerosene-render ◄───┘               │
                    │                           │
                kerosene-engine ── kerosene (runtime) ┘
                                chisel
```

Nothing points upward. `kerosene-math` knows about nothing; `kerosene-engine` knows
about everything. The tools sit off to the side, depending on the format crates
but never on the engine.

`kerosene-config` is not on the diagram because it sits to the side of all of
it: a small crate on top of `kerosene-kv` that reads `engineconf.keroconfig`,
the settings every program shares (which renderer, how big the window). The
game, Chisel, and the tool windows all ask it the same question, so the
renderer is chosen once in a file rather than once per program.

Two edges in that picture are there for a reason worth stating. `chisel`
depends on `alchemy`, because the editor builds the content tree's textures
itself rather than shelling out to a sibling binary that may not be beside it.
And everything -- the editor, the compilers, the runtime -- finds the content
tree through `kerosene_vfs::root`, one function they all call, and finds its
sibling binaries through `kerosene_vfs::toolchain`, likewise. Each of them working
either out separately is not a hypothetical: they did, they disagreed, and a
tool looking in the wrong directory is indistinguishable from a tool that is
broken. A shared answer that explains itself is the whole of the fix.

`kiln` sits above the compilers rather than beside them: it is the only tool
that runs the others, and it does so as subprocesses, so nothing about the
pipeline being one program leaks into the compilers being separate ones.

## The map pipeline in detail

### Cleave: `.keromap` → `.kerobsp`

1. **Brushes.** Each `.keromap` solid becomes a set of interned half-space planes.
   Plane interning matters more than it sounds: two faces meant to be coplanar
   must end up sharing *one* plane index, or the tree splits along a hair's
   width between them and the compile explodes.

2. **CSG.** Every face is cut against every brush that could bury it, and the
   buried parts are dropped. On a real map this removes about a third of all
   faces before anything else runs. The subtle case is coplanar faces: two
   brushes flush side by side keep exactly one shared face (lower index wins,
   deterministically); two brushes back to back drop it entirely.

3. **The tree.** Built top-down from the brushes' own planes, choosing at each
   step the plane that scores best on Quake's heuristic — coplanar faces are
   worth a great deal, splits cost the same amount, axial planes get a bonus.
   Detail brushes are held out of this entirely, which is the single biggest
   lever a designer has over compile time.

4. **Portals and flood fill.** Portals are the polygons where two leaves touch.
   Flooding outward from every entity through passable portals answers three
   questions at once: is the map sealed, which leaves are inside it, and what
   is the connectivity graph the visibility compile needs.

5. **Outside removal.** Leaves the flood never reached become solid. This
   deletes the entire outer shell of the level — the backs of every wall, the
   underside of the floor — and it is why a sealed map is so much cheaper than
   an open one.

6. **Emission.** Faces are filed down the tree into the leaves that can see
   them, vertices are welded so adjacent faces share records (or their seams
   crack open under rounding), and edges are shared through the surfedge
   indirection.

### Umbra: visibility

For every cluster, which other clusters can be seen from anywhere inside it.

Base vis is cheap and generous — two portals might see each other if each pokes
out on the far side of the other, flooded transitively. The full portal flow
then narrows a shrinking sight cone with *separating planes*: the planes
touching one edge of the source and one vertex of each portal passed through.
When the cone closes to nothing, everything beyond is invisible.

The result is a bit per cluster pair, run-length encoded. At runtime the
renderer decompresses one row and skips every leaf not in it.

### Radiance: lighting

Every lit face carries a grid of luxels. For each, find where it sits in the
world and ask every light whether it can see it, then bounce.

Two details do most of the work of making it look right. Grid points near the
edge of an angled face fall inside neighbouring geometry — left alone they bake
black and produce a dark rim around every surface, so they are pulled toward
the face centre until they clear. And shadow rays start slightly off the
surface, because starting exactly on it means the first thing every ray hits is
the face it came from.

## Runtime

### The tick

Fixed at 64 Hz, decoupled from rendering. A 240 Hz display draws 240 smooth
frames of a 64 Hz simulation, not 240 simulations. The catch-up accumulator is
capped so that a stall — a breakpoint, a window drag — does not produce a burst
of hundreds of ticks that looks like the world fast-forwarding.

Each tick:

1. Categorise the player's position: are they standing on something?
2. Friction, then acceleration, then the move itself. That order matters:
   running friction after acceleration makes ground movement mushy and breaks
   air strafing entirely.
3. Update triggers from the player's box.
4. Run the entity queue: deliver due events, run thinks, reclaim removals.

### Collision

Traces work against **brushes**, not triangles. A brush is a handful of planes,
so testing one is a handful of dot products regardless of how detailed its
surface is, and the BSP narrows the search to the brushes actually along the
path.

Brush entities — doors, platforms — are separate models kept out of the world
tree so they can move without re-splitting it. They are traced separately and
the nearest hit taken. Forgetting that is how you walk through every door in
the game.

### Rendering

What to draw is decided long before the GPU is involved:

1. Find the viewer's cluster.
2. Take its PVS row — the clusters that can possibly be seen.
3. Frustum-cull the leaves in those clusters, then their surfaces.
4. Draw what is left, batched by material.

The PVS and the frustum do different jobs and neither subsumes the other: the
PVS removes rooms you cannot see through any opening, the frustum removes what
is behind you.

**Brush entities are a second pass, and have to be.** A door, a lift, anything
tied to a class is compiled as its own model with its own leaves, and those
leaves are not in the world's PVS. A leaf walk therefore finds the world and
nothing else — which is exactly what happened: every brush entity in every map
was built into the mesh and never drawn, and the sample map's door was simply
not there. They are drawn after the world, one model at a time, culled against
the frustum by their bounds rather than by the PVS. There are a handful of
them, they are the things the player walks up to, and culling one wrongly
costs far more than drawing it.

Each carries a displacement — where its entity has moved to — applied in the
vertex shader through a uniform with a dynamic offset. That displacement comes
from the same fields the collision code traces against, so what you see and
what you walk into are the same thing by construction.

Lightmaps pack into one atlas, so the world draws in as many calls as it has
materials rather than one per face.

### Entities

Entities are a bag of named fields rather than typed structs, because the
meaningful fields belong to the *game*, not the engine. Classes are registered
handlers — the same split Source draws between its engine and its game DLL.
`kerosene-entity` knows how to route an input; `kerosene-game` decides what `Open`
means.

Outputs become queued events even at zero delay, so an entity firing at itself
cannot recurse into the stack, and ordering does not change when a delay is
added. Handles carry a generation, so an event naming an entity that died
before it fired resolves to nothing rather than to whoever took its slot.

## Testing

Where a subsystem can be tested without a GPU or a window, it is:

- **Movement** runs against a hand-built box world — a floor, a step, a wall —
  rather than a compiled map, so a failing test is a movement bug rather than a
  map bug.
- **Shaders** are validated through `naga`, the same compiler wgpu uses, so a
  typo fails in CI rather than at pipeline creation on a machine with a display.
- **The whole pipeline** is exercised by tests that build a `.keromap` in memory,
  compile it through Cleave, load the result and play it. Every crate can pass
  its own tests and still not add up to a level you can walk around; that suite
  is where the seams show.
