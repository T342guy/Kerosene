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
      `LICENSE-LGPL-3.0`, `LICENSE-MPL-2.0` and `README.txt` — and **no**
      `kerosene-tools` binary.
- [ ] `README.txt`, "Engine source": replaced the instruction with the URL or
      offer where the Kerosene source you built against can be obtained.
      Nothing does this for you.
- [ ] If you changed any Kerosene file: the changed files (MPL arm) or the
      modified engine (LGPL arm) are published, and the README says where.
      Better: the change is a pull request upstream.
      ([Publishing](publishing.md#requirements))
- [ ] The engine commit you built against is tagged or written down, so the
      source pointer means something in a year.

## Before it goes out

- [ ] Run `dist/<name>` from a *different* working directory. It must find
      its own content.
- [ ] Run it on a machine that is not yours, ideally one that has never had
      a Rust toolchain or a Kerosene checkout on it.
- [ ] Run it once from a read-only location and confirm it still starts —
      `engine.kconfig` cannot be written there and the defaults must do.
- [ ] If the target is Windows or macOS: you are the first. Test there
      before announcing it. ([Platforms](platforms.md))
- [ ] Your store page says the things the engine cannot do yet and your game
      therefore does not: no save/load, no gamepad, no options menu, unless
      you built them. ([Publishing](publishing.md#you-can-but))
