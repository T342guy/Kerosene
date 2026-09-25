# Game UI

HUDs, menus, screen overlays and screens in the level are all built the same
way: an **XML layout**, a **CSS stylesheet**, and optionally a **Rhai
script**, loaded from the content tree like any other asset. It is the shape
of Source 2's Panorama, with one idea borrowed from HTMX: an element *says*
what game state it shows, in its attributes, instead of a script finding it
and updating it.

```xml
<Label text="{weapon.ammo} / {weapon.reserve}" class:low="{weapon.ammo < 5}"/>
<Panel class="crosshair xh-{weapon.active}"/>
```

That label shows the ammo count and turns red when it runs low; the crosshair
swaps its look whenever the weapon changes. No script is involved. Most of a
HUD is built like this.

Edit a layout, stylesheet or script with the game running and it reloads a
second later.

## Files

| Extension | What |
|---|---|
| `.keroui` | Layout: XML |
| `.kerocss` | Stylesheet: a subset of CSS |
| `.keroscript` | Behaviour: Rhai, the same language as [level scripts](scripting.md) |
| `.ttf` `.otf` | Fonts, named by `@font-face` |
| images | Ordinary textures: `art/ui/logo.png` is compiled by Alchemy and named `ui/logo` |

The stock game ships these under `content/ui/`:

| File | Shown |
|---|---|
| `ui/hud.keroui` | While a map is running (the `ui_hud` convar) |
| `ui/overlays/damage.keroui` | By the HUD's script, on the `overlay` layer |
| `ui/menus/pause.keroui` | By Escape (the `ui_pausemenu` convar) |
| `ui/panels/keypad.keroui` | On a `point_worldpanel` in `kero_start` |
| `ui/panels/status.keroui` | On a `point_worldpanel` in `kero_start` |

They are written to be read. Copying one is the fastest way to start.

## A layout

```xml
<root interactive="true" z="100">
    <styles>
        <include src="ui/menus/menu.kerocss"/>
    </styles>
    <scripts>
        <include src="ui/menus/pause.keroscript"/>
    </scripts>

    <Panel id="menu">
        <Label class="title" text="Paused"/>
        <Button text="Resume" onactivate="resume()"/>
    </Panel>
</root>
```

The outer element is always `<root>`. Its attributes:

| Attribute | |
|---|---|
| `interactive="true"` | The layout takes the mouse and keyboard while shown, and nothing below it gets them: a menu |
| `z` | Draw order among layers. Higher is on top |
| `reference-height` | How many UI pixels tall the screen is. Default 1080 |

`<style>` and `<script>` blocks may also be written inline, directly under
`<root>`. You don't need to escape `<` and `&&` in bindings and scripts: the
loader escapes them before the XML reader sees them.

### Elements

| Element | |
|---|---|
| `Panel` | A box. Everything is built from these |
| `Label` | Text: `text="..."`, or the element's own text |
| `Image` | `src="ui/logo"`, a compiled texture. Fits `contain` unless styled otherwise |
| `Button` | Clickable and focusable. `text=` gives it a label |
| `TextEntry` | A text field. `value`, `placeholder`, `maxlength`, `password` |
| `Slider` | `min`, `max`, `step`, `value` |
| `Toggle` | A checkbox. `checked`, and `text=` for its label |
| `ProgressBar` | `value` from 0 to 1 |
| `Repeat` | Its children, `count` times. Each copy sees `index` (rename with `as=`) |
| `Include` | `src=` another layout, pasted in here |

Controls make parts of their own for the stylesheet to style:
`.slider-track`, `.slider-fill`, `.slider-thumb`, `.toggle-box`,
`.toggle-knob`, `.progress-fill`, and a `Label` inside a `Button` or
`Toggle`. Their look before any sheet of yours comes from
`crates/kerosene-ui/src/default.kerocss`, which is compiled into the engine.

A `<Repeat>` has no box of its own. What it makes is laid out in its parent as
if written there, so a row of slots made by a Repeat inside
`#slots { flex-direction: row }` is a row. Because of that, select what a
Repeat made with a descendant selector (`#slots .slot`), not `>`.

### Attributes every element takes

