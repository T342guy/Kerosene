# Shipping checklist

In the order you will do them. Each item links to the page that explains it.

## Before the build

- [ ] `startmap` in `.keroproj` names the map the game should open on.
- [ ] `game` in `.keroproj` names your Cargo package — or is deliberately
      absent, because you are shipping a content-only project on the stock
      runtime. ([Making a game](making-a-game.md#the-project-file))
- [ ] `kerosene-tools kiln --dry-run` lists every stage you expect and no
      map you have forgotten.
- [ ] `kerosene-tools kiln` runs clean: no leaks reported at the end, no
      missing texture or model. A leaking map compiles anyway, so read the
      summary.
- [ ] You have played every map from the compiled `.vault`, not from loose
      files — loose files shadow the archive, and a file you forgot to add
      will look fine on your machine and be missing on the player's.

## The build

- [ ] `kerosene-tools kiln --ship dist` succeeds. If it refuses, it says why:
      a stale archive means run `kiln` again first.
      ([Publishing](publishing.md#how-kiln---ship-helps))
- [ ] `dist/` holds the game binary, `<name>.keroproj`, `content/<name>.vault`,
      `LICENSE`, `LICENSE-EXCEPTION` and `README.txt` — and **no**
      `kerosene-tools` binary.
- [ ] `README.txt`, "Engine source": replaced the instruction with the URL or
      offer where the Kerosene source you built against can be obtained.
      Nothing does this for you.
- [ ] The game shows an attribution screen when it starts — "Built with
      Kerosene", before or with the first interactive screen — that a player
      cannot switch off. The engine does not draw one for you yet; a few
      lines in `Game::ui` do it.
      ([Publishing](publishing.md#requirements))
- [ ] If you changed any Kerosene file: the whole modified engine is
      published where anyone can get it, and the `README.txt` and the
      attribution screen say "modified from Kerosene" with the version or
      commit you diverged from and where the modified source is. Better: the
      change is a pull request upstream.
      ([Publishing](publishing.md#forks-and-derived-engines))
- [ ] If the engine you built on carries a Kerosene-derived name: the same,
      plus its source is published whether or not your game is closed.
      ([Publishing](publishing.md#the-naming-policy))
- [ ] The engine commit you built against is tagged or written down, so the
      source pointer means something in a year.

## If it ships on Steam

- [ ] `steam_appid` in `.keroproj` is your app id, not 480.
- [ ] Every achievement and stat the game awards is declared in the
      `.keroproj` *and* set up under the same API name in Steamworks.
      `achievement_list` in the console shows what the game declares.
      ([Steam](steam.md#the-project-file))
- [ ] `kerosene-tools kiln --ship dist --steam` succeeds, and `dist/` holds
      Valve's `libsteam_api.so` / `steam_api64.dll` / `libsteam_api.dylib`
      beside the game.
- [ ] No `steam_appid.txt` is in what you upload. `--steam-dev` writes one
      for testing, and the depot script excludes it anyway.
- [ ] Started from Steam, `platform_status` says `steam (connected)`, and
      Shift+Tab pauses the game.

## Before it goes out

- [ ] Run `dist/<name>` from a *different* working directory. It must find
      its own content.
- [ ] Run it on a machine that is not yours, ideally one that has never had
      a Rust toolchain or a Kerosene checkout on it.
- [ ] Run it once from a read-only location and confirm it still starts —
      `engine.kconfig` cannot be written there and the defaults must do.
- [ ] If the target is Windows or macOS: CI builds it; you are still the
      first to *run* it. Test there before announcing it.
      ([Platforms](platforms.md))
- [ ] Your store page says the things the engine cannot do yet and your game
      therefore does not: no gamepad, unless you built it. ([Publishing](publishing.md#you-can-but))
