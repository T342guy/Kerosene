# Positioning

What this engine is good at, what it is not, and which games it is shaped to
make. A companion to [`missing-features.md`](missing-features.md): that sheet
says what is absent, this one says which of those absences actually matter.

> Game titles here are named for one reason: to point at a shape of game, the
> same way the README names Valve and id tools to point at a shape of tool.
> None of their makers are affiliated with, endorse or sponsor Kerosene, and
> none of their code or content is in it. See [`NOTICE`](../NOTICE).

## The comparison people reach for

The question that prompted this document was whether Kerosene could compete
with Unity. The answer is no, and pursuing it would be the fastest way to kill
the project.

Unity is roughly two decades and thousands of engineer-years. Matching it means
a shader graph, an animation state machine, terrain, particles, LOD, five
platform backends, an asset store, and console certification. That is not a
roadmap; it is a company. Kerosene is around 73,000 lines across eighteen
crates and ten tools.

But "compete with Unity" is the wrong frame, and the architecture already says
so. The README's thesis -- the tools are separate from the engine and share
open formats -- is something Unity structurally cannot do, because its editor
*is* the product and its runtime is downstream of it. Kerosene's compilers are
subcommands. You can script them, run them on a build server, replace one, or
write your own. That is a real advantage and a permanent one, and it is worth
more than feature parity in a race that cannot be won.

The useful question is not "how do we catch up" but "what is this shaped to be
best at".

## The Titanfall argument

The strongest case for this architecture is that it has already been made, by
someone shipping a game on it.

Respawn built Titanfall on a branch of Source 2009. They did not take it for
its renderer, most of which they replaced, or for its tools. They took it for
two things: **the movement solver and the brush pipeline**.

Everything that makes that game feel the way it does -- wallrunning, sliding,
carried momentum -- is built on top of the structure in
[`kerosene-physics`](../crates/kerosene-physics/src/movement.rs):
`accelerate`, `air_accelerate`, `clip_velocity`, `try_move`, `step_move`. That
is the Quake-to-Source lineage, and it is why air-strafing works at all. It is
also five readable functions with unit tests, rather than a character
controller written for someone else's game.

The other half is the pipeline. Those levels are still convex solids compiled
to a BSP tree with a precomputed PVS, because for a shooter that has to hold
sixty frames a second, nothing beats the level telling you what you can see.
That is Cleave, Umbra and Radiance.

So: Kerosene is already good at the two things that mattered enough to build a
studio's flagship on. What it does not have is everything that was built on
top of them with a large team.

## Four things it is structurally good at

**Movement feel.** The solver is legible and tested. Tuning air acceleration is
editing a function, not fighting an abstraction.

**Entity I/O.** [`kerosene-entity/src/io.rs`](../crates/kerosene-entity/src/io.rs)
has connections with delays, parameters and fire-once semantics. A designer
wires an entire set-piece without writing code, and the result is a text file
that diffs and reviews. This is an underrated design, and the mainstream
engines' answer -- callbacks serialised into a scene blob -- is worse.

**Iteration speed.** Brushes compile in seconds. That is a different design
loop from sculpting in a DCC package and importing.

**Determinism.** A headless simulation, a unit scale that lands on powers of
two, and no garbage collector. This is more valuable than it sounds; see the
time-trial entry below.

## Five games it is shaped to make

Each entry names the shape, what it leans on, and what it is still missing.

### 1. A movement shooter

*Titanfall 2*, *Ultrakill*, *Neon White*, *Severed Steel*.

These live or die on the solver, and Neon White in particular shows the market
does not want photorealism -- it wants frame-perfect movement and levels that
read at a glance.

**Leans on:** the movement solver, fast compiles.
**Needs:** weapons, and skeletal animation for viewmodels. Very little else.
**This is the shortest path from here to a shipped game.**

### 2. An immersive sim or boomer shooter

*Half-Life 2*, *Dishonored*, *Prey*, *Deathloop*.

All brush-built, all I/O-driven. Physics props plus entity I/O plus Rhai hooks
is most of the substrate for "the player solves this room however they like".

**Leans on:** entity I/O, the physics sandbox, scripting.
**Needs:** weapons, damage, and eventually NPCs.
**The best fit for what actually exists today**, because it leans on the I/O
system and the physics rather than on animation.

### 3. A time-trial or speedrunning game

*Neon White*, *Clustertruck*, the *Trackmania* design space.

Deterministic movement, fast level compiles, ghost replays. Determinism is the
*feature* here rather than an implementation detail: fixed-timestep physics in
the mainstream engines is not replay-stable across machines, and people build
elaborate workarounds. Kerosene gets it as a consequence of how it is built.

**Leans on:** determinism, the solver, iteration speed.
**Needs:** save/load for ghosts, and a HUD.

### 4. Competitive multiplayer with a mod scene

*Counter-Strike*, *Team Fortress 2*.

This is where the open-formats thesis pays off hardest, because a community
that can write its own tools makes a game outlive its studio.

**Leans on:** replaceable compilers, open formats, the headless simulation.
**Needs:** the whole of the networking section. Furthest out -- but the
headless split means the architecture is already pointed the right way.

### 5. Horror

*Cry of Fear* and the lo-fi space around it.

Baked radiosity in tight brush geometry is an aesthetic, not a compromise, and
this genre has no appetite for physically-based shading.

**Leans on:** Radiance, the brush pipeline.
**Needs:** audio occlusion and reverb -- sound doing the work graphics does
elsewhere.

## The pattern, and what it rules out

Every one of those is **geometry-heavy and animation-light**. That is not a
coincidence: it is exactly where the missing-features sheet is strongest and
weakest.

The genres to stay away from are the ones the mainstream engines are actually
good at -- anything built on characters, dialogue, cutscenes, open worlds, or
a large art budget. Not because they are impossible, but because they are a
fight against every strength listed above.

## What gates all of it

Three items on the sheet stand between "an impressive engine" and "somebody
shipped a game with this":

1. **Skeletal animation.** `.keromdl` stores bones and weights and nothing
   plays them. No characters means no NPCs and no enemies -- a sandbox rather
   than a game. This is the single biggest blocker on the whole sheet.
2. **Networking.** The headless simulation is the hard architectural half, and
   it is done. Without a wire protocol, though, the comparison to Source stops
   being true in the way that mattered most to Source.
3. **Windows.** Audio is ALSA-only, so most of the people who would want this
   cannot run it.

Everything else on that sheet -- PBR, post-processing, decals, particles, LOD
-- makes games *prettier*. These three make games *possible*.

## The honest summary

Kerosene will not be a general-purpose Unity competitor. It could plausibly
become the engine someone reaches for when they want to build a Half-Life
2-shaped game and find that nothing else lets them. That is a smaller market
and a far more winnable one.
