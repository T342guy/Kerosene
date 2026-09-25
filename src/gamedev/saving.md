# Saving and level changes

Kerosene saves the whole game between two ticks and puts it back exactly: the
same entities with the same handles, the same inputs still on their way, the
player where they stood and moving as they were. A saved game played on for
two hundred ticks ends in the same world as the game that was saved played on
for two hundred ticks; a test holds it to that.

Most games need no code for any of it. A game crate adds two methods if it
keeps state of its own.

## For players

| Key or command | Does |
|---|---|
| **F5**, `quicksave` | Save as `quick` |
| **F9**, `quickload` | Load `quick` |
| `save <name>` | Save under a name: letters, digits, `_`, `-` and `.` |
| `load <name>` | Load one |
| `saves` | List them, newest first |

Saves are files, `save/<name>.kerosave`, in the first writable content
directory: the same place `cfg/config.cfg` goes. They are JSON, so a bug
report can include one and anyone can read it.

A game cannot be saved with no map loaded, or while the player is dead.

## For mappers

Three entities:

| Class | Does |
|---|---|
| `logic_autosave` | A checkpoint. Its `Save` input saves under `savename` (`auto` by default). Wire a `trigger_once` to it. |
| `trigger_changelevel` | Walk in, and the player moves to `map`, keeping their health and whatever the game carries. Also has a `ChangeLevel` input, for a level change something else decides on. |
| `info_landmark` | A named point the two maps share. |

**Landmarks.** Put an `info_landmark` with the same name at the same spot,
relative to the geometry either side of the seam, in both maps, and name it
in the `trigger_changelevel`'s `landmark`. The player arrives exactly where
they stood relative to it, moving and facing as they were, so a corridor that
crosses the seam is walked straight through. Without one, they start at the
next map's `info_player_start`.

With `sv_autosave 1`, the default, every level change saves as `auto` on
arrival.

From the console or a script: `changelevel <map> [landmark]`.

### What a save keeps

Everything in an entity's fields, wires and I/O queue, which is everything
the stock classes know:

- a door half open stays half open, and finishes opening
- a `math_counter` keeps its count
- a wire that fires once, and has, stays spent
- a `logic_auto` does not fire again
- an output fired with a delay still arrives on time

It also keeps:

- the player, with their velocity and view
- each physics prop's velocity
- the placed decals
- the UI store and the layers that were showing
- the map script's variables, meaning every top-level variable that holds
  plain data: numbers, text, booleans, arrays and maps

A variable holding a function or an engine object is not saved. It comes back
as whatever the script's top level sets it to.

A save of a map that has since been recompiled still loads, with a warning,
because the entities it holds may no longer match the geometry.

## For game code

The engine owns the level. Your `Game` owns everything else, and saves it with
two methods:

```rust
impl Game for MyGame {
    fn save(&mut self, _: &mut Engine) -> kerosene::serde_json::Value {
        kerosene::serde_json::json!({ "inventory": self.inventory })
    }

    fn load(&mut self, engine: &mut Engine, data: &kerosene::serde_json::Value) {
        self.inventory = data["inventory"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
}
```

- `save` is called for every saved game and every level change.
- `load` is called after the map has loaded and after `map_loaded`, so it
  overrides whatever a fresh map set up. It is not called when `save`
  returned `null`.

The stock game saves its weapons this way: rounds, the weapon in hand and the
dash cooldown.

From code:

| Call | When |
|---|---|
| `engine.save_game(name)` | Saves now |
| `engine.request_load(name)` | Loads at the start of the next frame. From a hook, use this. |
| `engine.load_game(name)` | Loads now. Not from a hook. |
| `engine.change_level(map, landmark)` | Changes level at the start of the next frame |
| `engine.list_saves()` | Lists the saves |
| `engine.read_save(name)` | Reads a save without loading it |

A class whose state lives outside its fields registers `on_restore`. It runs
instead of the spawn handler when a save puts the entity back.
`ambient_generic` uses it to ask for its looping sound again:

```rust
ClassDef::new("ambient_generic").on_spawn(spawn).on_restore(restore)
```

The UI hears `game_saved`, `game_loaded` and `level_changed` as events, and
the name of the last save is in `save.last`.

## Cloud saves

When the store has a cloud, as Steam does, every save is also written there.
On load, whichever copy is newer wins, the local file or the cloud's, so a
save follows the player to another machine. A local file that is missing is
fetched from the cloud. Off Steam there is no cloud, and saves are only local.

Steam Auto-Cloud works as well: point it at the `save/` directory.
