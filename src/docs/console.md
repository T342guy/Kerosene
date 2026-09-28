# The console

The developer console opens with the key left of `1` (`` ` ``, `~`). It runs
commands and sets convars the way Source's does, and so does everything else
that runs text: a key binding, `autoexec.cfg`, a `+command` on the command
line, a layout's `command()`, a map's script.

- `name` prints a convar and its help; `name value` sets it.
- `;` separates commands, `//` starts a comment, quotes keep a phrase
  together.
- Tab completes a command's name, and then its argument where that is a map,
  a save, a sound, a class or a convar. Pressing Tab again cycles.
- Up and down walk history, which is kept between sessions in
  `cfg/console_history.txt` beside `config.cfg`.
- `find <text>` searches names and help; `help <name>` explains one.

## Cheats

A command or convar marked *cheat* works only with `sv_cheats 1`. Turning
`sv_cheats` off puts every cheat convar back to its default, so a `noclip`
left on does not outlive the permission for it. `toggle`, `incrementvar`
and `revert` respect the flag like typing does.

## Developer commands

| Command | Does | Cheat |
|---|---|---|
| `god` | Take no damage | yes |
| `buddha` | Take damage, never below 1 health | yes |
| `notarget` | Go unnoticed; for a game's AI to read (`Engine::notarget`) | yes |
| `noclip` | Fly through walls | yes |
| `kill` | Die, god mode or not | |
| `give <class> [key value]...` | Spawn an entity where you stand | yes |
| `getpos` | Print where you are as a `setpos; setang` line | |
| `setpos <x> <y> <z>` | Move there | yes |
| `setang <pitch> <yaw> [roll]` | Face that way | yes |
| `ent_fire <target> <input> [param] [delay]` | Fire an input at every entity with that name, else that class; `!picker` is what you are looking at | yes |
| `ent_create <class> [key value]...` | Spawn an entity where you are looking | yes |
| `ent_remove [name]` | Remove what you are looking at, or everything with a name | yes |
| `ent_info [name]` | Print an entity's class, name, place, keys and outputs | |
| `host_timescale <n>` | Run the world `n` times as fast, 0.01 to 10 | yes |
| `phys_spawn [model]` | Drop a physics prop in front of you | yes |
| `restart` | Load the current map again from the start | |
| `maps [filter]` | List the maps there are | |
| `map <name>` | Load a map | |
| `save <name>`, `load <name>` | Save and load a game | |
| `screenshot` | Save the next frame under `screenshots/` in the player's directory | |
| `condump [name]` | Write the scrollback to a file in the player's directory | |
| `revert <convar>` | Put a convar back to its default | |
| `host_writeconfig` | Write `config.cfg` now rather than on exit | |
| `map_autoreload 1` | Reload the map, keeping your place, when it is rebuilt (`play --watch` sets it) | |

## Video

`r_fullscreen` is 0 for a window, 1 for borderless fullscreen and 2 for
exclusive fullscreen at the monitor's own mode. It is saved like the other
options, and the options menu has a switch for it. `cl_fov` takes 50 to 130.

## The command line

```text
game +map arena +sv_cheats 1     # any +command runs after config.cfg
game -w 1280 -h 720              # the window's size, over engine.kcfg's
game -fullscreen                 # or -windowed, over the saved r_fullscreen
game --headless 600 +map arena   # simulate without a window, then report
game --portable                  # keep saves and settings beside the content
```

`--help` (or `-h` on its own) lists the rest.

## Where things are written

Everything the console writes -- `config.cfg`, the history, screenshots,
`condump` -- goes into the player's own directory (`~/.local/share/<game>`,
`%APPDATA%\<game>`, `~/Library/Application Support/<game>`), or with
`--portable` into the content tree. Paths are virtual: nothing a command is
given can reach outside that directory.
