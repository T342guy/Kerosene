# Steam

Kerosene talks to Steam through Valve's Steamworks SDK. It covers:

- achievements, stats and leaderboards
- rich presence
- cloud files
- DLC checks
- the overlay
- the Workshop

The same map, script and game code works off Steam too. Without Steam, the
store is a stand-in that keeps achievements and stats for the session and
answers every request the way Steam would. Nothing needs an `if steam` branch.

## Turning it on

Steam is an **opt-in Cargo feature**. A default build of Kerosene links
nothing that is not free software, and the Steamworks SDK is not. So a Steam
build is one you ask for:

```toml
# your game's Cargo.toml
[dependencies]
kerosene = { ..., features = ["steam"] }        # always Steam

# or, better, as a feature of your own that kiln can turn on:
[features]
steam = ["kerosene/steam"]
```

The stock runtime has the same switch:

```sh
cargo run -p kerosene-runtime --features steam
```

The SDK comes from the [`steamworks`](https://crates.io/crates/steamworks)
crate, whose Rust bindings are MIT/Apache and which carries Valve's headers
and redistributable libraries. You need no C++ toolchain, no bindgen and no
separate SDK download.

## The project file

Everything Steam needs to know goes in the `.keroproj`:

```text
project
{
    "name"        "My Game"
    "startmap"    "mg_intro"

    "steam_appid" "480"          // your app id; 480 is Valve's test app, Spacewar
    "steam_depot" "481"          // optional; the app id plus one by default

    "achievements"
    {
        "ACH_FIRST_DOOR"  "Open the first door"
        "ACH_TEN_DOORS"   "Open ten doors"
    }
    "stats"
    {
        "doors_opened"    "int"
        "distance"        "float"
    }
    "dlc"
    {
        "1234560"         "Soundtrack"
    }
}
```

Achievements and stats are **declared**. An id that is not listed here is
refused, and the console says which id and why. Without that rule, a typo in
a map would be an achievement nobody can ever get. Each id must also be set up
under the same API name in the Steamworks partner site.

## Running it

| | |
|---|---|
| `cargo run --features steam` with the Steam client open | Connects as the signed-in user. `platform_status` in the console says so |
| The same, with Steam closed | Warns once and falls back to the local store. The game runs |
| `--no-steam` | Skips Steam even in a Steam build |
| `--headless` | Never tries Steam: a server has no signed-in player |
| A release build started from its folder rather than from Steam | Relaunches itself through Steam, as Valve requires, unless `steam_appid.txt` sits beside it |

Shift+Tab opens the overlay, and the game pauses under it by opening the
pause menu. The menu stays open after the overlay closes, so the player
returns to a menu rather than straight into play.

## From a map: entity I/O

| Class | Keys | Inputs | Outputs |
|---|---|---|---|
| `logic_achievement` | `achievement`, `progressmax` | `Unlock`, `Clear`, `SetProgress <n> [max]` | `OnUnlocked` |
| `logic_stat` | `stat`, `threshold`, `achievement` | `Set <n>`, `Add [n]`, `Increment`, `Store` | `OnChanged` (the value), `OnThreshold` |
| `logic_leaderboard` | `leaderboard`, `sort` (`desc` for points, `asc` for times) | `Submit <score>` | `OnSubmitted`, `OnRankImproved` (the rank), `OnFailed` |
| `logic_richpresence` | `status` | `SetStatus <text>`, `SetKey <key> <value>`, `Clear` | |
| `logic_platform` | `dlc` | `Refresh`, `OpenOverlay [dialog]`, `OpenUrl <url>`, `OpenStore [appid]`, `CheckDlc [appid]` | `OnAvailable` / `OnUnavailable` at map start, `OnOverlayOpened`, `OnOverlayClosed`, `OnDlcOwned`, `OnDlcNotOwned` |

Results fire on **every** entity of the class that names the same thing, not
only on the one that asked. For example, an achievement a script awards still
fires the `logic_achievement` for it. A leaderboard's answer arrives a moment
later from Valve's servers and lands on the entity for that board.

A `logic_stat` with a `threshold` and an `achievement` does the usual thing
for "open ten doors". It shows Steam's progress toast on the way (3/10, 4/10,
...) and awards the achievement when the stat reaches the threshold.

`kero_start` has one example: typing the code into the keypad fires
`ach_keypad`'s `Unlock`, and the HUD toasts "Code breaker".

## From a script: `platform` (or `steam`)

Map scripts and UI scripts have the same object:

```rhai
platform.unlock("ACH_FIRST_DOOR");
steam.add_stat("doors_opened", 1);          // `steam` is the same object
platform.progress("ACH_TEN_DOORS", 3, 10);
platform.submit_score("atrium_time", 5230, true);   // true: lower is better
platform.presence("status", "In the atrium");
if platform.owns_dlc(1234560) { ... }
platform.open_overlay("achievements");

print(platform.user);                       // the Steam name, or the OS user
if platform.available { ... }               // a store is connected
if platform.is_unlocked("ACH_FIRST_DOOR") { ... }
let doors = platform.stat("doors_opened");
```

