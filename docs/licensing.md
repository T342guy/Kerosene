# Licensing

Kerosene is **LGPL-3.0-or-later**. `COPYING` holds the GPL-3.0 text that the
LGPL builds on, `COPYING.LESSER` holds the LGPL-3.0 text, `Cargo.toml` declares
`license = "LGPL-3.0-or-later"`, and every source file carries an
`SPDX-License-Identifier` line.

Nothing in the dependency tree obstructs that, and every line of Kerosene was
written for this project. Three things are worth understanding anyway: one
licence is *not* available to this project, one clause of the LGPL needs real
care in a Rust project, and one part of the provenance story deserves to be
stated plainly rather than buried.

I am not a lawyer and none of this is legal advice. It is an accurate inventory
plus the reasoning behind the choice, so that you or an actual lawyer can move
quickly.

## What LGPL-3.0 means here

The Lesser GPL is the GPL plus a linking permission. Concretely:

* **Changing Kerosene** — fixing the BSP compiler, adding a shader path,
  altering a format — means publishing those changes under the LGPL too, if you
  distribute the result.
* **Building a game on Kerosene** does not. Your game code, your assets and
  your levels stay yours under whatever terms you like. That is exactly why the
  Lesser GPL exists, and it is the reason to pick it over the plain GPL for an
  engine: the GPL would have reached into every game anyone shipped.
* **The tools** (Chisel, Cleave, Umbra, Radiance, Alchemy, Timbre, Forge,
  Vault, Kiln) are covered too. For a standalone program the LGPL's extra permission simply has
  nothing to bite on, so in practice they behave as GPL-3.0 binaries: ship the
  source if you ship the tool.

### The static-linking clause — read this before shipping a binary

The LGPL's whole mechanism assumes a user who receives your program can replace
the library inside it with their own build. That assumption was written for
dynamic linking. **Rust links statically by default**, so a game shipping as one
executable has the engine baked in, and nobody can swap it.

LGPL-3.0 §4 anticipates this and lets you do it anyway, provided you take one of
its routes — in Rust terms:

1. **Ship the engine as a shared library** (`crate-type = ["cdylib"]` behind a C
   ABI) and link your game against it dynamically. This is the clause's native
   case, and the least paperwork. It is also real work: Kerosene currently
   builds as rlibs and has no stable C ABI.
2. **Ship what is needed to relink.** Provide your game's object files or
   compiled-but-unlinked artefacts, plus the engine source, so a user can build
   a modified engine and relink your game against it. `cargo build` leaves
   suitable artefacts in `target/`, but you have to actually distribute them and
   document the step.
3. **Open-source your game**, at which point the question stops mattering.

None of this affects development, internal builds, or a game you ship with
source. It matters the moment a *closed-source* binary goes out the door. If
that is the plan, decide between routes 1 and 2 early — retrofitting a C ABI
onto a finished engine is unpleasant.

## Third-party dependencies

325 distinct crates in the workspace dependency graph (normal edges, all
targets). **No GPL, LGPL, AGPL, EUPL, CDDL or SSPL crate at any depth** — the
strong copyleft licences, the ones that would reach into a game built on this,
are absent entirely.

There is weak copyleft, in two places, and they are not equivalent:

| | Reaches | Ships in a game |
|---|---|---|
| `smartstring` (MPL-2.0+), via `rhai` | the engine | **yes** |
| Symphonia, six crates (MPL-2.0) | `timbre` only | no |

