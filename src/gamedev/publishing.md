# Publishing

What has to be true before a game built on Kerosene goes to other people:
what the licences require, what you may do, what you may not, and what you
technically can do today but will pay for. The legal reasoning lives in
[Licensing](../docs/licensing.md) and the technical gaps in
[Missing features](../docs/missing-features.md); this page is the two of
them condensed to the questions a release actually raises.

> [!NOTE]
> None of this is legal advice. It is an accurate account of what the
> licences in this repository say and what the code does, so that you or an
> actual lawyer can move quickly.

## Requirements

Kerosene is dual-licensed, **LGPL-3.0-or-later OR MPL-2.0**, and you ship
under *one* of them. Nearly everyone shipping a game takes the MPL arm,
because it has no linking clause: a closed-source game statically linked to
the engine owes nothing beyond a notice and a pointer to the source. Under
either arm, a distribution must carry:

1. **The licence texts.** `LICENSE-LGPL-3.0` and `LICENSE-MPL-2.0`, in full,
   beside the game.
2. **A notice** that the program is built with Kerosene, which licence it is
   offered under, and that it comes with no warranty.
3. **A pointer to the corresponding engine source.** The obligation travels
   with the copy, so it has to be in the copy: a URL, or an offer, in the
   `README.txt`.
4. **Third-party notices for what travels inside the binary.** Two things
   do: `smartstring` (MPL-2.0, pulled in by the script engine) and the
   typefaces egui embeds (SIL OFL 1.1 and Ubuntu Font Licence 1.0). Both are
   satisfied by preserving their notices.
