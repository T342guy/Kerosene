# Licensing

Kerosene is **MPL-2.0**. `LICENSE-MPL-2.0` holds the full text, `Cargo.toml`
declares `license = "MPL-2.0"`, and every source file carries an
`SPDX-License-Identifier: MPL-2.0` line.

Nothing in the dependency tree obstructs that, and every line of Kerosene was
written for this project. Two things are worth understanding anyway: one
property of the licence needs stating plainly up front, because most people
get it wrong, and one part of the provenance story deserves to be stated
rather than buried.

I am not a lawyer and none of this is legal advice. It is an accurate inventory
plus the reasoning behind the choice, so that you or an actual lawyer can move
quickly.

## What MPL-2.0 means here

The Mozilla Public License is **weak, file-level copyleft**. That one phrase
is the whole of it, and the whole reason for choosing it:

* **A "file" is the unit of copyleft, not the program.** MPL-2.0 obligations
  attach to *individual source files* that carry the licence. Modify an MPL
  file and distribute it, and you must make *that file's* source available
  under MPL-2.0. Files you write yourself — your game code, your levels, your
  scripts — carry whatever terms you like, even when they sit in the same
  directory, the same crate, or the same binary as MPL-2.0 code.
* **There is no "linking" stage.** This is the entire difference from the
  GPL family, and it makes the question of static versus dynamic linking
  irrelevant. MPL-2.0 does not care how MPL and non-MPL code are combined —
  the obligations stay where the licence is, in the MPL files, and reach
  nothing beyond them. The concern that decides so much of how a GPL or LGPL
  engine has to be shipped simply does not arise here.
* **Your game is yours.** Game code, assets, levels, shaders, scripts — all
  yours, under whatever terms you like, shipped any way you like, closed
  source or otherwise. No relinking clause, no object-file distribution, no
  shared-library requirement.
* **Changing Kerosene itself** — fixing the BSP compiler, adding a shader
  path, altering a format — means distributing those changed files under
  MPL-2.0 too. The obligation is confined to the files you actually change,
  not to a whole work they are embedded in.
* **The tools work exactly the same way.** Chisel, Cleave, Umbra, Radiance,
  Alchemy, Timbre, Forge, Vault and Kiln are also MPL-2.0. Because MPL-2.0
  makes no distinction between a library and a standalone program, there is
  nothing special to say about them: ship the corresponding source for the
  MPL files if you distribute modified tools, exactly as for the engine.

### The thing most people get wrong

MPL-2.0 is *not* permissive, and it is not an MIT-style licence with extra
words. Unmodified use imposes a notice obligation (below), and modified MPL
files must be released under MPL-2.0. What it does *not* do — and cannot do —
is leap from the files you touch to the rest of the program. That is the
single property this project relies on, and it is worth reading the licence's
own definitions of "Covered Software" and "Larger Work" once to see that it is
real and not a happy reading.

## Why MPL-2.0 and not the GPL family

The engine was originally released under LGPL-3.0-or-later, and moved to
MPL-2.0 for two reasons that are really one reason.

The LGPL's mechanism assumes a user can swap the library inside your program
for their own build, and Rust links statically by default. LGPL-3.0 §4 offers
routes around that — ship the engine as a shared library behind a C ABI, or
ship your game's object files so it can be relinked — and both are real,
ongoing work for anyone shipping a closed-source game. None of it bites at
development time, but the moment a binary leaves the door it demands either
C-ABI plumbing or an object-file distribution step that `cargo` does not do
for you.

MPL-2.0 has no linking stage, so none of that applies. The trade for the
project is that MPL-2.0's copyleft is *weaker* than the LGPL's: it does not
require that a *modified engine* be released as a whole under a copyleft
licence, only the modified files. If a contributor wanted the strongest
possible guarantee that forks of the engine stay open, LGPL-3.0 is stronger;
if the goal is "a game built on this can be shipped with no ceremony," MPL-2.0
is the licence that says so *without* a clause you have to lawyer your way
around. The project chose the second.

Compatibility in the other direction is worth one sentence: MPL-2.0 §3.3
says you may combine MPL code with code under a "Secondary License" from
the GPL, LGPL or AGPL families and convey the larger work under that licence.
So a GPL or LGPL project may still take Kerosene crates and use them, with
the combined work carrying the GPL-family licence. MPL-2.0 does not wall
this project off from the copyleft world the way a permissive-only reading
might suggest.

## Third-party dependencies

325 distinct crates in the workspace dependency graph (normal edges, all
targets). Two facts matter more than any list:

* **The previous licence forced a hard line around copyleft.** Under the LGPL,
  a copyleft dependency could reach into a game built on this, so only
  file-level copyleft like MPL-2.0 was acceptable, and only where it stayed
  out of the engine. That constraint is gone: MPL-2.0 code is now the same
  licence the project itself ships under, so a copyleft dependency is no
  longer something to quarantine, it is the norm.
* **No GPL, LGPL or AGPL crate appears at any depth.** The strong
  whole-work copyleft licences are absent entirely. Nothing requires a game
  built on this to carry a copyleft of its own.

The only copyleft-licensed dependencies are themselves MPL-2.0, which is now
the project's own licence:

| Reaches | Ships in a game |
|---|---|
| `smartstring` (MPL-2.0+), via `rhai` | the engine | **yes** |
| Symphonia, six crates (MPL-2.0) | `timbre` only | no |

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
| `MPL-2.0` / `MPL-2.0+` | 7 |