| Attribute | |
|---|---|
| `id`, `class`, `style` | As on the web |
| `visible="{...}"` | Hidden and out of the layout when false |
| `class:NAME="{...}"` | The class is on while the expression is true |
| `style:PROP="{...}"` | One property from an expression: `style:width="{hp}%"`. Write `style:kero-fill` for `-kero-fill` |
| `on:EVENT="..."` | Code to run when the event happens |
| `onactivate` `onchange` `onsubmit` `onmouseover` `onmouseout` `onfocus` `onblur` | Code to run on interaction |
| `cvar="NAME"` | On a `Slider`, `Toggle` or `TextEntry`: shows the convar and sets it when changed |
| `sound="NAME"` | A sound to play when activated |

## Bindings

Anything in braces is a Rhai expression, and published game state is its
variables: `player.health` is the `health` entry of the `player` map.

```xml
<ProgressBar value="{player.health / 100.0}"/>
<Label text="{if weapon.reloading { 'Reloading' } else { weapon.active }}"/>
<Panel visible="{player.alive && ability.dash.ready}"/>
```

- An attribute that is exactly one expression keeps its type, so `visible` and
  `class:` get a real true or false. Anything else becomes text.
- A binding is evaluated again only when a value it reads changes. A HUD with
  a hundred bindings costs nothing on a frame where none of their values moved.
- A value nobody has published yet reads as nothing (`()`), not as an error.
  `visible="{objective.text}"` stays hidden until a script sets an objective.
- Strings may be written in single quotes, which is the natural way inside a
  double-quoted attribute.
- `{{` and `}}` are literal braces.

### What is published

Run `ui_dump` in the console for the live list. The engine publishes:

| Key | |
|---|---|
| `player.health` `player.alive` `player.speed` `player.x` `player.y` `player.z` | |
| `map.name` | |
| `ui.menu_open` | |
| `cvar.NAME` | Any convar a layout reads |
| `platform.available` `platform.name` `platform.user` `platform.overlay` | The store: Steam, or `none` |
| `platform.achievements.ID` `platform.stats.NAME` `platform.dlc.APPID` `platform.names.ID` | Every declared one; `names` are the display names. See [Steam](../gamedev/steam.md) |

The stock game (`kerosene::game::Stock`) adds its weapons and ability:

| Key | |
|---|---|
| `weapon.active` `weapon.slot` | `pistol`, `shotgun`, `rifle` |
| `weapon.ammo` `weapon.clip` `weapon.reserve` | |
| `weapon.reloading` `weapon.reload_progress` `weapon.firing` | `firing` is true for a moment after each shot |
| `weapon.count`, `weapons.N.name` `weapons.N.slot` `weapons.N.active` | For a `Repeat` over the slots |
| `ability.dash.ready` `ability.dash.charge` `ability.dash.remaining` | `charge` runs 0 to 1 |

A game publishes its own from Rust with `engine.ui_set("key", value)`. A level
script uses `ui_set("key", value)`, and a map uses a `logic_ui` entity's
`SetValue` input. The HUD doesn't know or care which one set a value.

### Events

Events are one-off: something happened. Listen with `on:NAME` or a script's
`on_event(name, data)`.

