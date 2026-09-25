# The store (Steam)

How the Steam integration is put together, for someone changing it. For
using it, see [Steam](../gamedev/steam.md). When a note and the code
disagree, the code wins.

## Where it lives

| | |
|---|---|
| `crates/kerosene-platform/src/lib.rs` | `Platform`: declarations, validation, stat batching, the event queue. `PlatformAction` and its one-line text form, `PlatformEvent`, `PlatformView` |
| `crates/kerosene-platform/src/null.rs` | The offline stand-in. It behaves like a store: it simulates leaderboard ranks, and keeps "cloud" files in memory or a directory |
| `crates/kerosene-platform/src/steam.rs` | `SteamBackend`, behind the `steam` feature |
| `crates/kerosene-platform/src/script.rs` | The Rhai `platform`/`steam` object, registered by both script VMs |
| `crates/kerosene-engine/src/platform.rs` | Entity requests, result outputs, console commands, publishing to the UI store |
| `crates/kerosene-game/src/platform.rs` | The five `logic_*` classes |
| `tools/kiln/src/steam.rs` | `--ship --steam`: build options, finding the redistributable, SteamPipe scripts |
| `tools/kerosene-tools/src/workshop.rs` | `workshop upload` |

## One path in, one path out

Everything becomes a `PlatformAction` and goes through `Platform::apply`:

- entity requests (`host_requests::PLATFORM`, whose payload is the action's
  text form)
- `ScriptAction::Platform` and `UiAction::Platform`
- console commands
- `Engine::platform_apply`

`apply` checks declared ids, turns `Progress` at its maximum into `Unlock`,
rounds integer stats, and queues whatever events it can report synchronously.
Asynchronous results, such as leaderboards and the overlay, arrive through
`Backend::frame`.

`Engine::dispatch_platform_events` then hands each event to four listeners,
in order:

1. **Entities.** Every entity of the matching class that names the same id,
   stat or board, not only the one that asked, so nothing has to remember a
   caller across an asynchronous call.
2. **The UI store.** The `platform.*` keys are republished and the event is
   emitted.
3. **The map script's `on_platform_event`.**
4. **`Game::platform_event`.**

A handler may cause more events; for example, a stat threshold unlocks an
achievement. Dispatch loops, capped at 16 rounds.

Output parameters: in this engine an output's value *replaces* a wire's own
parameter. So only the outputs that mean something by it (`OnChanged`,
`OnRankImproved`, `OnSubmitted`, `OnDlc*`) pass one. `OnUnlocked`,
`OnAvailable` and `OnThreshold` keep the wire's.

## Timing

- `Engine::tick` dispatches what the tick caused.
- `Engine::platform_frame` pumps the backend and dispatches. The host calls
  it every frame, before the UI, so Steam's callbacks keep flowing while the
  game is paused under the overlay. Headless runs call it every tick.

## Startup

`Platform::new` runs inside `Engine::with_game`, before the VFS is sealed,
because subscribed Workshop items are more archives to mount.
`launch::config_from` reads the store settings from the project whichever way
the content was found. It sets `use_steam` only for a windowed run without
`--no-steam`. `launch` calls `restart_through_steam` before anything else,
which does nothing in debug builds or with `steam_appid.txt` beside the
executable.

## The Rhai object

`platform` is resolved through `Engine::on_var`, not pushed into a scope. A
Rhai function cannot see its script's top-level scope, and the object has to
work inside `fn award() { ... }`. The resolver runs first, which is why the
names are reserved. A VM can have only one resolver, and nothing else in
either VM sets one.

## Tests

- `crates/kerosene-platform/src/tests.rs` covers `Platform` against the null
  backend and the Rhai object.
- `crates/kerosene-engine/tests/platform.rs` uses a compiled map to cover
  every class's outputs, map scripts, and cheat gating.
- `tools/kiln/src/ship/tests.rs` covers `--steam` ships against a fake
  build directory.
- CI lints the `steam` feature and runs the platform tests with it on. The
  vendored library links without a Steam client.

> Next: [Tools and the build](tools-and-build.md).
