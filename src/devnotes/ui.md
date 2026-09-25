# The game UI

How the UI is built, for someone changing it. For how to *use* it, see
[Game UI](../docs/ui.md). When a note and the code disagree, the code wins.

## Where it lives

| | |
|---|---|
| `crates/kerosene-ui` | Everything up to a list of quads. No GPU, no winit, no engine |
| `crates/kerosene-render/src/ui.rs`, `shaders/ui.wgsl`, `shaders/panel.wgsl` | Drawing those quads, on screen and into world panel textures |
| `crates/kerosene-render/src/decals.rs` | Cutting decals out of the world mesh (CPU) |
| `crates/kerosene-render/src/gpu.rs` (`GpuDecals`, `Pass::Decal`) | Drawing them |
| `crates/kerosene-engine/src/ui.rs` | The engine's side: the store, publishing, console commands, world panel entities, the decal list |
| `crates/kerosene-engine/src/host.rs` | Input routing, the menu's hold on the mouse, the frame's UI passes |
| `crates/kerosene-game/src/ui.rs` | `logic_ui`, `point_worldpanel`, `infodecal` |
| `crates/kerosene-game/src/weapons.rs`, `crates/kerosene/src/lib.rs` (`Stock`) | The weapon stub that feeds the HUD |
| `crates/kerosene-toolui` | Not this. The tools' egui host, which used to own the name |

## `kerosene-ui`, in order

```mermaid
flowchart LR
    xml[".keroui"] --> markup["markup.rs<br/>escape, parse"] --> doc
    css[".kerocss"] --> cssrs["css.rs<br/>rules, selectors"] --> doc
    rhai[".keroscript"] --> script["script.rs<br/>sandboxed VM"] --> doc
    store["store.rs<br/>UiStore"] --> bind["bind.rs<br/>templates, deps"] --> doc
    doc["document.rs<br/>Document::update"] --> style["style.rs<br/>cascade"]
    doc --> layout["taffy<br/>flexbox"]
    doc --> text["text.rs<br/>ab_glyph, atlas"]
    doc --> draw["draw.rs<br/>DisplayList"]
    system["system.rs<br/>UiSystem: layers, panels, hot reload"] --> doc
```

`Document::update` runs the same steps every frame:

1. Evaluate the bindings whose store keys changed since the last frame
   (`UiStore::changed_since`, `Template::depends_on`).
2. Deliver the store's events to `on:` handlers and `on_event`.
3. Run due timers, then the handlers input queued.
4. Apply what scripts asked for (`DocOp`s, store writes, `UiAction`s).
5. Re-cascade every panel marked dirty, with its subtree.
6. Advance transitions and keyframe animations into `Node::shown`.
7. Lay out with taffy. It caches, so an unchanged tree costs almost nothing.
8. Rebuild the display list, recording each panel's drawn rect and inverse
   transform for hit testing.

Input (`pointer_move`, `pointer_button`, `key`, `text`) never runs script. It
changes state and queues `Pending` handlers, which step 3 runs with the store
in scope. So a click's handler runs on the next update, and input handling
needs no access to the store.

### Decisions worth knowing

- **Scripts never touch a live structure.** Like level scripts, UI script
  calls queue `DocOp`s and `UiAction`s and are applied after the script
  returns (`script.rs`).
- **Bindings are Rhai expressions** compiled once, with store roots pushed into
  the scope as nested maps (`UiStore::to_scope_maps`). Every root a binding
  names is pushed, as an empty map if nothing has been published under it, so
  an unpublished key reads `()`. Dependencies come from a textual scan
  (`bind::dependencies`), which is conservative: a false positive only costs an
  extra evaluation.
- **Markup is preprocessed** (`markup::escape_code`, `declare_prefixes`): `<`
  and `&` inside attribute values and `<script>`/`<style>` are escaped, `--`
  in comments is neutralised, and the `class:`/`style:`/`on:` prefixes are
  declared as XML namespaces on `<root>`. Single-quoted strings in code become
  double-quoted (`bind::single_quotes_to_double`). Each of these came from a
  sample layout failing to load.
- **Units.** Layout happens in UI pixels (reference height 1080, or
  `reference-height`). The display list is in physical pixels. Glyphs are
  rasterised at physical size, keyed by size in quarter pixels.
- **`<Repeat>` has no layout box.** Its children are attached to the nearest
  non-Repeat ancestor's taffy node (`layout_parent`, `sync_layout_children`),
  and `read_layout` passes the parent's origin through it.
- **Transitions** snapshot `shown` when a property's target changes and blend
  from that snapshot to the new target. Only the properties in
  `style::AnimProp` animate. Keyframe animations hold their last frame
  (`forwards`).
- **Hot reload** reads every source a document loaded once a second and
  compares FNV hashes. It works through the VFS, so it also covers files in
  archives. A document that fails to reload keeps the old one.

## Rendering

`pack` turns a display list into `GpuQuad` instances and batches, splitting
on image, blend mode and scissor. Solid and glyph quads can join any batch,
since the image binding is unused for them. A quad whose image hasn't loaded
is dropped *before* batching. Otherwise one missing image took every quad in
its batch down with it, which is how a missing logo once blanked a whole
menu.

The shader converts authored sRGB colours to linear, blends premultiplied in
linear, and lets the sRGB target encode. If the target isn't sRGB,
`output_srgb` encodes in the shader instead. Both textures are sampled before
any branch or `discard`, to keep naga's uniformity analysis happy.

Frame order in `host.rs`:

1. `ui_frame` runs the documents. This happens in `App::frame`, after the
   ticks.
2. Upload the glyph atlas and any new images.
3. Render world panels into their textures, but only those whose `revision`
   moved.
4. Cut new decals and upload them if the decal revision moved.
5. Shadows, then the scene pass. Decals are drawn per section after that
   section's world, with the section's frame bind group, because each section
   has its own lightmap atlas. World panels are drawn after the props.
6. Tonemap, then the screen UI, then egui (`Game::ui` and the console).

## Decals

`decals::build` clips each world-model triangle inside the decal's oriented
box (Sutherland–Hodgman against six planes), keeps its lightmap UVs,
replaces the UVs with the decal's projection, and lifts the result
`decals::LIFT` units off the surface. Facing is judged by vertex normals, not
winding. The host caches cut geometry by `DecalRequest::id` and clears the
cache when the map generation changes. The engine keeps only the request list,
so headless runs agree with windowed ones about what was placed.

## World panels

`Engine::sync_world_panels` rebuilds the panel list from `point_worldpanel`
entities every UI frame. The panel faces along the entity's forward vector,
and its right edge is `-basis.right`, the viewer's right. `world_panel_input`
runs in the tick before use and attack are handled: it ray-casts the view
against interactive panels within 1.5 × `sv_use_range`, checks the world
isn't in the way, and turns presses into pointer events. When it takes a
press, the tick does not also use or throw. `Engine::aiming_at_panel` lets a
game hold its fire too, and the stock game does.

## Tests

- `crates/kerosene-ui/src/*/tests.rs` cover each module headless.
- `crates/kerosene-ui/tests/content.rs` loads every shipped layout and fails
  on any warning.
- `crates/kerosene-engine/tests/ui.rs` uses a compiled map to test map scripts
  publishing, damage events, `infodecal`, the `decal` command, the pause menu,
  and a keypad panel whose `OnUnlock` opens a door.
- `crates/kerosene-render/tests/ui_smoke.rs` runs on a real GPU and skips
  without one. Set `KEROSENE_UI_SHOT=<dir>` to get PNGs of the shipped HUD
  and pause menu.

> Next: [Tools and the build](tools-and-build.md).
