# Architecture

## The shape of the thing

```
   .kmap ──cleave──► .kbsp ──umbra──► +vis ──radiance──► +light
                       │
                       └──────────────────────────────► kerosene
```

Three properties do most of the work, and each is a decision rather than an
accident.

**Everything expensive happens at build time.** Visibility and lighting are
computed once, by separate programs, and written into the level. The engine
loads what the tools produced. This is why the compilers are allowed to be slow
and why the runtime can afford to be simple.

**The tools are separate from the engine.** Not by convention — by a build rule.
`cmake/KeroseneTargets.cmake` fails configuration if an engine library reaches
into the toolset, naming the offending edge. Source's real design achievement
was this separation, and it is one careless `#include` from being lost.

**The simulation does not know whether there is a display.** `kerosene::engine`
does not link `kerosene::render`. That makes `--headless` the dedicated server
rather than a mode bolted on, lets an end-to-end playthrough run in CI on a
machine with no GPU, and is the precondition for client prediction later. Every
engine that adds this afterwards finds the renderer has grown into the
simulation.

## The libraries

Dependency order, bottom up. Nothing depends on anything below it in this list.

| Library | What it owns |
|---|---|
| `core` | types, assertions, the log, an arena allocator, the job system |
| `math` | vectors, planes, convex windings, the unit scale, the epsilon policy |
| `kv` | the KeyValues text format, with line-and-column diagnostics |
| `console` | convars, concommands, the command buffer |
| `map` | `.kmap` |
| `bsp` | `.kbsp` — the format, the loader, tracing, PVS decoding, the material table |
| `physics` | player movement and collision response |
| `entity` | entities, their fields, the I/O event queue |
| `game` | entity classes — the game-DLL analogue |
| `engine` | the host loop |
| `render` | the SDL_GPU renderer |

`tools/cleave` and `tools/umbra` sit above all of them and are reachable from
none of them. So do `tools/shell`, `tools/chisel` and `tools/build`, which are
the toolset window — one application holding every tool, with each tool a Panel
that knows nothing about the others.

The compile stages are libraries, not programs. `kerosene-tools` is a GUI
application with no command line, so anything that wants to compile a map links
`kerosene::cleave` and `kerosene::umbra` and calls them — which is what the
Build panel does, what Chisel's F9 does, and what the test fixture does.

### Two of these are worth explaining

**`core`'s job system exists before anything needs it.** Source's compilers
thread their innermost loops and leave the rest serial, which is why `vvis` on a
large level idles most of a modern machine. Every expensive stage here is
written against the scheduler from the first line. Two properties make it safe
to rely on: a thread waiting on a group *runs jobs while it waits*, so recursive
fork/join cannot deadlock the pool; and a job's exception is re-thrown from
`wait()` on the waiting thread, so a compile stage can report which brush was at
fault instead of the process disappearing.

**`bsp` owns the material table, not `cleave`.** What a brush *does* comes from
the material on its faces — `tools/clip` blocks players, `tools/trigger` is not
solid at all. Putting that table in an engine library means the editor showing a
designer "blocks players only" and the compiler deciding to emit a clip brush
are reading the same rows and cannot disagree. A tooltip that lies about what a
brush will compile to is worse than no tooltip.

## Precision

The compile stages instantiate the geometry on `double`; the runtime
instantiates the same code on `float`. `Vec3T`, `PlaneT` and `WindingT` are
templated on the scalar so there is one implementation and no chance of two
copies drifting apart — which is the failure mode of every codebase that keeps a
`vec_t` typedef and flips it.

Source runs CSG in single precision and pays for it in microscopic slivers and
phantom leaks. Doubles cost nothing at build time, where the whole point is to be
slow and right.

Every tolerance lives in `src/math/scalar.hpp`, named for what it means
(`kPointOnPlane`, `kDegenerateArea`, `kPlaneDedup`) and chosen per scalar type.
`vbsp` spreads its tolerances across the files that use them, several spelled
`0.1` inline; it is then impossible to tell, at a given comparison, which
tolerance is meant or what would break if it moved.

## Cleave

Four passes.

1. **Brushes from planes.** Each side's base winding is clipped by every other
   side's half-space. A side left with nothing does not bound the solid, which
   is ordinary rather than an error.
2. **CSG.** The parts of each face buried inside another brush are cut away,
   leaving the *surface* of the union. The brushes themselves are left whole —
   they are what collision uses.
3. **The tree.** Recursive splitting on brush planes, scoring cuts against
   balance with a large bonus for axial planes and an overwhelming one for hint
   planes.
