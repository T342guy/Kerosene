# Tool reference

One application, `kerosene-tools`. Open it with no arguments and you get one
window holding every tool: a project page, the world editor, the sound editor,
a build form and an archive form, switched with an activity bar of icons down
the left edge (`ctrl-1` to `ctrl-5`), and one **output panel** along the
bottom (`` ctrl-` ``) that every job -- a compile, a build, a pack -- logs
into. It comes up on its own when a job starts. None of it is the engine, and
none of it depends on it.

The **project page** is where the window opens: which project this is, where
its content is and how that was decided, how many maps, materials, models,
sounds and scripts it holds, and the maps themselves with whether each has
been compiled. Click a map to edit it; *new map*, *build everything* and
*pack archive* are the three buttons.

The same stages also run headless, as subcommands, so a script or build server
can drive them without a screen: `kerosene-tools cleave map.keromap`, and so
on. Each subcommand is the program it used to be, unchanged in argument and
output.

---

## init — starting a project

```sh
kerosene-tools init [dir] [--name "My Mod"] [--content content]
```

Writes a `.keroproj` and creates the content tree beside it: `maps`,
`materials`, `art`, `textures`, `models`, `sound`, `scripts`. Without a
`--name` the directory's own name is used, and the project file is named after
it.

It is safe to run on a directory that is already a project. The project file is
the one thing here somebody is expected to have edited, so an existing one is
left exactly as it is, and only missing directories are created — which makes
this the way to fill in a directory the engine has started using since the tree
was made.

A project may name its own directories instead; see
[formats.md](formats.md#the-content-tree-is-created-not-required).

Nothing *requires* this command: the engine and the toolset both create the
tree on their own the first time they find a content root. It exists so that
starting a project is a thing you do, rather than a thing you discover you
should have done.

---

## Chisel — the world editor

A tab in the toolset window, and openable straight to a map:

```sh
kerosene-tools                 # the toolset window, on the project page
kerosene-tools chisel [map.keromap] [--content <dir>]   # straight to the editor
```

**Finding the content.** Chisel needs the content root -- the tree holding
`maps/`, `materials/` and the `.kerodef` class definitions -- to show entity
classes and materials at all. The search lives in `kerosene-vfs` and every tool
and the engine share it, so they cannot disagree about which tree is in use.

The reliable way to settle it is a **project file**: a `.keroproj` at the top
of a project naming its content directory. See [formats](formats.md#keroproj).
Without one, the tree is inferred, which works and is why a fresh clone needs
no setup -- but inference is a guess, and a project file is how you overrule
it. Three places are searched, nearest first: the tree the map lives in, the
working directory, and the directory the executable is in. In each of them a
project file wins over a guess, even a guess found closer down; between them,
nearness decides. So a map sitting in a content tree of its own is not claimed
by a project on the far side of the disk.

Failing a project file, each place is searched like this: `--content` if given,
then beside the map being opened, then the working directory, then beside its
own executable, climbing up to six levels from each looking for a directory
holding `kerosene.kerodef` (or, failing that, both `maps/` and `materials/`).
Opening a map from anywhere in a project therefore just works, and the map's
own tree wins over the working directory on purpose -- editing another
project's map should not show this project's entities.

The status bar says what it found: `20 classes, 41 materials` when the content
is there, `no entity classes` in red when it is not, and `n materials unbuilt`
in amber when a material has no texture behind it. If the first is red, nothing
in the editor will look right, and `kerosene-tools chisel --help` lists the search order.
`cargo run -p kerosene-chisel --example diagnose` prints the same thing without opening
a window -- discovery, classes, materials, which materials have no texture, and
which maps have never been compiled -- which is the fastest way to answer "why
does Chisel show no entities". It changes nothing unless given `--build`.

**Textures are built on the way in.** Before the editor finishes loading it
runs the same texture build Alchemy's `build` command does: the developer set
is regenerated, then every image under `art/` is compiled into `materials/`.
Anything already compiled is skipped, so the cost after the first run is a
directory walk. It happens *before* the materials are scanned, because scanning
first and building second is an editor with no textures in it and no way to
tell. `--no-build` turns it off; `F9` does it again before compiling the map,
so a texture added during a session is compiled before the map that uses it.

**Files.** `ctrl-S` saves. A map that has never been saved is asked for a name
first rather than being written to `untitled.keromap` somewhere -- the name is
what `kerosene +map <name>` loads, so an editor that picks one for you is an editor
whose output you have to go looking for. `ctrl-shift-S` and `file → save as…`
ask for a name outright. A bare name means a map in this project: typing
`arena` writes `<content>/maps/arena.keromap`. An absolute path is taken as
given.

`file → rename…` moves the map *and* the artefacts compiled from it -- the
`.kerobsp`, `.keroprt` and `.keroleak`. Leaving a `.kerobsp` behind under the
old name is worse than clutter: the game still loads it, so a renamed map
appears to work under a name that no longer exists and to be missing under the
one that does. Renaming onto a map that already exists is refused.

`file → open` lists the maps in the project. Anything that would throw away
unsaved changes asks first, and offers to save. The title bar and the status
bar both name the file, with a `*` when there are unsaved changes; a map with
no file yet says `not saved` rather than showing an invented one.

**The layout is Hammer's.** A strip of tool icons down the left edge -- hover
one for its name and key -- a toolbar row under the menu with the grid size,
snap, how the 3D panes draw, the texture tool's modes and the compile button,
and an inspector on the right with four tabs: **Object** (what is selected),
**Tool** (the current tool's settings: the entity classes with a search box,
the shape sliders), **Materials** (the browser, docked) and **VisGroups**
(what is shown; see below). The tab follows
the work -- a fresh selection brings up Object, picking the entity or shape
tool brings up Tool -- and otherwise stays where it was put. The status bar
along the bottom names the file, the selection's size, the pointer's place in
the world, the grid, and what content was found.

Four panes fill the middle, each showing whichever view you point it at: 3D,
or any of the six flat views -- top, bottom, front, back, left and right.
Brush geometry is axis-aligned far more often than not and an orthographic
view along an axis is the only way to place a vertex exactly without typing
numbers. Each pane has a header: its view is a menu there, its zoom or fly
speed is written beside it, and the button at the right end (or a
double-click on the header, or `shift-space`) makes it the only pane. Drag the
bars between the panes to resize them.

| Key | |
|---|---|
| `1` `2` `3` `4` `5` `6` | select, block, entity, texture, shape, clip tool |
| `M` | the material browser, as a window |
| `Alt+Enter` | object properties, as a window |
| `Ctrl+Shift+E` | the entity report |
| `Ctrl+G` / `Ctrl+U` | group / ungroup the selection |
| `H` / `Ctrl+H` / `U` | hide the selection / hide everything else / unhide all |
| `Ctrl+Shift+G` | a new visgroup of the selection |
| `Enter` (clip tool) | cut along the laid line; `6` again cycles what is kept |
| `Ctrl+Shift+C` / `Ctrl+Shift+H` | carve / hollow |
| `Ctrl+M` | transform: rotate, scale or move by numbers |
| `R` | rotate 90 degrees about the axis the pane looks along |
| `Ctrl+L` / `Ctrl+I` | flip horizontally / vertically |
| `Ctrl+B` | align the selection to the grid |
| `Shift+Space` | maximise the active pane, or show four again |
| `[` `]` | finer / coarser grid |
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo |
| `Ctrl+S` | save (asks for a name the first time) |
| `Ctrl+Shift+S` | save as |
| `Delete` | delete selection |
| `Escape` | clear selection, cancel a drag |
| `F9` | compile (fast) and run |
| **In a 3D pane** | |
| `W` `A` `S` `D` | fly forward, left, back, right |
| `Q` `E` | fly straight down and up, whichever way you are looking |
| `Shift` / `Alt` | fly 2.5x faster / 4x slower |
| `Ctrl`+scroll | set the fly speed |
| Right-drag | look around |
| Middle-drag | slide the camera sideways and up |
| **In a 2D pane** | |
| Middle-drag | pan |
| Scroll | zoom about the pointer |
| Shift-click | add to or remove from the selection |
| Drag a grip | resize the selection |

Keys only reach the pane the pointer is over, and none of them fire while a
property field has the keyboard -- naming an entity `wasd_door` should not fly
the camera across the level.

**Seeing what things are.** Point entities are drawn as what they are: a lamp
for a light, a figure for the player start, a speaker for a sound, a diamond
for logic, a crate for a prop. A room full of identical squares tells you where
things are and not what they are, and the label beside each one was a wall of
text you had to read to navigate. The label now shows the entity's *name* when
it has one, which is what the wiring refers to and what you are actually
looking for. The 3D pane marks them in the same colours.

Shapes are drawn rather than loaded from files: an icon set is a set of things
to ship, scale, theme and keep in step with the class list, and a dozen lines
of geometry is none of those. Classes are matched by prefix, so a game that
adds `light_dynamic` gets the right icon without anything here changing.

**The asset browser** is the inspector's *Materials* tab, and also a window
(`M`, or `view → browse materials...`) when a property field wants a model or
the tab is not enough room. Names, folders, a size slider and a search that
matches words in any order — so `wood crate` and `crate wood` both find
`props/crate_wood`. Materials show what each one does on hover, read from
Cleave's table; models show a rendered preview, because a name is not a shape
and `crate_wood` tells you nothing about whether it is the crate you want.
Clicking a material with something selected applies it.

The old picker was a two-column strip of unlabelled 48-pixel swatches in a
120-point panel, which is a keyhole rather than a browser. Worse, every swatch
was drawn from a mip two from the end of the chain — a 2x2 image for a
256-pixel texture — so every material in the list was the same grey smudge and
the only way to tell two apart was to hover both. Swatches are built from
the smallest mip that is still bigger than the swatch, and a checkerboard
looks like a checkerboard.

**Resizing.** Something selected in a 2D pane wears eight grips: four corners
and four edge midpoints. Drag a corner to scale both axes at once, an edge to
scale one; the opposite grip holds still, so the selection grows away from
where you are pulling rather than wandering across the level. The axis the
pane cannot see is left alone. Dragging a grip past the far side stops at one
grid square instead of turning the brush inside out — an inverted brush is not
a small brush, it is a hole in the world that compiles cleanly. The preview
shows the shape it will become and its new size while you drag. The texture
stays put in world space rather than stretching, so making a wall twice as
wide tiles the bricks twice instead of drawing bricks twice the size.

**Key-values on anything.** Every object in a map -- an entity, a brush, a
face -- carries key-values, and the Object tab edits whatever is selected.
An entity shows every key its class reads, set or not, with a widget for
each (a colour picker for a colour, a dropdown for a choice, checkboxes for
flags); a brush or a face shows the keys it carries, and `+ add a key` puts
any key at all on any of them. The `{ }` toggle is Hammer's SmartEdit
switched off: every key as plain text, only the keys the object actually
has. Selecting several things edits them together: a key they agree on
shows its value, a key they disagree on says *differs* and is left alone on
every one of them unless you type over it, which is how one speed lands on
six doors at once. With nothing selected the tab shows the world's own keys
(the sky, the level title). `Alt+Enter` opens the same editor as a window,
which is where the add-a-key row and the wiring editor live when there is
no room in the tab. A brush's keys mean nothing to the game -- a brush that
should do something is tied to an entity -- but two of them mean something
to the compiler: `detail 1` keeps a brush out of the vis tree, `section
<name>` puts it in a streamed section. Under the keys sit the editor's own
notes: a colour to draw the object in, and comments.

**Groups and VisGroups.** `Ctrl+G` groups the selection, and from then on
a click on any member takes the whole group -- unless the toolbar's
*select whole groups* toggle is off, which is how you nudge one thing in a
group. `Ctrl+U` ungroups. VisGroups are named sets you can hide together:
select the things that make up a room and press `Ctrl+Shift+G`, name it,
and the VisGroups tab has a checkbox that makes the room vanish from every
pane -- not drawn, not clickable, not in select-all -- and a colour every
member is drawn in while it is shown. VisGroups nest (right-click one for
*new child*), and a hidden parent hides its children. Below the user's
groups the tab lists the automatic ones, made from what the map holds:
every entity, every world brush, every tool brush, every class that is
placed. Untick *light* and the lights are gone until you tick it back.
`H` hides the selection outright, `Ctrl+H` hides everything but it, and `U`
brings back whatever was hidden that way. The status bar counts what is
hidden, because a hidden brush that is still there is the classic way to
compile something you cannot see. Hiding is an edit: `Ctrl+Z` undoes it.

A visgroup marked with the stack icon is a **streamed section**: the
engine loads and unloads its geometry around the player. See
[architecture](architecture.md#streamed-sections) for what that does and
does not do.

**The cordon.** The box icon on the toolbar turns the cordon on: only what
is inside the box is shown, and only what is inside is compiled -- Cleave
seals the box with its own walls, so a corner of a large map compiles and
runs on its own in seconds. The arrows icon beside it puts the box's grips
in place of the selection's so it can be dragged to size in a 2D pane. The
box is drawn dashed in red, dimmer when it is off, and it is saved with the
map.

**Clip, carve and hollow.** The clip tool (`6`) is how a wall gets a
doorway or a slab gets a bevel: drag a line across the selection in a 2D
pane, and the cut is the plane through that line running along the axis
the pane looks down. An arrow on the line shows which side is *front*;
`6` again cycles between keeping both halves, the front or the back, and
`Enter` cuts. The new face wears the material of the face most nearly
facing the same way, aligned as that one is. **Carve** (`Ctrl+Shift+C`)
takes the selected brushes out of every world brush they overlap and then
deletes them -- a doorway through a wall in one step, at the cost of the
wall becoming four brushes. **Hollow** (`Ctrl+Shift+H`) turns a brush into
walls of a given thickness, mitred at the corners so no two overlap; a
negative thickness builds the walls outward around it. A room is a hollowed
box. All three keep a brush's keys and visgroups on every piece.

**Meshes.** `Tools → Convert to mesh` turns the selected world brushes into
polygon meshes: same shape, same materials and alignment, drawn in their own
colour. A mesh is detail -- drawn, lit and collided with, but it no longer
seals the map or blocks visibility, so converting a wall that seals the map
makes it leak and the compile says so. Meshes select, move, resize,
duplicate, delete, hide and take a material like brushes; editing their
vertices is not in Chisel yet. See `.keromap` in `formats.md`.

**Transform.** `Ctrl+M` rotates, scales or moves the selection by numbers,
about its centre or the world origin; `R` is a quarter turn about the axis
the active pane looks along; `Ctrl+L` and `Ctrl+I` flip it; `Ctrl+B` moves
it so its lowest corner sits on the grid. Rotation turns the texture with
the brush so a surface keeps its texel, the way moving does.

**The entity report** (`Ctrl+Shift+E`) lists every entity in the map: filter
by class or name, point or brush, click one to select and frame it in every
pane, double-click for its properties. It marks every output aimed at a name
no entity has, which is otherwise found by playing the map and wondering
why the door did not open. `Edit → History...` is the undo stack with names
on it; click a step to go back to it.

**Shapes that are not boxes.** A brush is a convex solid and no convex solid
is curved, so an archway cannot be one brush. It is several, arranged to read
as a curve — which is miserable to do by hand and is why people give up on
curves. The **shape** tool (`5`) generates them: drag a box in a 2D pane the
way you would with the block tool, and it fills it.

| Shape | |
|---|---|
| wedge | A ramp: a box with one top edge pulled down to the floor. One brush. |
| cylinder | A pillar or a pipe. One brush however many sides — a convex polygon swept along a line is still convex. |
| cone | A spike or a pyramid. One brush. |
| arch | A doorway, a tunnel mouth, a round window: a fan of brushes, one per segment. |
| stairs | Solid steps, one brush each. Solid rather than hollow, because a player falls through thin treads when a physics tick lands between two of them. |

The pane you draw in decides which way the shape stands: a cylinder drawn from
above is a pillar, the same drag in the front view is a pipe lying across the
room. Sides, arc and wall thickness are on the Tool tab, and only the ones the
chosen shape uses are shown. The preview draws the actual shape and the number
of brushes it will cost, not the box it is being fitted into. A whole arch is
one undo step.

`cargo run -p kerosene-chisel --example shape_sheet -- shapes.png` renders every shape
in both orientations. Geometry has a way of being valid and still wrong; a
test can say the brushes are solid, only a picture can say they are an arch.

**Brush properties.** A brush's **type** is a setting on it, at the top of its
panel: world geometry, `func_detail`, `func_door`, `trigger_multiple`, and so
on. Choose one and its settings appear underneath, in the same panel, with
nothing to press first. There is no "tie to entity" step — that was a mode
change to reach settings that were never anywhere else, and it left
`func_detail`, which is a wall and has nothing to configure, looking exactly as
configurable as a door.

Changing the type is one operation and one undo step, and it keeps the name and
the wiring: dropping a `targetname` would silently break every output pointing
at it.

**A trigger textures itself.** Pick a `trigger_*` type and its brushes become
`tools/trigger`, so it is invisible and compiles as a region. Forgetting to do
that by hand compiles a solid block where a doorway was meant to be, and the
map looks broken in a way that has nothing to do with triggers. A door keeps
whatever it was textured with, because only a designer knows which door.

The panel also says what the brushes will **compile as**, read from Cleave's own
material table rather than a copy of it, so the editor and the compiler cannot
disagree. It is where tool textures stop being paint: `tools/clip` says "blocks
players only; bullets and sight pass through". Two rules that surprise everybody
are called out on the spot:

- **One tool face changes the whole brush.** Solid is the absence of anything
  more specific, so a single `tools/clip` face on an otherwise ordinary box
  stops the box being a wall.
- **A misspelt tool material is not an error.** `tools/clipp` compiles as
  ordinary world geometry, which is how a doorway gets walled off by a typo
  nobody sees. The panel says so in red.

**Where a door goes.** Select a brush entity that moves and the 2D panes draw
its travel: an arrow along `movedir`, an outline where it ends up, and a label
saying how far and which way. The distance comes from `kerosene_game`'s own
formula, so the picture and the door agree by construction rather than by
luck. Anything with `angles` gets a facing arrow the same way.

**Building a level.** Draw brushes with the block tool in a 2D view; they snap
to the grid, outward, so a brush is never smaller than the rubber band. Pick a
material from the Materials tab — picking one with something selected applies
it. Place entities with the entity tool; its classes are on the Tool tab. Give
brushes a type on the Object tab to make them a door or a trigger. Wire
outputs to inputs in the same panel.

**Wiring, as a sequence.** A `.keromap` stores wiring as a flat list of
connections, which is the right thing to store and the wrong thing to show:
what a designer is building is *when this happens, do these things, in this
order*, and a column of rows with delays in them makes the order something you
reconstruct in your head. The panel groups them by event instead. Under
`OnStartTouch` you get `do Open on gate`, then `then Trigger on siren`, in the
order they will actually fire, and `+ then` adds another step after the last
one — with a delay, because two actions at the same instant fire in whatever
order the file happens to hold.

**Alternatives need an entity that can choose.** Every other class fires a
list; firing one of two lists depending on something is a decision, and a
decision cannot be faked in the editor. That is what `logic_branch` is for: it
remembers a yes or no and fires `OnTrue` or `OnFalse`, never both. When one
side of a pair is wired and the other is not, the panel says so — an `OnTrue`
with nothing on `OnFalse` does nothing half the time, which is a bug you find
by playing rather than by reading.

Dragging a selection shows a ghost of it at the destination, in every pane at
once including the 3D one, with the offset written out in kerosene units. The
rubber band a drag sweeps out is not where anything ends up, so it is not what
gets drawn.

Clicking a brush that belongs to an entity selects the entity, not the brush:
that is what a designer means by "the door".

**Entity properties.** The inspector is driven by the game's class definitions
-- the `.kerodef` files Chisel finds under the content root. For a selected
entity it lists *every* key its class reads, whether or not the entity has been
given a value for one, with the type, the game's default and a line of help.
Keys are edited with a widget suited to what they hold: a colour picker for a
light's colour, checkboxes for a spawnflag field, a menu of the map's entity
names when wiring an output. A key the definitions do not describe is still
shown -- that is how a typo becomes visible rather than silent.

Without a `.kerodef` the inspector can only show the keys an entity already
carries, which for a freshly placed entity is none. Chisel says so in the
status bar rather than looking like a game with no settings.

**The 3D pane** is rasterised in software with a depth buffer, so what hides
what is decided per pixel, and it draws the materials themselves --
perspective-correct, mipped, with a face tinted rather than painted over when
it is selected so you can still see what it is wearing. `view` switches
between *textured*, *flat colour* (each material's average, when a texture is
too busy to read shape through), *shaded only* (untextured grey, for hunting
a brush in the wrong place) and *walkmap* -- which colours each face by its
walkmap rule (green to go, red to stay away) without touching the texture, so
you can read where NPCs may go without compiling. Lighting is not previewed;
compiling and running the map is one keystroke away.

Tool **volumes** are drawn see-through, as they are in Hammer, because that is
what they are: a trigger is a region, not a wall, and one drawn solid hides the
room it is sitting in. `tools/nodraw` and the other solid tool materials are
the exception -- those *are* walls, just ones nobody sees, so they stay opaque.
A volume does not claim the depth buffer either, so two overlapping ones both
show and neither erases what is behind it.

It reads the **compiled** `.kerotex`, through the same VFS the engine uses, so
what it shows is what the engine will draw -- including from inside a `.vault`
archive. The consequence is worth stating plainly: **the content has to be
built**. A material Alchemy has not compiled yet shows as a flat colour derived
from its name, so it is a wrong colour rather than a black hole, and `view →
reload textures` picks up a rebuild without restarting.

**Materials** are picked from a grid of what they actually look like, with a
filter box. A list of names is only usable by someone who already knows what
every name looks like, which is nobody on their first level.

**Face editing.** With the texture tool, clicking a face in the 3D pane selects
it (shift adds, ctrl picks its material up), and the inspector becomes a face
editor: scale, shift, rotation, fit, align to world or to the face itself,
justify to an edge or the middle, the lightmap scale, and the face's **walkmap
rule** -- `allow`, `deny`, `avoid` or `always` -- which decides whether the
compiler puts that face in the NPC walkmap. Everything acts on the whole
selection as one undo step, and a value the selected faces disagree about
shows as `--` rather than as one of them.

The texture tool has two settings, chosen in the toolbar: *what* it selects
-- **single face** or **whole brush** (a brush entity is selected whole, as
the door) -- and *when* it applies, cycled with `T`: **select only**, **apply
on double-click** (the default) or **always apply**. Shift always just
selects, in every combination. The face editor only applies when a face is
selected, so it simply has nothing to show for a whole-brush selection.

**Compiling.** The compile button on the toolbar (or `F9`) compiles fast and
runs. `map → compile settings...` opens the settings with three buttons:
*compile* runs exactly what the dialog is showing, while *fast* and *full*
apply a quality preset and leave every other choice alone. That distinction
matters -- "build even if the map leaks" is not something a quality preset
gets to forget. The dialog closes when the compile starts; the log streams
into the toolset's output panel, where it does not cover the map.

A compile starts by running Alchemy over `content/art`, so a texture added or
changed since the last build is compiled before the map that uses it, and the
editor's own texture cache is reloaded when the compile finishes -- a new
texture shows up in the pane without a restart. Uncheck *build materials* to
skip that stage when the art has not moved.

When a map is not sealed, Cleave writes a `.keroleak` trace beside it and
Chisel loads it and draws the route out in red, through every pane. Follow the
line to the wall it goes through. `map → clear the leak trace` puts it away.

Chisel runs the compilers by re-invoking the same executable with the
compiler's name as a subcommand, so they are always present. `map → check
tools are installed` still says which pieces it found, including the engine
runtime it launches after a compile.

**Developer textures.** `kerosene-tools alchemy dev-textures` writes the standard set, and
`scripts/build-content.sh` runs it: `dev/` measurement checkerboards where one
cell is 16 ku at the default texture scale, and the full `tools/` set --
`nodraw`, `clip`, `playerclip`, `trigger`, `hint`, `skip`, `skybox` and the
rest -- each a flat colour with its own name written across it. The compiler
already understood every one of those; until now none of them had a texture, so
picking one in the editor showed nothing.

**Units.** Distances are kerosene units (`ku`); one is an inch. A player is 72 ku
tall and runs at 320 ku/s, which is the scale a room is judged against. The
status bar carries the unit on every number, with metres and a player-height
comparison on hover.

---

## Cleave — the BSP compiler

```sh
kerosene-tools cleave map.keromap [-o out.kerobsp] [--ignore-leaks] [--no-fill] [--dry-run] [-v]
```

`.keromap` → `.kerobsp` plus a `.keroprt` portal graph for Umbra, and a
`.kerowalk` NPC walkmap built from the world's flat walkable faces. The
walkmap is written every compile -- the compiler already has the final face
polygons, so the designer does not run a separate step to get one.

Reports every brush and entity problem in one pass rather than stopping at the
first, because a designer would rather fix five brushes in one cycle than five.

**Leaks.** If the flood fill escapes to the void, the map is not sealed and
Cleave refuses to build it — visibility would be nearly useless and the compile
would take far longer. `--ignore-leaks` builds it anyway and writes a `.keroleak`
trace naming the route out, which is the only practical way to find a one-unit
gap in a large map.

A compile that seals the map **deletes** any `.keroleak` left beside it by an
earlier one. A stale trace is worse than none: Chisel loads whatever is on
disk, so a map that leaked once would go on reporting a leak through every
successful compile after it.

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
kerosene-tools umbra map.kerobsp [--portals map.keroprt] [--fast] [--dry-run]
```

Computes which clusters can see which, and writes the PVS back into the map.

Reports "clusters visible per cluster" — the single best predictor of frame
rate in a BSP engine. Lower is better.

`--fast` stops after the base estimate: much quicker, leaves far too much
visible, and exactly what you want while a layout is still moving.

Umbra also writes the *potentially audible set* — the PVS grown by one room —
which is what lets the engine silence a sound that has no way of reaching you.

---

## Resonance — the acoustics compiler

```sh
kerosene-tools resonance map.kerobsp [--content DIR] [--portals map.keroprt]
                  [--fast | --extra] [--rooms] [--dry-run]
```

Works out what each part of the map sounds like, and writes it back so the
engine's reverb reads it straight off the map. An empty concrete hall rings;
a carpeted office does not; a courtyard open to the sky barely does at all —
and nobody placed anything to make it so. See
[`audio.md`](audio.md#how-a-room-sounds) for what the engine does with it.

It listens by throwing rays. From inside every leaf a few hundred rays go out
and bounce until they have nothing left, and each surface they strike gives
up how much it soaks up per band — from the material's `$surfaceprop`, or its
`$acoustics` key when a designer has said otherwise. The averages are what
Eyring's formula wants, measured rather than assumed, in a shape no formula
assumes. Leaves that touch and sound alike are then gathered into *rooms*, one
record each, so a hall the BSP cut into twenty pieces is one hall again and a
doorway is a boundary because the two sides disagree.

| Output | |
|---|---|
| `rt60` | Seconds for each of 125, 500, 2000 and 8000 Hz to fall 60 dB |
| `predelay` | Time before the first reflection: the round trip to the nearest wall |
| `openness` | The share of rays that escaped to the sky; over 0.35 is outdoors |
| `wet` | How loud the room is next to the sound itself |
| `diffusion` | How evenly the reflections are spread |

`--rooms` prints the table. `--fast` uses a third of the rays, which is
rougher and several times quicker; `--extra` four times as many, for a final
build. It needs the materials, so it is told where the content is like Cleave
is, and it reads the portal file Umbra reads to know which leaves touch —
without one it joins leaves whose bounds touch, which is close.

| Entity | |
|---|---|
| `env_acoustic_override` | The designer's last word: `rt60`, `wet`, `predelay`, `openness` for the room it sits in, and every room within `radius`. Anything left blank keeps the measured figure. |

The whole thing is deterministic — the rays are seeded from the leaf index —
so the same map compiles to the same bytes, and a room that rings differently
after a rebuild does so because the map changed.

---

## Radiance — the lighting compiler

```sh
kerosene-tools radiance map.kerobsp [--samples 1-8] [--bounces 0-8] [--scale N]
                  [--ambient-scale N] [--cubemap-size 4-256] [--fast] [--dry-run]
```

Bakes static lighting from the map's own light entities, then the reflection
probes.

| Entity | |
|---|---|
| `light` | A point light. `_light` is `"r g b brightness"`. |
| `light_spot` | A cone. `_cone`, `_inner_cone`, `_exponent`, `pitch`. |
| `light_environment` | The sun and the sky. `_light` is the sun, `_ambient` the fill. |
| `env_cubemap` | A reflection probe: what the lit world looks like from here. |

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

**Reflection probes.** After the lightmaps, Radiance stands at each
`env_cubemap` and records what it sees in every direction -- one ray a texel,
the colour being the lightmap where the ray landed times the surface's
reflectivity, or the sky's colour if it escaped. `--cubemap-size` is the edge
of each face, 32 by default, which is what Source ships its cubemaps at; the
renderer blurs them further for rough surfaces. Put one at head height in
each room. Every smooth or metal face reflects the nearest probe it can see,
and a room with none reflects an even glow instead. A probe inside a wall
sees nothing and Radiance says which. Unlike Source's `buildcubemaps`, this
is part of the compile: there is no step to forget.

---

## Alchemy — textures and materials

```sh
kerosene-tools alchemy compile art/grid.png -o materials/dev/grid.kerotex [--normal] [--clamp] [--ui]
kerosene-tools alchemy material dev/grid --basetexture dev/grid --shader lit
kerosene-tools alchemy batch art -o materials --make-materials
kerosene-tools alchemy new-texture Walls/brick --basecolor b.png --normal b_n.png
kerosene-tools alchemy texture-set content/textures/Walls/brick
kerosene-tools alchemy build content
kerosene-tools alchemy info materials/dev/grid.kerotex
```

Compiles PNG/JPEG/TGA into `.kerotex` and authors `.keromat` materials.

`new-texture` is the deliberate way to add one. It makes a folder under
`content/textures/`, copies the images in under canonical names, and writes the
`texture.kconfig` that documents what the set can say — as against the older
route of dropping a PNG under `art/` and relying on a filename suffix to be
guessed correctly. The folder is a *texture set*: colour, normals, roughness,
emissive and occlusion compiled together, plus the material binding them. See
[formats.md](formats.md) for the folder layout and the config keys.

`texture-set` compiles one such folder by hand. It derives the set's name the
same way a full build does — by climbing to the `textures/` directory above it
— so a set compiled either way ends up called the same thing. Pass `--root` to
say otherwise.

`build` is the whole texture half of a content build for one project: the
developer set is generated into `art/`, then everything under `art/` is
compiled into `materials/`, then every texture set under `textures/` is. Sets
go last, so a set may deliberately shadow a loose image of the same name: the
folder is the more specific statement of the two. A project with no `textures/`
directory builds as it always did. It is one command because three callers need
exactly it -- this tool, `scripts/build-content.sh`, and Chisel on the way to
opening its window -- and three callers with three ideas of what "build the
textures" meant is how the editor came to open with no textures in it while the
build script insisted everything was fine. Alchemy is a library as well as a
command so the editor can call it rather than shell out to a sibling binary
that may not be on the path.

`batch` and `build` skip an image whose `.kerotex` is already newer than it, so
a build with nothing to do costs a directory walk.

Alpha is dropped when an image does not use it, which saves a quarter of the
memory. In `batch` mode, a file ending `_normal` or `_n` is taken to be a
normal map — the convention beats a flag, because batch compiles run
unattended.

---

## Forge — the model compiler

```sh
kerosene-tools forge compile art/crate.obj -o models/props/crate.keromdl
                            [--scale-metres] [--z-up] [--scale N]
                            [--material old=new] [--recompute-normals]
kerosene-tools forge compile art/turret.glb -o models/props/turret.keromdl
                            [--scale N] [--material old=new] [--once clip]
kerosene-tools forge info models/props/crate.keromdl
```

OBJ or glTF (`.gltf`, `.glb`) → `.keromdl`, splitting by material and welding
vertices.

**glTF** carries what OBJ cannot: a skeleton and its animations. Forge reads
every triangle mesh in the default scene, the first skin as the model's bones
(parents first, rest poses from the skin's inverse bind matrices), and every
animation, resampled at 30 fps; bone translation and rotation are kept and
animated scale is warned about. glTF is metres, Y-up and faces +Z by
definition, so there is no `--scale-metres` or `--z-up` to get wrong. Clips
loop unless named with `--once`. Kiln picks up `.gltf` and `.glb` under `art/`
beside `.obj`. A `prop_dynamic` plays the result.

Two conversions happen on the way in, and getting either wrong produces a model
that is subtly rotated or a hundred times too small. OBJ is Y-up with -Z
forward; Kerosene is Z-up with +X forward. And modelling packages usually
work in metres — `--scale-metres` converts.

Winding is taken from the OBJ as-is (the axis remap preserves handedness), so
export your faces counter-clockwise from the front: `.keromdl` stores them
that way, and a face wound the other way renders inside out. `--recompute-normals`
rebuilds normals from the winding and ignores any in the source, but it does
not repair a source that was wound backwards to begin with.

Welding is by the full corner tuple, not by position: two faces meeting at a
hard edge legitimately share a position while needing different normals, and
merging them rounds off every corner of the model.

---

## Kiln — building a project

The Build tab in the toolset window -- stages as toggles, one button, and the
log in the output panel -- and also a headless stage:

```sh
kerosene-tools kiln                              # build everything, from here
kerosene-tools kiln --content path/to/content    # or from there
kerosene-tools kiln --only maps --fast           # just relight, quickly
kerosene-tools kiln --only textures              # after adding art
kerosene-tools kiln --dry-run                    # say what would run
kerosene-tools kiln --tools                      # which pieces can be found
kerosene-tools kiln --ship dist                  # build, then assemble a distribution
```

Runs the whole content pipeline over a project: the texture build, then models
through Forge, then every map through Cleave, Umbra, Resonance and Radiance,
then the pack into a `.vault`.

It is a program rather than a shell script for one reason, and it is the
reason that matters: **a script is not shipped**. Install the tools, or copy
them somewhere, and the thing that knows how to *use* them stays behind in a
git checkout — so the first thing anyone does with a fresh copy of the
toolchain is discover the build step is missing. Kiln is part of the one
toolset executable, so it needs no shell.

The compilers are stages of the same toolset and Kiln drives them by
re-invoking the executable with the stage's name as a subcommand, exactly as
Chisel does. You can still run any stage by hand or from a build server. Only
the texture build is a library call, because Chisel makes the same one and the
two must not be able to disagree.

Sources decide what gets built: every `.obj` under `art/` becomes a
`.keromdl` at the matching path under `models/`, and every `.keromap` under
`maps/` becomes a `.kerobsp`. Nothing has a list to keep up to date.

A map that leaks still compiles, and is reported at the end rather than
stopping the build — finding out on the first of forty maps that the run is
over is not a service. The archive is named after the project and written
inside the content tree, which is where the engine looks for it.

`scripts/build-content.sh` in this repository is a thin wrapper: it builds the
toolset from source and regenerates the sample map from the code that defines
it, then calls Kiln. Neither of those two belongs in a shipped tool.

`--ship <dir>` is the stage after the content: it builds the project's
`game` package if the `.keroproj` names one (or takes the `kerosene` runtime
beside the toolset if not), then assembles a distribution — the binary, the
`.vault` under `content/`, a `.keroproj` pointing at it, both licence texts
and a `README.txt` carrying the notices the licences require. It copies a
named list of files rather than a directory, so no tool ever ends up in a
player's hands, and it refuses an archive that is missing or older than the
content tree rather than shipping maps nobody built. What it does and does
not do for a release is the subject of
[Publishing](../gamedev/publishing.md).

---

## Vault — content archives

The Archive tab in the toolset window -- pack, verify and list, with the log
in the output panel -- and also a headless stage:

```sh
kerosene-tools vault pack content -o content/kerosene_content.vault [--ext kerotex] [--exclude tmp]
kerosene-tools vault list content/kerosene_content.vault [--long]
kerosene-tools vault verify content/kerosene_content.vault
kerosene-tools vault unpack content/kerosene_content.vault -o extracted
```

The archive belongs *inside* the content tree, which is where a shipped game
keeps its archives and where the engine looks without being told. Writing it
into the tree it packs is safe: `.vault` is never one of the extensions packed.

Packs a content tree into one archive. Every entry carries a CRC, checked on
read and by `verify`.

Loose files still shadow packed ones when both are mounted, so a developer can
drop a file next to a shipped archive without repacking.

---

## kerosene — the runtime

```sh
kerosene [+command ...] [--content <dir>] [--vault <file>] [--headless <ticks>]
```

Arguments beginning with `+` are console commands, so any convar is settable
from the command line with no flag needing to exist for it:

```sh
kerosene +map kero_start
kerosene +map kero_start +sv_gravity 200 +developer 1
kerosene --headless 640 +map kero_start
kerosene --content path/to/content --vault extra.vault +map kero_start
```

`--headless` runs the simulation with no window at all — which is what a
dedicated server is, not a testing mode bolted on the side.

With no `--content`, the engine finds the content tree the way every tool does
(see Chisel, above) and says which one it took. With no `--vault`, every
`.vault` in that tree is mounted, in name order, so a packed install runs
without being told about its own archives. Loose files still win over packed
ones, which is what makes dropping a file beside a shipped archive work.

A map that will not load says why rather than saying "not found in any search
path". The usual reason is that it has never been compiled — the `.keromap` is
right there and nothing turned it into a `.kerobsp` — so that is what it says,
along with the command to run and the list of paths it searched.

### The console

`` ` `` opens it, `` ` `` or escape closes it, and `toggleconsole` does the
same from a binding or the command line. Those two keys are read by the host
before anything else sees them and are never passed on — a way out that the
thing you are trying to leave can capture is not a way out, and handling them
any later meant the console's own text field swallowed them. The same mistake
put the backtick that opened it *into* the prompt, so every command typed
afterwards began with a character that made it unknown.

While it is open the console takes the keyboard completely and releases the
mouse: a console you cannot type an `n` into without walking forward is not a
console. Tab completes the command word and cycles the candidates, up and down
walk history, page up and down scroll without disturbing what you are typing.

It introduces itself the first time it opens, because an empty box with a
blinking cursor reads as "this accepts nothing":

```
Kerosene console -- 52 commands and convars. `find <text>` searches them,
`help <name>` explains one, `cvarlist` lists the lot. Tab completes, up walks
back, ` or escape closes.
```

Log lines from the engine appear in it as they happen. Crates that are not
ours — the graphics backend, the window library — are held to warnings, so
opening the console to read one line does not mean scrolling past a page of
Vulkan loader chatter. `RUST_LOG` lifts that: someone who sets it is debugging
the thing they set it for.

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
| `sv_stream` `sv_stream_linger` `r_stream_debug` | streamed sections: on/off, seconds a section lingers, draw their bounds |
| `mat_exposure` | overall brightness, applied by the tone-map pass |
| `mat_tonemap` | tone curve: `0` none (clip), `1` Reinhard, `2` ACES filmic (default) |
| `r_msaa` | multisample anti-aliasing: `0`/`1` off, anything higher 4x (default) |
| `r_bumpmap` `r_specular` | material debug scales: `0` removes normal maps / reflections, `2` exaggerates (cheat) |
| `r_dynamic` `r_shadows` | dynamic lights (`light_dynamic`, the flashlight) and their real-time shadows |
| `flashlight` / `cl_flashlight` | toggle the flashlight (bound to F) / whether it is on |
| `volume` `snd_reverb` `snd_reverb_preset` | sound; see [`audio.md`](audio.md#how-a-room-sounds) |
| `sv_pause_on_menu` / `pause` | stop the world while the pause menu, console or Steam overlay is open, or the window is in the background (default on) / pause whatever is open |
| `snd_mute_losefocus` | silence the game while its window is in the background (default on) |
| `developer` | verbosity; `2` also traces entity I/O |

`sv_air_max_wishspeed` is the air-speed cap that makes bunny-hopping and
surfing work. It is 30 by default, and it is not a bug: changing it changes the
game.

---

# Timbre — the sound compiler

Turns `.wav`, `.flac` and `.mp3` into `.keroaud`. It is the one tool with no
Source counterpart, because Source shipped `.wav` and paid for it in download
size; this pays a compile step instead. It is the sound tab in the toolset
window, and also a headless stage:

```
kerosene-tools timbre                          # the sound tab
kerosene-tools timbre build                    # compile a project's sounds
kerosene-tools timbre compile a.wav --gain 0.8 --mono
kerosene-tools timbre info a.keroaud
```

## What it reads

WAV goes through the engine's own decoder — the one that also reads `smpl`
chunk loop points. FLAC and MP3 go through Symphonia, which is a dependency
this project would not accept in the engine and does accept in a build tool:
see [`licensing.md`](licensing.md#the-one-copyleft-dependency).

MP3 is read because people have MP3s, not because it is a good thing to build
from. Compiling one to ADPCM is lossy-to-lossy — the artifacts compound rather
than cancelling, and the second encoder spends its bits describing the first
one's mistakes. Timbre says so every time, and says so again if the result
comes out *larger* than the source, which for an already-compressed input it
often does.

A file whose extension lies about its contents is named as such rather than
reported as corrupt: a `.wav` that is really an MP3 is a thing that happens to
downloaded files, and "not a RIFF/WAVE file" sends you looking for the wrong
problem.

FLAC can carry a loop region in `LOOPSTART`/`LOOPLENGTH` Vorbis comments, which
is what game audio has settled on, and Timbre reads it — otherwise a looping
ambience compiled from FLAC loses what the same sound in a WAV would keep.

## What it decides

**Encoding.** ADPCM at a quarter the size, or 16-bit PCM. Per sound, not per
project: ADPCM is close to transparent on impacts, speech and machinery, and
audible on a quiet room tone with a lot of air in it. It also has an attack
transient — the quantiser starts at its smallest step and takes a few
milliseconds to reach a loud signal — which softens a sharp onset. PCM16 is
the escape hatch for material where either matters.

**Gain.** Applied before encoding rather than at play time, so a sound
recorded too hot is fixed once instead of in every entity that plays it.

**Loop points.** Read out of the WAV's `smpl` chunk if it has one. Before
this, looping was all-or-nothing: a room tone with a proper loop region had it
thrown away and was repeated end to end, click and all.

**Channels.** A sound placed in the world has to be mono — there is one pan
and a stereo file already carries its own left and right, so positioning it
applies a pan to a signal that is not a point. Timbre says so, and can fold it
down.

## The window

`timbre` with no arguments opens it, because every one of those decisions is
better made by seeing and hearing the result than by reading a number. A gain
of 0.8 means nothing on a command line; the same 0.8 with the waveform redrawn
under it and the clipped samples marked in red means something at a glance.

- The waveform is the samples that will actually be written, not the source.
- Peak and a live level meter, the second following the playhead.
- Play through the same mixer the engine uses.
- Gain in decibels, encoding, mono, and the loop region shaded on the wave.

Settings are written to `sound/timbre.kerobuild` and read back by `timbre
build`, so the window and the command line cannot disagree about what a build
is — the same discipline that makes the texture build a library call rather
than a second implementation.

## Where the window comes from

`kerosene-toolui` is a window with egui in it, and the look every tool shares:
winit's application handler, a wgpu surface, an egui integration and the frame
loop that drives them, plus `theme` (the palette, the spacing and the
[Phosphor](https://phosphoricons.com/) icon font, installed once by `run`),
`widgets` (tool buttons, tabs, sections, chips, menu items with their
shortcuts on the right, a dialog) and `output` (the panel every job logs
into). Implement `App`, call `run`. It exists because the window is three
hundred lines with nothing to do with any particular tool, and a second copy
of them is a second place for a resize bug to live -- and because a palette
each tool chose for itself would be three palettes.

**The window icon** is the Kerosene mark from `.github/Images/`, compiled
into every binary that opens a window (`kerosene_config::icon`) and set on
the window. X11, Windows and macOS show it from there. Wayland does not let a
window carry its own icon: the compositor shows the icon of the `.desktop`
file whose name matches the window's app id, which is `kerosene`.
`scripts/install-desktop.sh` installs that entry and the icon set under
`~/.local` for the current user; `--uninstall` takes them out again.