(Counts are per dependency *edge*, so a crate pulled in by several others is
counted more than once. Regenerate them with the command below; the table is a
snapshot and the command is the truth.)

Regenerate with:

```
cargo tree --workspace --edges normal --prefix none --format '{p}|{l}'
```

The MPL-2.0 rows above are `smartstring` and Symphonia's six crates; under
the previous licence they were the exceptions this section existed to manage,
and now they are simply dependencies that share the project's licence.

### The two MPL-2.0 dependencies

**What MPL-2.0 asks**, first, because it is the project's own licence too:
file-level copyleft. A shipped, *unmodified* binary carrying MPL-2.0 code owes
a notice — the licence text and a statement of where the MPL source lives.
Modified MPL *files* must be released as source under MPL-2.0. It reaches
nothing else.

#### `smartstring`, which the engine does link

```
smartstring 1.0.1 (MPL-2.0+)
└── rhai
    └── kerosene-script → kerosene-engine
```

Scripting pulls it in, so it is inside `kerosene` and inside every game built
on it. A shipped game therefore carries MPL-2.0 code, and the obligation that
travels with it is the notice — the same shape as the OFL and Ubuntu font
obligations that egui brings, and satisfied the same way. `kiln --ship` writes
that notice into the distribution's `README.txt`, so a build made with it is
compliant without anyone remembering to be.

#### Symphonia, which only a build tool links

```
symphonia  symphonia-core  symphonia-common
symphonia-metadata  symphonia-bundle-flac  symphonia-bundle-mp3
```

All MPL-2.0, all reached only by `timbre`, which reads FLAC and MP3 sources.
`kerosene-audio` decodes what a *player's* machine loads and is hand-written
for that reason; Timbre is a compiler that runs on the machine making the
content, and `kiln --ship` refuses to put a tool in a distribution at all. So
no game ever ships these, and no game developer inherits anything from them.
Alchemy made the same call first, pulling in `image` for PNG and JPEG while
the engine reads only `.kerotex`.

Admitting them was a choice with poor alternatives. There is no maintained,
permissively licensed, pure-Rust MP3 decoder: the options were Symphonia, a C
library through bindings — which would put a C++ toolchain in the build and
wreck the cross-compilation story `kiln --ship` depends on — or writing an MP3
decoder by hand, which is several thousand lines of solved problem. FLAC alone
could have used `claxon` (Apache-2.0); MP3 forced it.

### The four that actually matter

**Apache-2.0-only crates.**

```
winit  cpal  ab_glyph  ab_glyph_rasterizer
owned_ttf_parser  spirv  codespan-reporting  gethostname
```

Apache-2.0 is compatible with MPL-2.0 in both directions — neither licence
imposes an obligation the other forbids, and neither's copyleft reaches the
other's files. Under the previous LGPL-3.0 licence this list decided the
*version* of the licence, because Apache-2.0 cannot be bundled under GPL-2.0.
MPL-2.0 has no such constraint, so the list is a note rather than a decision.
It is kept here because it is still the set of crates whose "Apache-2.0 only"
status is worth knowing if anyone ever reverts the licence or forks with
different terms in mind.

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

**The choice of licence does not change this**, and it is worth being explicit
about why, since picking MPL-2.0 might look like an answer to it. It isn't. A
licence governs what *this* project grants downstream; it cannot clear anything
upstream. MPL-2.0's compatibility with the GPL (§3.3) is about *combining*
MPL code with GPL code going forward, not about taking code *out of* a
GPL-2.0 work — GPL-2.0 code could not be moved under MPL-2.0 in any case,
because the GPL does not permit relicensing its code to a non-GPL licence
without the author's permission. What actually lowers the risk is what the
project already does: ship no Valve or id content, define formats theirs
cannot read, and state provenance plainly.

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

**For contributors.** Patches to Kerosene's own files are MPL-2.0. Nothing
else is needed — there is no CLA, and the SPDX line in every file records it.

**For someone forking the engine.** Publish your changes to the files you
change under MPL-2.0. You are not obliged to open anything you write *around*
them.

**For someone shipping a game.** Your game is yours. One obligation travels
with the binary: preserve the notice for the MPL-2.0 code it carries — say
that it uses Kerosene, and point at where the engine's source lives. Nothing
resembles a relinking clause, because there is no linking stage in MPL-2.0.

**For someone shipping the tools.** Chisel and the compilers are MPL-2.0 like
everything else. Distribute the corresponding source for any MPL files you
modify; an unmodified tool ships with the notice and a pointer to the source,
the same as the engine.

**Fonts, again, because it catches people.** Any binary linking egui — Chisel,
Timbre, and the engine's debug overlay — carries OFL-1.1 and Ubuntu-Font-1.0
typefaces inside it. Both licences are satisfied by shipping their notices
alongside the binary. Neither conflicts with MPL-2.0, because the fonts are
data travelling with the program rather than part of it.

**And `smartstring`, for the same reason.** It is MPL-2.0 and it is inside the
engine, so a shipped game carries it and owes its notice. `kiln --ship` writes
that too.

**What the licence does not do.** It does not make the provenance question above
go away, in either direction. A weak copyleft is a statement about what *you*
grant downstream; it is not a clearance of anything upstream. What actually
keeps this project clean is what it already does: ship no Valve or id content,
define formats theirs cannot read, and name every module that follows the
structure of published work.