4. **Portals and the flood fill.** Every non-solid boundary becomes a portal;
   the flood runs outward from every point entity; anything it never reached is
   filled in as solid.

### The two subtleties worth knowing about

**Coplanar faces are two different cases.** Two brushes pushed flush against
each other share an *interior* wall, which must be removed from both. Two
brushes whose tops are level share an *exposed* surface, which exactly one must
keep. Because the plane set stores planes in facing pairs, telling them apart is
an index comparison: same index is same facing, `index ^ 1` is opposite. Get it
wrong and you either lose a wall or draw one twice and light it twice.

**The split heuristic tests the node's region, not the brushes in it.** A
brush-count test looks equivalent and is not: with one brush left in a room — a
step, a pillar — every one of its own faces has the brush entirely on one side,
so every candidate is rejected, the node becomes a leaf with the brush still in
it, and the whole room compiles as solid.

### Leaks

The flood is breadth-first, so the path recorded is the shortest way out. Cleave
names the entity that leaked and writes the path to `.kleak` as a polyline.
Source tells you a leak exists and leaves you to find it; naming the entity and
the path turns a hunt into a fix. A stale `.kleak` is deleted when the level
compiles clean, because a file saying the level is broken when it is not is
worse than no file.

## Umbra

Portal flow, exact rather than conservative. Sight is traced through chains of
openings, clipping the sight-lines against the *separating planes* between each
pair — so a corridor that turns twice really does occlude, which a conservative
method cannot discover.

Base vis (do two portals face each other at all?) prunes the flow, which is what
makes the flow affordable. It over-reports, and that is the safe direction: too
large a PVS costs frame rate, too small a one puts holes in the world.

One rule that is easy to get wrong: **the flow must not mark a cluster visible
merely because it is adjacent.** The shortcut looks harmless and makes the
answer asymmetric, because the pruning sets run out at different depths in the
two directions. Two clusters either have a sight line between them or they do
not.

## The runtime

### Tracing

The trace takes an **arbitrary box**. Quake and Source precompute collision
hulls at a few fixed sizes and snap every entity to the nearest, which
constrains what an entity can be for the rest of the engine's life. Cleave adds
bevel planes at compile time — exactly the supporting planes of the Minkowski
sum — so pushing each brush plane out by the box's extent along its normal gives
an exact sweep at any size.

The box is centred by shifting the sweep rather than by symmetrising the box: a
player's box runs from its feet to its head, and treating that as symmetric
would have it collide with the floor above its own head.

Sweeps stop a hair short of what they hit. Landing exactly on a plane leaves the
mover touching the wall, and the next tick's start-solid test then depends on
the last bit of a float.

### Movement

Reproduced from the Quake lineage, air-speed cap included. In the air,
acceleration is applied against a wish speed clamped to a small constant, so
aiming where you are already going adds nothing while aiming across your motion
adds velocity. That asymmetry is bunny-hopping and surfing. It looks exactly
like a bug until you notice removing it changes what the game is.

The constants are re-derived for the two-inch unit rather than halved blindly:
speeds and accelerations are lengths per unit time and halve; friction and the
acceleration coefficients are dimensionless and do not.

Gravity is integrated in two halves, before and after the move, so a jump's
height does not depend on the tick rate.

### The tick

Fixed at 66 Hz and decoupled from the frame. A variable timestep makes a jump's
height depend on how fast your machine is and makes a recorded input sequence
unreproducible — and reproducibility is the precondition for client prediction,
which is why the tick is fixed now rather than when networking demands it. The
view interpolates between the last two ticks, so the picture is smooth even
though the simulation is not continuous.

A frame that stalls drops its backlog rather than catching up. Catching up makes
the next frame longer still and the one after that longer again.

### Entity I/O

No scripting language. A trigger's `OnStartTouch` fires a relay, the relay fires
three things at once, a `logic_branch` supplies the "otherwise". What that buys
over a script is that the graph is *data*: it can be drawn, diffed and validated
without running it. Wires that name nothing are reported at load, because a
designer wants to know then rather than when the button turns out to do nothing.

Events are delivered in batches per tick, so an input that queues another does
not deliver it within the same tick. A relay wired to itself is then a slow loop
you can see rather than a frozen frame — a rule instead of an arbitrary
iteration limit.

### Rendering

Culling is PVS first, then frustum: the expensive question was answered once at
build time, the cheap one is asked per frame. Faces are sorted by material so
adjacent visible ones merge into a single draw.

Shaders are compiled to SPIR-V at build time and embedded. The runtime never
needs a shader compiler and never ships one; a shader error is a build error
found by whoever changed the shader.
