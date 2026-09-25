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

Kerosene is licensed **GPL-3.0-or-later with the Kerosene Exception**. The
exception is what lets a closed-source game link the engine at all: it is an
additional permission under GPL §7, granted on conditions, and shipping a
game means meeting them. A distribution must carry:

1. **The licence texts.** `LICENSE` (the GPL) and `LICENSE-EXCEPTION` (the
   Kerosene Exception), in full, beside the game.
2. **A notice** that the program is built with Kerosene, what licence the
   engine is under, and that it comes with no warranty.
3. **A pointer to the corresponding engine source.** The obligation travels
   with the copy, so it has to be in the copy: a URL, or an offer, in the
   `README.txt`.
4. **An attribution screen.** Each time the game starts, before or together
   with its first interactive screen, it shows that it is built with
   Kerosene. Brief and dismissable is fine; disabled, hidden or illegible is
   not. A game with no graphical interface prints the line instead.
5. **Third-party notices for what travels inside the binary.** Two things
   do: `smartstring` (MPL-2.0, pulled in by the script engine) and the
   typefaces egui embeds (SIL OFL 1.1 and Ubuntu Font Licence 1.0). Both are
   satisfied by preserving their notices.
6. **If you changed Kerosene's own files:** the whole modified engine stays
   under the GPL with the exception; it says *"modified from Kerosene"*, the
   version or commit it diverged from, and where Kerosene's source is — in
   its notice, in the game's `README.txt` and on the attribution screen; and
   its complete source is published where anyone can obtain it free of
   charge. See [Forks and derived engines](#forks-and-derived-engines).

`kiln --ship` writes 1, 2 and 5 for you and leaves a labelled space for 3
and for the modified-engine line in 6. Item 4 is drawn by your game — the
engine does not draw it for you yet ([Missing
features](../docs/missing-features.md)) — and the rest of 6 is yours alone;
no tool can know whether the engine you built against is the one in the
repository.

## You can

| What | Why | What it costs you |
|---|---|---|
| Sell the game | The licence does not restrict commercial use | Nothing |
| Keep your game code, levels, assets and scripts closed | The exception's linking permission: an independent module does not become a covered work by being linked | The permission's conditions: engine source, the notice, the attribution screen |
| Statically link the engine | Rust does this by default and the exception names static linking explicitly | Nothing more; there is no relinking clause |
| Ship on any storefront | No storefront term conflicts with the licence | The storefront's own requirements, none of which the engine helps with yet ([Platforms](platforms.md)) |
| Ship the stock `kerosene` runtime, unmodified, as your game | A content-only project does exactly this; `kiln --ship` copies the runtime when `.keroproj` names no `game` package | The notice and licence texts, which `kiln --ship` writes |
| Fork the engine | The GPL permits it | The whole fork stays GPL with the exception, says "modified from Kerosene" and the version, and — when a closed game ships on it — publishes its whole source; and you now maintain a fork ([Forks and derived engines](#forks-and-derived-engines)) |
| Call your fork *Kerosene: Something* | The project wants derived engines to be able to say where they came from | The name's price, §2(d) of the exception: the whole engine published as source, a link to yours and a "modified from Kerosene" link to this one |
| Ship without worrying about Valve or id content | There is none. Every format is Kerosene's own, byte-tagged, and cannot open theirs | Nothing, provided you do not put any of theirs in |
| Let players mod the shipped game | Loose files under `content/` shadow packed ones in the `.vault`, deliberately (`crates/kerosene-vfs`) | It is also a tamper vector — see the table below |

## You cannot

| What | Why | Where it is enforced |
|---|---|---|
| Omit the licence texts or the notice | The exception requires them, and nothing breaks when they are missing, which is why it is the common mistake | `kiln --ship` writes them unconditionally; deleting them afterwards is on you |
| Remove "built with Kerosene" and the source pointer | It is a condition of the linking permission and a §7(b) notice; without the permission the whole game is GPL | As above |
| Skip or hide the attribution screen | §1(d) and §2(e) of the exception: it is the author attribution the GPL's own §5(d) has interactive programs display | Nothing enforces it; the [Shipping checklist](shipping-checklist.md) asks |
| Ship a modified engine closed, or without saying it is modified | §1(a) and §1(c): the engine part stays GPL and a modified one is published whole; §2(b): it says "modified from Kerosene". Decline the exception and the entire game is GPL | — |
| Ship `kerosene-tools` — Chisel, the compilers, Kiln — inside your distribution without distributing its source | The tools are ordinary GPL binaries. A game that ships no compilers owes nobody anything for them; one that does inherits their obligations | `tools/kiln/src/ship.rs` copies a *named list* of files, never a directory, and a test asserts no tool is ever in the result |
| Relicense Kerosene's files under your own terms | The GPL does not permit it; only the copyright holder can | — |
| Present the game as Valve's, or use "Source", "Hammer" or Valve marks as branding | Those names appear in these docs nominatively — to say what a thing is analogous to. Branding is a different use and is theirs | `NOTICE`; the shipped `README.txt` disclaims affiliation |
| Ship a `.vault` older than the content in it | You would be shipping maps nobody built | `check_archive` in `ship.rs` refuses and lists the newer files |
| Ship from a checkout that has not built the archive | There is nothing to copy | `ship.rs` refuses with "run kiln with no `--only` first" |
| Expect a warranty | The licence disclaims one; the `README.txt` says so | — |
| Ship an engine *named after Kerosene* with its source closed or its origin unstated | The licence grants no right to the name; §2(d) of the exception grants it on conditions | [Forks and derived engines](#forks-and-derived-engines). Use another name and §2(d) does not apply |

## You can, but…

Everything here works, or can be made to work, and each has a cost the
engine does not yet hide. These are the gaps a *release* runs into, in
roughly the order you will hit them; the fuller list is
[Missing features](../docs/missing-features.md).

| What | The drawback | Source |
|---|---|---|
| Ship on Windows or macOS | CI builds both (`.github/workflows/ci.yml`), so they compile. Nobody has *run* the result by hand. Budget the time to be the first | README, "Known limits"; `missing-features.md` §13 |
| Ship a game with save/load | There is no game-state serialisation at all — no `serde` in the tree. State does not survive a map transition, let alone a restart | `missing-features.md` §1, §10 |
| Ship with a menu, HUD or options screen | `Game::ui` draws egui over the world, so a HUD or menu is yours to write; there is no stock menu, and no stock attribution screen either, so the one the licence requires is a few lines in `ui` for now | `missing-features.md` §8 |
| Ship with an options screen for sound | `volume` is the only sound convar. There is no separate music or effects volume to set | `missing-features.md` §1 |
| Ship with gamepad support | Keyboard and mouse only; no action-map layer, no rebinding UI | `missing-features.md` §9 |
| Ship multiplayer | The simulation runs headless, which is the hard part, but there is no wire protocol, prediction or replication | README; `missing-features.md` §12 |
| Ship with crash reporting | `kerosene_console::install_crash_handler` writes `crash.log` beside the binary with the panic, a backtrace and the last 64 log lines. Nothing sends it anywhere: the player has to find it and mail it to you | `crates/kerosene-console/src/logging.rs`; `missing-features.md` §13 |
| Ship uncompressed textures | `.kerotex` has no block compression, so the download is several times the size it needs to be | `missing-features.md` §2 |
| Ship a modified engine | Legal: the whole fork public, "modified from Kerosene" everywhere the game names itself, plus §2(d) if you call it Kerosene. Practical: you maintain a fork, and every upstream fix is a merge. The project asks for a pull request first | [Forks and derived engines](#forks-and-derived-engines) |
| Write gameplay in Rhai instead of Rust | The script layer is sandboxed on purpose: it cannot allocate an entity, walk the BSP, open a file or touch the renderer. Level glue, yes; a weapon system, no | `scripting.md` |
| Let players drop loose files beside the `.vault` | The feature that makes modding trivial makes tampering trivial too. There is no signing, no manifest, no integrity check on loose files — only per-entry CRCs *inside* the archive | `tools.md`, Vault; `crates/kerosene-vfs` |
| Install the game somewhere read-only | The first run writes `engine.kconfig` into the content tree beside the binary. In a read-only install directory that write fails and the defaults apply every run | `crates/kerosene-config` |
| Rely on the engine being deterministic (replays, ghosts) | Asserted, not audited: nothing replays a session yet to prove it | `missing-features.md` §4 |
| Ship on Steam | Build with the `steam` feature and ship with `kiln --ship --steam`. The drawbacks: your game links Valve's proprietary SDK, which the Exception permits but the GPL alone would not, and there is no Steam Input yet | [Steam](steam.md) |

## Forks and derived engines

There are three ways to change the engine, and they are not equal.

**A pull request.** The cheapest by far, and the one the project asks for
first. Send the change upstream, build on the updated engine, and the fix
exists once — reviewed, tested, and maintained by someone other than you.
Every other option below means you carry it.

**A private fork.** You changed something and you ship it inside your own
game. A modified Kerosene is a modified Kerosene wherever the changed files
live, so what you owe is what the exception says of one:

* the whole modified engine stays under the GPL with the exception (§1(a));
* if the game is closed — that is, if you are using the linking permission —
  the complete source of the modified engine is published where anyone can
  obtain it free of charge, not only offered to your players (§1(c));
* it says *"modified from Kerosene"* (or *"built from Kerosene"*), the
  version or commit it diverged from, and where Kerosene's source is — in the
  engine's own notice, in the game's `README.txt`, and on the game's
  attribution screen (§2(b), §2(e));
* it is not presented as the original or as endorsed by this project.

Decline the exception and you are under the plain GPL: the entire game is
then a covered work, and only then is publication not required — recipients
get the source instead.

**A derived engine.** The Source lineage has a precedent for this: Respawn
took Source, rewrote large parts of it, and shipped Titanfall on the result
— a branch that was recognisably Source and recognisably not. Kerosene would
like to be forked that way. If you rewrite the renderer, or the physics, or
half of everything, and other people are going to build games on what you
made, that is a derived engine, and you are welcome to name it after this
one so its lineage is plain: *Kerosene: Ultimate*, *Kerosene NG*, whatever
fits. Everything a private fork owes, a derived engine owes too; the
attribution-screen requirement is inherited, so every game built on the
fork shows *"built with Kerosene NG, modified from Kerosene"*.

### The naming policy

The licence grants no right to the name. §2(d) of the exception grants it,
on conditions: call your engine something a reasonable person would take for
a Kerosene branch — a name that contains "Kerosene" or is plainly derived
from it — and you accept these:

1. **Publish the whole modified engine as source**, under the GPL with the
   exception — the engine, not only the files you changed — where anyone
   can obtain it free of charge. A Kerosene-named engine is open the way
   Kerosene is open, whether or not a closed game ever ships on it.
2. **Attribute Kerosene** as §2(b) asks: *"modified from Kerosene"*, the
   version or commit you diverged from, and a link to Kerosene's source, in
   your engine's notice and in the notice and attribution screen of every
   game built on it.
3. **Say it is a derivative.** Do not present it as the official Kerosene or
   as endorsed by this project; a line in the README saying it is an
   independent fork is enough.

Use a name that is not Kerosene's and only §2(b) and the linking conditions
apply. The project would rather see a well-named derivative than a closed
one with a different label.

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
  LICENSE            the GNU General Public License, version 3, full text
  LICENSE-EXCEPTION  the Kerosene Exception, full text
  README.txt         what this is, and the notices the licence asks for
```

Three properties are worth knowing:

* **It copies a named list, not a directory.** Nothing from `target/` sweeps
  in by accident, and in particular no tool does.
* **It refuses rather than warns.** A missing archive, or one older than any
  file in the content tree, stops the ship stage. Run `kiln` again first.
* **The licence texts are compiled into `kiln`**, so shipping works from an
  installed toolset that is nowhere near a checkout.
* **`--steam` adds Valve's library and nothing else.** It builds with the
  `steam` feature, puts `libsteam_api` beside the game with an rpath to find
  it, and writes SteamPipe scripts next to `dist/` rather than inside it. See
  [Steam](steam.md#shipping-kiln---ship---steam). On Windows, every ship links
  the C runtime statically, so players need no Visual C++ redistributable.

What it leaves for you: the `README.txt` has an **Engine source** section
that says the source must be available and asks you to say where — and, if
your engine is modified, which version it diverged from and where the whole
modified source is published. Fill it in. That, the attribution screen the
game draws itself, and testing the result on a machine that is not yours —
see the [Shipping checklist](shipping-checklist.md).
