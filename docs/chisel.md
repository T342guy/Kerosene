<!-- SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0 -->
# Chisel

The world editor, inside `kerosene-tools`. Open the toolset, pick Chisel off the
rail, and you have four viewports, a material browser, an entity properties
panel and a compile log — enough to draw a room, texture it, put a player start
in it, press F9 and be standing in it.

That closing loop is the point. Anything short of it is a viewer.

```sh
./build/debug/bin/kerosene-tools content/maps/kero_start.kmap
```

A map named on the command line is opened at startup; without one, Chisel starts
empty and **File → Open map** lists what the project has.

---

## The window

| Pane | What it is |
|---|---|
| Top, Front, Side | Orthographic. No foreshortening, which is what makes building on a grid possible. |
| 3D | Perspective, textured. For judging the result, not for placing anything. |
| Map | Tool, grid, undo, and what is selected. |
| Materials | The project's `.kmat` files, with previews. |
| Properties | The selected entity's keys and its I/O wiring, and the selected brushes' type. |
| Compile | Cleave and Umbra, with a clickable log. |

The layout is built the first time Chisel opens and remembered after that — move
a splitter and it stays moved, because a layout you have arranged and a layout
the program insists on are not the same thing.

## Getting about

| | Orthographic | 3D |
|---|---|---|
| Look | — | hold right mouse |
| Move | middle-drag, or right-drag | `W` `A` `S` `D`, `Q` down, `E` up; hold `Shift` for three times the speed |
| Zoom | wheel — the world point under the cursor stays put | wheel dollies forward |

**Frame all** in the Map panel puts every brush in view, which is the fastest
way back when you have flown somewhere you did not mean to.

## The tools

**Select.** Click a brush to select it, `Ctrl`- or `Shift`-click to add to the
selection, `Escape` to drop it. Drag a selected brush to move it on the grid.
Eight grips sit around the selection in the orthographic views: the four corner
grips resize two axes, the four edge grips resize one. Nothing moves along the
axis a view looks down — an orthographic view cannot show that axis, and a drag
that changed it would move brushes in a direction you cannot see.

**Block.** Drag out a box in any viewport. The pane you draw in decides which
way it stands: the depth is taken along the axis that view looks down, centred
on the depth you were already looking at, and **Depth** in the Map panel sets how
much of it there is.

**Entity.** Click to place a point entity of the class chosen in the Map panel.

Everything goes through the undo stack — there is no path that changes the map
directly, which is what keeps undo honest. `Ctrl+Z` and `Ctrl+Shift+Z`, or the
buttons in the Map panel. `Delete` removes what is selected.

Nothing reaches the map until the mouse button comes up. What moves under the
cursor during a drag is an overlay; the edit is the difference between where the
brushes were and where they were let go, so one gesture is one undo step however
many brushes it moved.

### Brushes are planes

A brush is stored as a list of planes, not a list of vertices, and every
transform moves the planes. A box stays a box because its half-spaces stay
half-spaces: there is no arrangement of drags that can make a brush non-convex,
so there is no validation pass that has to catch one. A resize that would turn a
brush inside out is refused rather than accepted, because refusing the last unit
of a drag is far kinder than letting the compiler explain it three stages later.

The same property is what makes picking exact. A click casts a ray and clips its
parameter range by each of a brush's planes in turn — the slab method — so it
works identically in the orthographic views, where there is no perspective ray
to cast, and the plane the ray entered through *is* the face that was hit.

## Materials

The browser lists every `.kmat` under `content/materials`, by the name a `.kmap`
refers to them by — `dev/wall`, not a path. Search filters the list; clicking a
preview chooses it; **Apply to face** paints the picked face of a single
selected brush and **Apply to brush** paints all of them.

Previews come from the same generator the engine uses, so a surface looks the
same in the browser, in the viewport and in the game. There is no texture
compiler yet, so every material is a tinted developer grid — see the
[status section](../README.md#status).

Texture axes are world-space projections rather than per-vertex coordinates.
A wall that is made wider therefore shows *more* texture rather than the same
texture stretched, which is what a person resizing a room wants. Painting a face
realigns its axes only if nobody has aligned them by hand: a shift somebody
typed is work, and repainting must not undo it.

What a material will compile to — solid, a trigger, drawn or not — is shown
under the chosen name. It comes from the compiler's own table
(`bsp::describe_material`), so the editor cannot come to disagree with what
actually happens.

## Entities

The properties panel shows **every key as written**, including ones this build
does not implement. There is no class schema to consult — entity classes
register themselves in the engine process, not in the editor — so rather than
guess at a widget per key, every key is text. The rule that matters is that
nothing is dropped: a key the editor has never heard of survives a load, an edit
and a save.

Outputs are grouped by event, so a sequence reads as "when this happens: do
that, then that". Each wire is a target, an input, an optional parameter, a
delay and a fire count, which is Source's output/input system kept as it was
because it is the best idea in that engine — a button's `OnPressed` fires a
door's `Open` after a delay, and it composes far further than it has any right
to. There is no scripting language.

Selected brushes carry a **Type** at the top of the panel: the world itself, or
a brush entity like `func_detail` or `trigger_multiple`. World brushes are what
seal a level; everything else is furniture inside it. Tying brushes to an entity
cleans up any brush entity the move leaves empty.

## Compile and run

`F9` compiles and runs; `F8` compiles only. Both save the map first if it has
moved — what runs has to be what is on disk, or the engine shows a level that
exists nowhere. Cleave and Umbra are called in-process; the engine is then
started from beside the toolset binary, detached, so closing the game does not
close the editor.

**Fast** skips the expensive visibility pass, for a layout that is still moving.
**Geometry only** stops after Cleave, which is enough to see whether the level
compiles and whether it is sealed.

Errors in the log that name a brush are clickable: clicking one selects that
brush and frames it. That is the whole reason to run the compiler from inside
the editor rather than from a terminal.

### Leaks

A level that is open to the void cannot have its visibility computed, because
there is no inside to compute it for. When Cleave finds one it writes a
`.kleak` polyline — the shortest path from the entity that leaked out to the
hole it escaped through — and Chisel draws it in red in every viewport, on top
of everything, until the level compiles sealed and Cleave deletes the file.

Follow the red line. It ends at the hole.

## What is not here

No shape tool (arches, cylinders, stairs), no vertex manipulation, no clipping
tool, no displacements, no prefabs, no 2D texture-alignment view, and one map at
a time. Anything not made of boxes has to be made of several boxes.

The GUI itself is not unit tested. What is tested is everything underneath it —
the document and its undo stack, brush transforms as plane moves, ray picking
against known geometry, the grips, and a round trip that loads a `.kmap`, edits
it, undoes the edit and saves it back byte for byte. `tests/content_test.cpp`
goes further and builds a room out of the editor's own primitives, compiles it,
and checks it seals; a second case leaves a wall out and follows the leak line
through the hole.