Both are covered [below](#the-two-copyleft-dependencies).

| Licence | Crates |
|---|---|
| `MIT OR Apache-2.0` (and orderings/spellings of it) | 442 |
| `MIT` | 55 |
| `Unicode-3.0` | 36 |
| `Zlib OR Apache-2.0 OR MIT` (either order) | 34 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 12 |
| `Unlicense OR MIT` | 10 |
| `Apache-2.0` (only) | 10 |
| `ISC` | 5 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 4 |
| `BSD-2-Clause` / `BSD-3-Clause` / `BSD-* OR ...` | 9 |
| `Zlib`, `CC0-1.0`, `0BSD OR MIT OR Apache-2.0`, `Apache-2.0 AND MIT` | 4 |
| `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0` | 1 |

(Counts are per dependency *edge*, so a crate pulled in by several others is
counted more than once. Regenerate them with the command below; the table is a
snapshot and the command is the truth.)

Regenerate with:

```
cargo tree --workspace --edges normal --prefix none --format '{p}|{l}'
```

Every other copyleft entry the command prints is one of Kerosene's own
crates, which are LGPL by intent.

### The two copyleft dependencies

**What MPL-2.0 asks, first**, because it applies to both and is mild: it is
*file-level* copyleft. Modify one of their files and you publish that file;
use them unmodified and you keep the notice with the binary. It reaches
nothing else — not your code, not the crate that called it — which is the
whole difference between it and the GPL. And §3.3 explicitly permits combining
MPL code with a "Secondary License" (GPL, LGPL or AGPL) and conveying the
larger work under that licence, which is exactly the combination here.

#### `smartstring`, which the engine does link

```
smartstring 1.0.1 (MPL-2.0+)
└── rhai
    └── kerosene-script → kerosene-engine
```

Scripting pulls it in, so it is inside `kerosene` and inside every game built on
it. **A shipped game therefore carries MPL-2.0 code**, and the obligation that
travels with it is to preserve the notice — the same shape as the OFL and
Ubuntu font licences that egui brings, and satisfied the same way.
`kiln --ship` writes that notice into the distribution's `README.txt`, so a
build made with it is compliant without anyone remembering to be.

#### Symphonia, which only a build tool links

```
symphonia  symphonia-core  symphonia-common
symphonia-metadata  symphonia-bundle-flac  symphonia-bundle-mp3
```

All MPL-2.0, all reached only by `timbre`, which reads FLAC and MP3 sources.
`kerosene-audio` decodes what a *player's* machine loads and is hand-written for
that reason; Timbre is a compiler that runs on the machine making the content,
and `kiln --ship` refuses to put a tool in a distribution at all. So no game
ever ships these, and no game developer inherits anything from them. Alchemy
made the same call first, pulling in `image` for PNG and JPEG while the engine
reads only `.kerotex`.

Admitting them was a choice with poor alternatives. There is no maintained,
permissively licensed, pure-Rust MP3 decoder: the options were Symphonia, a C
library through bindings — which would put a C++ toolchain in the build and
wreck the cross-compilation story `kiln --ship` depends on — or writing an MP3
decoder by hand, which is several thousand lines of solved problem. FLAC alone
could have used `claxon` (Apache-2.0); MP3 forced it.

Dropping `timbre` removes all six. Nothing else in the workspace depends on it
except Kiln's sound stage.

### The four that actually matter

**Apache-2.0-only crates — this rules out GPL-2.0.**

```
winit  cpal  ab_glyph  ab_glyph_rasterizer
owned_ttf_parser  spirv  codespan-reporting  gethostname
```

Apache-2.0 is one-way compatible with the version 3 licences: you may combine
Apache-2.0 code into a GPL-3.0 or LGPL-3.0 work, and the result carries that
licence. It is **not** compatible with GPL-2.0 or LGPL-2.1 — the FSF and the
ASF agree on this, over Apache's patent termination and indemnification
clauses. So the v3 licences are available and the v2 ones are not, for as long
as `winit` is the windowing layer, and `winit` is not replaceable without
rewriting the whole platform layer. This is the single fact that decided
LGPL-**3.0** rather than LGPL-2.1. (`cpal`, the audio backend, is the same
story and would decide it the same way on its own.)

**Bundled fonts — `epaint_default_fonts`.** egui ships default typefaces
under the SIL Open Font Licence 1.1 and the Ubuntu Font Licence 1.0. Those
travel inside any binary linking egui, which here means Chisel and the
engine's debug overlay. Both licences permit redistribution; both require
their notices to be preserved, and OFL forbids selling the fonts on their
own and imposes a Reserved Font Name rule if you *modify* a font. Shipping
them unmodified inside an application is exactly the intended case. If you
publish binaries, ship the font licences alongside them.

**`hexf-parse` is CC0-1.0.** A public-domain dedication, so it imposes
nothing — but some corporate policies flag CC0 because it explicitly does
*not* grant patent rights. Irrelevant for a hobby or open-source release;
worth knowing if this ever goes near a company's legal review.

**`dpi` is `Apache-2.0 AND MIT`** — conjunctive, not a choice. Both sets of
terms apply. Both are permissive, so this changes nothing practical.

## Content in this repository

Everything under `content/` was made for this project:

* `content/art/**.png` — flat-colour and procedural developer textures
  (256x256 and 128x128), authored here. No metadata, no third-party source.
* `content/art/props/crate.obj` — hand-written vertex list.
* `content/maps/kero_start.keromap` — emitted by
  `crates/kerosene-map/examples/sample_map.rs`, i.e. generated by code in this
  repository.
* `content/materials/**.keromat` — hand-written KeyValues.

Nothing was extracted from, decompiled from, or converted out of any game.
Every compiled artefact (`.kerobsp`, `.kerotex`, `.keromdl`, `.keroprt`,
`.vault`) is a build output, reproducible with `scripts/build-content.sh`,
and is not committed.

## Provenance of the algorithms

This is the part worth being honest about, because it is the only place the
question has any teeth.

Kerosene implements techniques that are decades of published graphics
research: BSP trees for solid geometry and draw order (Fuchs/Kedem/Naylor
1980; Naylor/Amanatides/Thibault 1990), portal-based PVS precomputation
(Teller's 1992 dissertation), radiosity lightmapping (Goral et al. 1984),
Sutherland–Hodgman clipping (1974), Gribb–Hartmann frustum extraction. None
of that is anyone's property.

id Software's Quake tools are a famous *implementation* of several of them,
released under GPL-2.0, and Valve's Source engine descends from that lineage.
Some modules here follow that implementation's structure closely enough that
a reader would recognise it:

| Module | Follows |
|---|---|
| `kerosene-math::winding::Winding::split` | `ClipWindingEpsilon` |
| `cleave::tree::select_split` | `SelectSplitSide`'s scoring |
| `umbra::flow::clip_to_separators` | `ClipToSeperators` |
| `kerosene-physics::movement` | Source's `gamemovement` solver |
| `kerosene-map::texture` base axes | Quake's `baseaxis` table |

All of it was written from scratch in Rust, against descriptions of the
algorithms rather than by transcription; the naming, types, error handling,
data layout and tests are this project's own. US copyright does not extend to
"any idea, procedure, process, system, method of operation" regardless of how
it is described in a work (17 U.S.C. §102(b)), and the constants that *are*
copied — an epsilon of `0.1`, an air-speed cap of 30, a table of six axis
vectors — are facts about a behaviour being deliberately matched, not
creative expression.

That is the reasoning. It is not a guarantee, and the honest framing is:
following the structure of GPL-2.0 source is lower risk than copying it and
higher risk than never having read it. Every place it happens is named in a
source comment and in the table above, so nothing is hidden.

**The choice of copyleft does not change this**, and it is worth being explicit
about why, since picking the LGPL might look like an answer to it. It isn't. A
licence governs what *this* project grants downstream; it cannot clear anything
upstream, and GPL-2.0 code could not be moved under LGPL-3.0 in any case. What
actually lowers the risk is what the project already does: ship no Valve or id
content, define formats theirs cannot read, and state provenance plainly.

## Names and trademarks

"Valve", "Source", "Hammer", "Quake" and the rest appear throughout the docs
and comments. That use is *nominative* — naming someone else's product in
order to say what a thing here is analogous to. It is the same use as "works
like Photoshop". It is not branding: nothing in this project is named after a
Valve product, no Valve mark appears in a binary's name, icon or UI chrome,
and `NOTICE` disclaims affiliation explicitly.

The file formats were renamed for exactly this reason. `.vmap`, `.vmat`,
`.vmdl` and `.vtex` — the extensions this project originally used — are
Source 2's real extensions, and `.lin` is the leak pointfile Hammer loads.
They are now `.keromap`, `.keromat`, `.keromdl`, `.kerotex` and `.keroleak`,
and the two binary magics that named Valve formats (`VTEX`, `VMDL`) are now
`KRTX` and `KRMD`.

**The engine's own name changed once too**, for a plainer reason: it was
VoidEngine until August 2026, and an unrelated engine built on id Tech 6
already had that name. Nothing was shared with it and nothing needed
clearing; the collision was simply a collision. The extensions moved with the
name — `.voidmap` became `.keromap`, and the magics `VOID`, `VOTX`, `VOMD`
and `VOAU` became `KROS`, `KRTX`, `KRMD` and `KRAU` — so a file written by
either version says which it came from.

## Consequences of the choice, in one place

**For contributors.** Patches are LGPL-3.0-or-later. Nothing else is needed —
there is no CLA, and the SPDX line in every file records it.

**For someone forking the engine.** Publish your changes to the engine under the
same licence. You are not obliged to open anything you build *on top of* it.

**For someone shipping a game.** Your game is yours. Two obligations travel with
the binary: say that it uses Kerosene and where to get the source, and satisfy
the §4 relinking clause above if the game itself is closed-source.

**For someone shipping the tools.** Chisel and the compilers are ordinary
copyleft binaries — distribute the corresponding source.

**Fonts, again, because it catches people.** Any binary linking egui — Chisel,
Timbre, and the engine's debug overlay — carries OFL-1.1 and Ubuntu-Font-1.0
typefaces inside it. Both licences are satisfied by shipping their notices
alongside the binary. Neither conflicts with the LGPL, because the fonts are
data travelling with the program rather than part of it.

**And `smartstring`, for the same reason.** It is MPL-2.0 and it is inside the
engine, so a shipped game carries it and owes its notice. `kiln --ship` writes
that too.

**What the licence does not do.** It does not make the provenance question above
go away, in either direction. Copyleft is a statement about what *you* grant
downstream; it is not a clearance of anything upstream. What actually keeps this
project clean is what it already does: ship no Valve or id content, define
formats theirs cannot read, and name every module that follows the structure of
published work.
