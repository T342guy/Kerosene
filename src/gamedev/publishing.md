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
5. **If you changed Kerosene's own files:** under the MPL arm, publish the
   files you changed under MPL-2.0; under the LGPL arm, publish the modified
   engine under the LGPL *and* keep it replaceable in your game (a shared
   library, or object files for relinking). The project's preference, under
   either: send the change upstream as a pull request and build on the
   updated engine, so the fix exists once.

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
| Fork the engine | Both arms permit it | The obligations in item 5, and you now maintain a fork |
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

## You can, but…

Everything here works, or can be made to work, and each has a cost the
engine does not yet hide. These are the gaps a *release* runs into, in
roughly the order you will hit them; the fuller list is
[Missing features](../docs/missing-features.md).

| What | The drawback | Source |
|---|---|---|
| Ship on Windows or macOS | Every dependency is cross-platform, and nothing has ever been tested there. There is no CI. Budget the time to be the first | README, "Known limits"; `missing-features.md` §13 |
| Ship a game with save/load | There is no game-state serialisation at all — no `serde` in the tree. State does not survive a map transition, let alone a restart | `missing-features.md` §1, §10 |
| Ship with a menu, HUD or options screen | The developer console is the only overlay. There is no game UI layer; egui is used for tools, not gameplay | `missing-features.md` §8 |
| Ship with an options screen for sound | `snd_restart` is the only sound convar. No master, music or effects volume exists to be set | `missing-features.md` §1 |
| Ship with gamepad support | Keyboard and mouse only; no action-map layer, no rebinding UI | `missing-features.md` §9 |
| Ship multiplayer | The simulation runs headless, which is the hard part, but there is no wire protocol, prediction or replication | README; `missing-features.md` §12 |
| Ship with crash reporting | Nothing installs a panic hook. A game that dies takes its backtrace with it, and you will not get the bug report | `missing-features.md` §1, §13 |
| Ship uncompressed textures | `.kerotex` has no block compression, so the download is several times the size it needs to be | `missing-features.md` §2 |
| Ship a modified engine | Legal: the obligations in Requirements item 5. Practical: you maintain a fork, and every upstream fix is a merge. The project asks for a pull request instead | `NOTICE`; `licensing.md` |
| Ship under the LGPL arm | Rust links statically, so LGPL-3.0 §4 means shipping the engine as a replaceable library or your object files for relinking — real work `cargo` does not do for you. The MPL arm exists precisely so nobody has to | `licensing.md`, "Why two licences" |
| Write gameplay in Rhai instead of Rust | The script layer is sandboxed on purpose: it cannot allocate an entity, walk the BSP, open a file or touch the renderer. Level glue, yes; a weapon system, no | `scripting.md` |
| Let players drop loose files beside the `.vault` | The feature that makes modding trivial makes tampering trivial too. There is no signing, no manifest, no integrity check on loose files — only per-entry CRCs *inside* the archive | `tools.md`, Vault; `crates/kerosene-vfs` |
| Install the game somewhere read-only | The first run writes `engine.kconfig` into the content tree beside the binary. In a read-only install directory that write fails and the defaults apply every run | `crates/kerosene-config` |
| Rely on the engine being deterministic (replays, ghosts) | Asserted, not audited: nothing replays a session yet to prove it | `missing-features.md` §4 |
| Ship on Steam | Nothing stops you, and nothing helps: no Steamworks integration, so no achievements, cloud, Workshop or Steam Input. The overlay should work over Vulkan and DX12 without help | [Platforms](platforms.md) |

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
