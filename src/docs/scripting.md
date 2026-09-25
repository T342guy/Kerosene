# Scripting

Entity outputs wired to inputs compose further than they have any right to,
and most of a level is built that way. But some things are not a graph.
Counting, arithmetic, "pick one of these three at random", "only if the player
still has the crowbar" — expressing those as relays and counters is possible
and miserable. This is the layer above.

It is deliberately *not* a second way to write the engine. A script cannot
allocate an entity slot, walk the BSP tree, open a file, or touch the
renderer. It reads a snapshot of the world and returns a list of things it
would like done.

## Running one

Three ways in, all equivalent underneath:

```
script find_by_name("gate").fire("Open")     # at the console (cheat-protected)
script_execute mymap                          # load scripts/mymap.keroscript
script_reload                                 # forget everything and load it again
```

A map called `atrium` automatically loads `scripts/atrium.keroscript` when it
starts. That is where a level's script belongs.

From inside a level, a `logic_script` entity is the seam: an output fires
`CallScriptFunction` and a function in the script runs.

```
| Key | |
|---|---|
| `scriptfile` | Loaded when the map starts |
| `function`   | What `CallScriptFunction` runs with no parameter |
| `code`       | What `RunScriptCode` runs with no parameter |
```

## Hooks

Define these and the engine calls them.

| Function | When |
|---|---|
| `on_map_start()` | Once, after every entity in the map has spawned |
| `on_tick(dt)` | Every tick, with the tick length in seconds |
| `on_platform_event(name, data)` | The store reported something: `achievement_unlocked`, `score_submitted`, `overlay_opened`, ... See [Steam](../gamedev/steam.md#from-a-script-platform-or-steam) |

A function called through `CallScriptFunction` may take the caller's name, or
take nothing — both spellings work, so the unused parameter is never forced on
you.

```rhai
fn on_used(who) {
    print(`used by ${who}`);
}
```

## The API

The language is [Rhai](https://rhai.rs). Everything below is the whole of it —
a scripting surface nobody can read in one sitting is one nobody can audit.

### Finding things

| | |
|---|---|
| `find_by_name(name)` | One entity, or `()` if there is none |
| `find_all_by_name(name)` | Every entity with that name — several may share one |
| `find_by_class(class)` | Every entity of a class |
| `player()` | The local player, or `()` |
| `entity_count()` | How many entities exist |

Asking about something that is not there gives `()` rather than throwing:
checking whether a thing exists is not a mistake.

### Entities

| | |
|---|---|
| `e.id` `e.classname` `e.targetname` `e.origin` | Read-only |
| `e.get(key)` `e.get_float(key)` `e.has(key)` | Keyvalues |
| `e.set(key, value)` | Set a keyvalue; value may be text or a number |
| `e.set_origin(vector)` | Move it |
| `e.fire(input)` | Fire one of its inputs |
| `e.kill()` | Remove it |

`e.fire` on an entity with no `targetname` addresses *that* entity and no
other. Without that, acting on one of a dozen unnamed lights would mean all of
them, or none.

### Entity I/O

```rhai
ent_fire("gate", "Open");
ent_fire("gate", "Open", "");           // with a parameter
ent_fire("counter", "SetValue", "3", 1.5);  // ...and a delay in seconds
```

Firing goes through the same event queue an output wired in Chisel uses, so
the ordering and the delays are the ones the rest of the level plays by.

### The console and the world

| | |
|---|---|
| `command(text)` | Run console text, exactly as typed |
| `cvar(name)` `cvar_float(name)` | Read a convar |
| `set_cvar(name, value)` | Set one — goes through the console, so cheat flags still apply |
| `time()` `tick()` `map_name()` | Where and when you are |
| `print(x)` `warn(x)` `error(x)` | Console output, with severity |

### The game UI

| | |
|---|---|
| `ui_set(key, value)` | Publish a value layouts can bind to: `ui_set("objective.text", "Find the key")` |
| `ui_event(name)` `ui_event(name, data)` | An event every layout hears |
| `ui_show(layer, file)` `ui_hide(layer)` | Show or hide a layout |
| `place_decal(material, origin, normal, size)` | Project a decal |

See [Game UI](ui.md).

### The store: `platform` (or `steam`)

Steam when the game is built with it, a stand-in otherwise. The same calls
work on both.

| | |
|---|---|
| `.available` `.name` `.user` `.language` `.overlay` | Read |
| `.is_unlocked(id)` `.stat(name)` `.owns_dlc(appid)` `.achievements` `.stats` | Read |
| `.unlock(id)` `.progress(id, current, max)` `.clear(id)` | Achievements; `clear` is for testing |
| `.set_stat(name, v)` `.add_stat(name)` `.add_stat(name, d)` `.store_stats()` | Stats |
| `.submit_score(board, score)` `.submit_score(board, score, lower_is_better)` | Leaderboards; the result comes to `on_platform_event` |
| `.presence(key, value)` `.clear_presence()` | Rich presence |
| `.open_overlay(dialog)` `.open_url(url)` `.open_store()` `.open_store(appid)` `.check_dlc(appid)` | |

Achievement ids and stat names must be declared in the `.keroproj`. The
object works inside functions, unlike top-level variables, and so `platform`
and `steam` cannot be used as variable names. See
[Steam](../gamedev/steam.md).

### Vectors

```rhai
let a = Vector(0.0, 0.0, 64.0);
let b = player().origin;
print(`${distance(a, b)}`);
```

`+` `-` `*`, `.x` `.y` `.z`, `length`, `distance`, `dot`, `normalize`,
`to_string`. Distances are kerosene units; see the units section of the README.

## What a script cannot do

* **Reach the filesystem.** There is no module resolver, so `import` fails.
  It would have been a way around every limit below.
* **Run forever.** Operations, call depth, string and array sizes are all
  bounded. A level's scripts are content, edited by people who make mistakes,
  and an infinite loop should stop the script rather than the game.
* **Queue unbounded work.** A runaway loop calling `ent_fire` is cut off, the
  same way the engine refuses to dispatch entity I/O forever.
* **See the world mid-tick.** A script reads a snapshot taken when it starts
  and its effects are applied when it finishes, so a run is a pure function of
  the world at one instant. That is why the same script run twice on the same
  world does the same thing.
* **Hold a reference across a death.** An entity handle carries its slot's
  generation, so a handle kept over a respawn cannot come back pointing at
  whatever moved into the slot.

## Why a snapshot and a queue

The obvious design hands the script a live reference to the entity world. It
cannot be done safely — script functions outlive the call that registered
them, so the borrow would have to be `'static` — and it should not be done
anyway. A script that mutates the world halfway through a frame can observe
the world in a state no other code ever sees, and that is where the
hard-to-reproduce bugs live.

## Cheat protection

`script`, `script_execute` and `script_reload` are cheat-protected: they can
move entities and set convars, which is not something to hand out for free.
A map's own script loads without `sv_cheats`, because shipping a level means
shipping its script.

## An example

`content/scripts/kero_start.keroscript` is the sample map's, written to be
read.