| Event | Data | From |
|---|---|---|
| `player_damaged` | amount | engine |
| `player_died` | cause | engine |
| `map_loaded` | map name | engine |
| `weapon_changed` | `from to` | stock game |
| `weapon_fired` | weapon | stock game |
| `weapon_empty` `weapon_reload` `weapon_reloaded` | | stock game |
| `ability_used` `ability_ready` | ability | stock game |
| `achievement_unlocked` `stat_changed` `score_submitted` `overlay_opened` ... | see [Steam](../gamedev/steam.md#from-a-script-platform-or-steam) | the store |

Send your own with `engine.ui_emit(...)`, `ui_event(...)` in a level script,
`logic_ui`'s `Emit` input, or `ui_emit` at the console.

## Styles

A stylesheet is ordinary CSS: rules, selectors, specificity, comments,
`@keyframes`, `@font-face`. Selectors can use element names, `.class`, `#id`,
descendant and `>` child combinators, and `:hover` `:active` `:focus`
`:disabled` `:checked` `:first-child` `:last-child`.

Layout is **flexbox**, and a panel's children stack in a column unless told
otherwise.

Lengths are **UI pixels**. The screen is 1080 of them tall however many real
pixels it has, so a HUD laid out at 1080p keeps its shape at 720p and at 4K.
`%` is the parent, as on the web, and `vw`/`vh` are the screen.

| | Properties |
|---|---|
| Box | `width` `height` `min-*` `max-*` `margin` `padding` (and `-left` etc.) `border` `border-width` `border-color` `border-radius`. Sizes include padding and border (`border-box`) |
| Flex | `display` (`flex` or `none`) `flex-direction` `flex-wrap` `flex` `flex-grow` `flex-shrink` `flex-basis` `justify-content` `align-items` `align-self` `gap` `row-gap` `column-gap` |
| Position | `position` (`relative`, `absolute`) `left` `top` `right` `bottom` `inset` `z-index` `overflow: hidden` |
| Paint | `background-color` `background: linear-gradient(to bottom, a, b)` `background-image: url(ui/x)` `background-size` (`contain`, `cover`, `fill`) `opacity` `box-shadow` (a soft glow) `visibility` `transform` (`translate` `scale` `rotate`) |
| Text | `color` `font-family` `font-size` `font-weight` `font` `text-align` `vertical-align` `text-shadow` `letter-spacing` `line-height` `text-transform: uppercase` `white-space: nowrap` |
| Motion | `transition` `animation` with `@keyframes` |
| Input | `pointer-events: none` |

Kerosene adds three properties a game HUD needs:

| | |
|---|---|
| `-kero-fill: radial(0.25)` | Show a clockwise sweep of the panel from twelve o'clock: a cooldown. Also `horizontal(x)` and `vertical(x)` |
| `-kero-blend: additive` | Add the panel onto what is behind it: glows, flashes, hit markers |
| `-kero-tint: red` | Multiply an image by a colour |

Transitions and animations can move `opacity`, colours, `transform`,
`-kero-fill`, `width` `height` and the insets. An animation stays on its last
frame when it ends, which is what a flash or a fade-out wants. To play one
again, a script calls `trigger_class`.

`font-weight: bold` uses a bold face when a stylesheet has loaded one:

```css
@font-face { font-family: "Hud"; src: url("ui/fonts/hud-bold.ttf"); font-weight: bold; }
```

Without one, the text is drawn slightly heavier. The built-in face is egui's
Ubuntu Light.

A property the stylesheet can't read is reported once in the console with the
file it came from, and the rest of the rule still applies.

## Scripts

Bindings show state. A script is for behaviour: a menu page that opens, a
keypad that checks a code, a toast that appears and fades. Every layout gets
its own sandboxed Rhai VM, with the same limits as a level script, and:

| | |
|---|---|
| `panel(id)` | A handle to the element with that id |
| `p.add_class(c)` `p.remove_class(c)` `p.toggle_class(c)` `p.set_class(c, on)` | |
| `p.trigger_class(c)` | Add a class and restart its animation |
| `p.set_text(t)` `p.set_attr(name, v)` `p.set_style(prop, v)` `p.show()` `p.hide()` `p.focus()` | |
| `store(key)` `store_or(key, default)` `set_store(key, value)` | Published values. `set_store` publishes, so bindings update |
| `emit(name, data)` | An event: to every layout, to the game, and from a world panel, to the panel's outputs |
| `command(text)` `set_cvar(name, v)` `play_sound(name)` | |
| `show_layer(layer, file)` `hide_layer(layer)` | |
| `schedule(seconds, "function")` | Call a function later |
| `time()` `print` `warn` `error` | |
| `platform` (or `steam`) | The store, the same object level scripts have: `platform.unlock(id)`, `platform.user`, ... See [Scripting](scripting.md#the-store-platform-or-steam) |

The engine calls two hooks if a script defines them: `on_load()` and
`on_event(name, data)`. Code in an attribute (`onactivate="resume()"`) runs
with `target`, the element it's on, and `data`, the event's data or the
control's value.

In Rhai a function can't see the script's top-level variables. Keep state
in the store with `set_store`, which also lets the layout bind to it. This is
how `ui/panels/keypad.keroscript` works.

## Layers

Each shown layout sits on a named layer. Showing a layer loads its file, or
keeps the one already loaded along with its state. Hiding a layer keeps the
document, so showing it again is instant.

- `hud` is managed by the engine. It shows `ui_hud` whenever a map is running.
- `menu` is Escape's. It toggles `ui_pausemenu`. While a layer marked
  `interactive` is visible, the mouse is released and every key and click
  goes to it. When the menu closes, the mouse goes back to the game, whether
  it closed by Escape, a Resume button or a script.
- Any other name is yours.

The developer console always comes first. It opens over a menu, and its
keys close it.

## World panels

A `point_worldpanel` entity puts a layout on a surface in the level. The
layout is rendered into a texture, only when what it shows has changed, and
that texture is drawn in the world, depth-tested and glowing.

| Key | |
|---|---|
| `layout` | The `.keroui` to show |
| `width` `height` | Size in world units |
| `resolution` | Texture height in pixels. The width follows the shape |
| `brightness` | How brightly it glows; 1 is a surface lit to full |
| `interactive` | Whether the player can use it |
| `angles` | It faces along its forward direction; place it just in front of a wall |

When the player looks at an interactive panel within reach, the crosshair
works as the panel's pointer, and **use** or **attack** clicks. The panel
takes the press, so the wall behind it isn't used and the gun doesn't fire.

A panel's script talks to the level through `emit`:

```rhai
emit("OnUnlock", code);   // fires the panel entity's OnUnlock output
```

Every event fires the panel's `OnPanelEvent` output with the event's name as
the parameter. An event whose name starts with `On` also fires the output of
that name with its data, so the mapper wires `OnUnlock` in Chisel like any
other output. Its `Enable`, `Disable` and `Emit` inputs work the other way.

Set `reference-height` on the panel layout's `<root>` to the entity's
`resolution` to lay the panel out in its texture's own pixels.

## Decals

Bullet holes, cracks, scorch marks and signs are projected onto the world.
When one is placed, the world triangles under it are cut to its square. The
decal keeps the lightmap of the surface it lies on, so a decal in shadow is
in shadow, and it's drawn with the world's own shading, dynamic lights
included.

- `infodecal` entity: `texture` (a material) and `size`. It lands on the
  surface nearest its origin when the map starts.
- `decal [material] [size]` at the console (cheat): wherever you are looking.
- `place_decal(material, origin, normal, size)` in a level script, or
  `engine.place_decal(...)` in Rust.
- Every stock weapon's shots.

A decal material is an ordinary `.keromat` whose texture has alpha, such as
`content/materials/decals/bullet.keromat`. The newest `r_decals` decals are
kept (default 256) and the oldest are dropped. `r_cleardecals` removes them
all, and a new map starts clean.

Decals land on the static world only, not on doors or props. A decal placed
while part of the level is streamed out does not appear on that part.

## Console

| | |
|---|---|
| `ui_reload` | Load every layout, style, script and image again |
| `ui_show <layer> <file>` `ui_hide <layer>` `ui_toggle <layer> [file]` | |
| `ui_set <key> <value>` `ui_emit <event> [data]` | Publish by hand, to test a HUD without playing |
| `ui_dump` | Every published value, every layer, and what is under the pointer |
| `ui_debug 1` | Outline every panel |
| `ui_hotreload 0` | Stop watching the files |
| `ui_hud` `ui_pausemenu` | The HUD and pause menu layouts; empty for none |
| `decal` `r_decals` `r_cleardecals` | |

## Weapons and the dash

The stock game's weapons exist so the HUD has something real to show. They are
a starting point, not a combat system. Keys: **1** **2** **3** select the
pistol, shotgun or rifle, **Q** switches to the last weapon, **R** reloads,
**mouse1** fires, and **mouse2** dashes. The commands behind them are
`slot1`–`slot3`, `lastinv`, `invnext`, `invprev`, `reload` and `+ability1`.
Shots trace from the eye and leave bullet holes. Nothing takes damage, because
nothing in the engine has health but the player. A game with real weapons
replaces `kerosene::game::Stock` rather than extending it. See
`crates/kerosene-game/src/weapons.rs`.

## Using it from a game

The engine owns the UI. A game only publishes state and listens:

```rust
impl Game for MyGame {
    fn tick(&mut self, engine: &mut Engine, _: &InputState, _: f32) {
        engine.ui_set("score.kills", self.kills);
    }
    fn ui_event(&mut self, engine: &mut Engine, name: &str, data: &str, source: &str) {
        if name == "buy" {
            // a shop menu's button: data is the item
        }
    }
}
```

`Game::ui`, egui, still works and still draws on top. It remains the quickest
way to put a debug window on screen.