5. **If you changed Kerosene's own files:** what you owe depends on the
   arm you took and on what you call the result. See
   [Forks and derived engines](#forks-and-derived-engines).

`kiln --ship` writes 1, 2 and 4 for you and leaves a labelled space for 3.
Item 5 is yours alone; no tool can know whether the engine you built against
is the one in the repository.

## You can

| What | Why | What it costs you |
|---|---|---|
| Sell the game | Neither arm restricts commercial use | Nothing |
| Keep your game code, levels, assets and scripts closed | MPL-2.0 copyleft is *per file* and reaches only Kerosene's own files; the LGPL reaches only the engine | Nothing under MPL. Under LGPL, the engine part must stay replaceable |
| Statically link the engine | Rust does this by default and the MPL arm has no linking stage | Nothing under MPL. Under LGPL this is the relinking obligation — see below |
| Ship on any storefront | No storefront term conflicts with either arm | The storefront's own requirements, none of which the engine helps with yet ([Platforms](platforms.md)) |
| Ship the stock `kerosene` runtime, unmodified, as your game | A content-only project does exactly this; `kiln --ship` copies the runtime when `.keroproj` names no `game` package | The notice and licence texts, which `kiln --ship` writes |
| Fork the engine | Both arms permit it | What the arm you took asks for, and you now maintain a fork ([Forks and derived engines](#forks-and-derived-engines)) |
| Call your fork *Kerosene: Something* | The project wants derived engines to be able to say where they came from | The naming policy: the whole engine published as source, a link to yours and a "built from" link to this one |
| Ship without worrying about Valve or id content | There is none. Every format is Kerosene's own, byte-tagged, and cannot open theirs | Nothing, provided you do not put any of theirs in |
| Let players mod the shipped game | Loose files under `content/` shadow packed ones in the `.vault`, deliberately (`crates/kerosene-vfs`) | It is also a tamper vector — see the table below |

## You cannot

| What | Why | Where it is enforced |
|---|---|---|
| Omit the licence texts or the notice | Both arms require them, and nothing breaks when they are missing, which is why it is the common mistake | `kiln --ship` writes them unconditionally; deleting them afterwards is on you |
| Remove "built with Kerosene" and the source pointer | It is the notice both licences ask of a program carrying their code | As above |
| Ship `kerosene-tools` — Chisel, the compilers, Kiln — inside your distribution without distributing its source | The tools are ordinary copyleft binaries. A game that ships no compilers owes nobody anything for them; one that does inherits their obligations | `tools/kiln/src/ship.rs` copies a *named list* of files, never a directory, and a test asserts no tool is ever in the result |
| Relicense Kerosene's files under your own terms | Neither arm permits it; only the copyright holder can | — |
| Present the game as Valve's, or use "Source", "Hammer" or Valve marks as branding | Those names appear in these docs nominatively — to say what a thing is analogous to. Branding is a different use and is theirs | `NOTICE`; the shipped `README.txt` disclaims affiliation |
| Ship a `.vault` older than the content in it | You would be shipping maps nobody built | `check_archive` in `ship.rs` refuses and lists the newer files |
| Ship from a checkout that has not built the archive | There is nothing to copy | `ship.rs` refuses with "run kiln with no `--only` first" |
| Expect a warranty | Both licences disclaim one; the `README.txt` says so | — |
| Ship an engine *named after Kerosene* with its source closed or its origin unstated | The licences govern the code; the name is a separate permission, and the project grants it on conditions | The naming policy in [Forks and derived engines](#forks-and-derived-engines). Use another name and only the licence applies |

## You can, but…

Everything here works, or can be made to work, and each has a cost the
engine does not yet hide. These are the gaps a *release* runs into, in
roughly the order you will hit them; the fuller list is
[Missing features](../docs/missing-features.md).

| What | The drawback | Source |
|---|---|---|
| Ship on Windows or macOS | CI builds both (`.github/workflows/ci.yml`), so they compile. Nobody has *run* the result by hand. Budget the time to be the first | README, "Known limits"; `missing-features.md` §13 |
| Ship a game with save/load | There is no game-state serialisation at all — no `serde` in the tree. State does not survive a map transition, let alone a restart | `missing-features.md` §1, §10 |
| Ship with a menu, HUD or options screen | The developer console is the only overlay. There is no game UI layer; egui is used for tools, not gameplay | `missing-features.md` §8 |
| Ship with an options screen for sound | `volume` is the only sound convar. There is no separate music or effects volume to set | `missing-features.md` §1 |
| Ship with gamepad support | Keyboard and mouse only; no action-map layer, no rebinding UI | `missing-features.md` §9 |
| Ship multiplayer | The simulation runs headless, which is the hard part, but there is no wire protocol, prediction or replication | README; `missing-features.md` §12 |
| Ship with crash reporting | `kerosene_console::install_crash_handler` writes `crash.log` beside the binary with the panic, a backtrace and the last 64 log lines. Nothing sends it anywhere: the player has to find it and mail it to you | `crates/kerosene-console/src/logging.rs`; `missing-features.md` §13 |
| Ship uncompressed textures | `.kerotex` has no block compression, so the download is several times the size it needs to be | `missing-features.md` §2 |
| Ship a modified engine | Legal: what your licence arm asks, plus the naming policy if you call it Kerosene. Practical: you maintain a fork, and every upstream fix is a merge. The project asks for a pull request first | [Forks and derived engines](#forks-and-derived-engines) |
| Ship under the LGPL arm | Rust links statically, so LGPL-3.0 §4 means shipping the engine as a replaceable library or your object files for relinking — real work `cargo` does not do for you. The MPL arm exists precisely so nobody has to | `licensing.md`, "Why two licences" |
| Write gameplay in Rhai instead of Rust | The script layer is sandboxed on purpose: it cannot allocate an entity, walk the BSP, open a file or touch the renderer. Level glue, yes; a weapon system, no | `scripting.md` |
| Let players drop loose files beside the `.vault` | The feature that makes modding trivial makes tampering trivial too. There is no signing, no manifest, no integrity check on loose files — only per-entry CRCs *inside* the archive | `tools.md`, Vault; `crates/kerosene-vfs` |
| Install the game somewhere read-only | The first run writes `engine.kconfig` into the content tree beside the binary. In a read-only install directory that write fails and the defaults apply every run | `crates/kerosene-config` |
| Rely on the engine being deterministic (replays, ghosts) | Asserted, not audited: nothing replays a session yet to prove it | `missing-features.md` §4 |
| Ship on Steam | Nothing stops you, and nothing helps: no Steamworks integration, so no achievements, cloud, Workshop or Steam Input. The overlay should work over Vulkan and DX12 without help | [Platforms](platforms.md) |

## Forks and derived engines

There are three ways to change the engine, and they are not equal.

**A pull request.** The cheapest by far, and the one the project asks for
first. Send the change upstream, build on the updated engine, and the fix
exists once — reviewed, tested, and maintained by someone other than you.
Every other option below means you carry it.

**A private fork.** You changed something, you ship it inside your own game,
and you never name the result. What you owe is exactly what the arm you took
says and nothing more: under MPL-2.0, the *files you changed*, as source,
under MPL-2.0; under the LGPL, the modified engine as a whole, under the
LGPL, kept replaceable in your game. That is the whole of it.

**A derived engine.** The Source lineage has a precedent for this: Respawn
took Source, rewrote large parts of it, and shipped Titanfall on the result
— a branch that was recognisably Source and recognisably not. Kerosene would
like to be forked that way. If you rewrite the renderer, or the physics, or
half of everything, and other people are going to build games on what you
made, that is a derived engine, and you are welcome to name it after this
one so its lineage is plain: *Kerosene: Ultimate*, *Kerosene NG*, whatever
fits.

### The naming policy

The licences govern the code. The name is a separate permission, and the
project grants it on four conditions. Call your engine something that a
reasonable person would take for a Kerosene branch — a name that contains
"Kerosene" or is plainly derived from it — and you accept these:

1. **Publish the whole modified engine as source**, under the same dual
   licence or the arm you took — not only the files you changed. A
   Kerosene-named engine is open the way Kerosene is open.
2. **Link to your engine's source** in its own README or NOTICE, and in the
   `README.txt` of every game built on it, so a player of that game can find
   the engine it runs on.
3. **Link to Kerosene's source** in the same places, with the words
   *"built from Kerosene"* or *"modified from Kerosene"*, and the version or
   commit you diverged from. That is the pointer this project's own notice
   asks for, carried one step further.
4. **Say it is a derivative.** Do not present it as the official Kerosene or
   as endorsed by this project; a line in the README saying it is an
   independent fork is enough.

Use a name that is not Kerosene's and none of this applies: the policy is the
price of the name, not of forking. Forking is free under either arm, and the
project would rather see a well-named derivative than a closed one with a
different label.

What stays the same whichever way you go: the two licence texts ship, the
notices ship, `kiln --ship` still writes them, and no Valve or id mark
becomes part of anybody's branding.

## How `kiln --ship` helps

```sh
kerosene-tools kiln --ship dist
```

builds the content if it is stale, builds the `game` package if the project
names one, and assembles:

```text
dist/
  my_game            the game, or the engine runtime when the project names none
  my_game.keroproj   content = "content", so the game finds its own archive
  content/
    my_game.vault
  LICENSE-LGPL-3.0   one arm of the licence, full text
  LICENSE-MPL-2.0    the other arm, full text
  README.txt         what this is, and the notices both licences ask for
```

Three properties are worth knowing:

* **It copies a named list, not a directory.** Nothing from `target/` sweeps
  in by accident, and in particular no tool does.
* **It refuses rather than warns.** A missing archive, or one older than any
  file in the content tree, stops the ship stage. Run `kiln` again first.
* **The licence texts are compiled into `kiln`**, so shipping works from an
  installed toolset that is nowhere near a checkout.

What it leaves for you: the `README.txt` has an **Engine source** section
that says the source must be available and asks you to say where. Fill it
in. That, and testing the result on a machine that is not yours — see the
[Shipping checklist](shipping-checklist.md).