The object works inside functions as well as at the top of a file. What comes
back arrives as an event:

- **Map scripts:** `fn on_platform_event(name, data)`.
- **UI layouts:** `on:achievement_unlocked="..."` or `on_event`.

| Event | Data |
|---|---|
| `achievement_unlocked`, `achievement_cleared` | the id |
| `achievement_progress` | `id current max` |
| `stat_changed` | `name value` |
| `score_submitted` | `board score rank improved` |
| `score_failed` | the board |
| `dlc_checked` | `appid owned` |
| `overlay_opened`, `overlay_closed` | |
| `workshop_mounted` | the item id |

UI bindings read the same state from the store:

- `platform.available`, `platform.name`, `platform.user` and `platform.overlay`
- `platform.achievements.<id>`, `platform.stats.<name>` and `platform.dlc.<appid>`
- `platform.names.<id>`, the display names from the project

The stock HUD's achievement toast is built from these.

## From game code

```rust
use kerosene::platform::PlatformAction;

if let Err(why) = engine.platform_apply(&PlatformAction::AddStat { name: "kills".into(), delta: 1.0 }) {
    log::warn!("{why}"); // e.g. the stat is not declared in the .keroproj
}
let user = engine.platform().user();
```

`Game::platform_event` hears every event after the entities and the map
script. `engine.platform_mut().cloud_write(name, bytes)` and `cloud_read` use
Steam Cloud's file API.

## Cloud saves

There are two ways to do it:

- **Steam Auto-Cloud.** Configure it in Steamworks to sync a folder, and no
  code is needed at all.
- **The cloud API.** `cloud_write` / `cloud_read` store named files in the
  game's cloud quota.

Off Steam, the stand-in keeps "cloud" files in memory. Save/load, when it
lands, uses these calls.

## The Workshop

A subscribed Workshop item is a folder. At startup, the engine mounts every
`.vault` in every subscribed, installed item as one more layer of its file
system. A map from the Workshop is found like any other map, and nothing
about the format changes.

To upload one:

```sh
cargo build -p kerosene-tools --features steam
kerosene-tools workshop upload content/my_map.vault --title "My Map" --preview shot.png
kerosene-tools workshop upload content/my_map.vault --item 3141592653 --note "Fixed the leak"
```

The first form makes a new item and prints its id. Pass that id next time to
update the item rather than make another. You need the Steam client running
and signed in to an account that owns the game.

## Shipping: `kiln --ship --steam`

```sh
kerosene-tools kiln --ship dist --steam                 # a build to upload
kerosene-tools kiln --ship dist --steam --steam-dev     # plus steam_appid.txt, to test outside Steam
kerosene-tools kiln --ship dist --steam-upload myacct   # build, then upload with steamcmd
```

On top of an ordinary ship, `--steam` does five things:

1. **Builds with the `steam` feature**, into `target/ship` so the everyday
   build is left alone. A project with no game package gets the stock
   runtime, built from the engine checkout.
2. **Installs Valve's redistributable** (`libsteam_api.so`,
   `steam_api64.dll` or `libsteam_api.dylib`) beside the game. The copy comes
   from the build that linked it, so the versions always match.
3. **Makes it findable.** On Linux the binary gets an rpath of `$ORIGIN`, and
   on macOS of `@executable_path`. Windows looks beside the executable on its
   own. The shipped game starts from any working directory.
4. **Writes the SteamPipe scripts**, `steam_build/app_build_<appid>.vdf` and
   `depot_build_<depot>.vdf`, *beside* `dist/` rather than in it. It then
   prints the `steamcmd` line that uploads them. The depot script excludes
   `steam_appid.txt` and `*.pdb`, so a test file can never reach players.
5. **Says so in `README.txt`**: the library is Valve's, redistributed under
   the Steamworks SDK Access Agreement, and not part of Kerosene.

Kiln never handles credentials. With `--steam-upload`, `steamcmd` asks for
the password and Steam Guard code itself.

On Windows, every ship links the C runtime statically, Steam or not. So a
player needs no Visual C++ redistributable either, and the only
redistributable a Kerosene game carries is Valve's.

## Licensing

The Kerosene Exception lets a game combine the engine with Independent
Modules, and the Steamworks SDK is one. A Steam build of your game is a
Combined Work like any other. The engine part stays GPL with its source
available, and the SDK stays Valve's under Valve's terms.

The Kerosene source tree carries none of the SDK. It arrives through Cargo
only when a game turns the feature on.

## What is not here yet

- Steam Input, and gamepads in general. Under Steam, Steam Input's virtual
  pads appear as ordinary controllers, so they come with gamepad support.
- Lobbies, matchmaking and networking.
- Inventory and microtransactions.
- Downloading or reading leaderboard entries. Posting works; showing a table
  does not yet.
- Workshop browsing from inside the game. The Steam client's Workshop page
  does the subscribing.
