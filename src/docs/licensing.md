# Licensing

Kerosene is licensed under the **GNU General Public License, version 3 or
later, with the Kerosene Exception** — additional terms under the GPL's
section 7. Both texts ship in the repository, `LICENSE` and
`LICENSE-EXCEPTION`; `Cargo.toml` declares
`license = "GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0"`, and
every source file carries the matching `SPDX-License-Identifier` line.

`AdditionRef-` is SPDX's prefix (from version 2.3) for an addition to a
licence that is not on SPDX's own list of exceptions, which the Kerosene
Exception is not. It is what makes the expression one that tools can read:
Cargo, `cargo-deny` and anything else that audits a dependency tree see
the GPL *and* the Exception — including its permission to link Kerosene
statically — rather than refusing the line or reading it as the GPL alone.
Earlier versions spelled it `LicenseRef-Kerosene-Exception-1.0`, which SPDX
does not allow after `WITH`; the terms are the same.

`NOTICE` states what that means on one page; this document explains the
reasoning, walks the exception clause by clause, and then does the dependency
audit.

I am not a lawyer and none of this is legal advice. It is an accurate inventory
plus the reasoning behind the choice, so that you or an actual lawyer can move
quickly.

## What the licence means here

**The GPL on its own** would make any program that links Kerosene a covered
work: a game built on it would have to be GPL too. The exception changes
that, and only that.

**A game built on Kerosene is yours.** The exception's section 1 is an
additional permission: link Kerosene, statically or dynamically, into a game
and convey the game under terms of your choice — closed, commercial, whatever
you like. Your code, levels, scripts and assets are yours. The conditions:

* the engine part of the game — Kerosene, plus any change you made to it —
  stays under the GPL with the exception, with its source available as the
  GPL requires;
* the game carries a notice that it is built with Kerosene and says where
  the engine source is (`kiln --ship` writes it);
* the game shows an **attribution screen** when it starts — "Built with
  Kerosene", before or together with its first interactive screen. Brief and
  dismissable is fine; disabled or hidden is not. A program with no graphical
  interface prints the same line instead;
* if the engine is modified, the *whole* modified engine is published as
  source where anyone can obtain it free of charge, not only offered to the
  people who receive the game.

**A modified engine is a modified Kerosene**, whatever it is called and
wherever the changed files are put — a copy of `movement.rs` inside your game
crate is still the engine. Convey one, alone or inside a game, and section 2
of the exception asks that it say plainly that it is *"modified from
Kerosene"* or *"built from Kerosene"*, name the version or commit it diverged
from, and link to Kerosene's source — in its own notice, in the notice of
every game built on it, and on the attribution screen of every game built on
it. It must not present itself as the original or as endorsed by this
project, and it must carry the attribution-screen requirement forward so that
games on the fork still show where they came from.

**The Kerosene name** is not granted by the licence. A fork may call itself
Kerosene-something only if it does the above *and* publishes its complete
source, as one engine rather than as changed files, under the same terms.
Any other name owes only what the GPL and the exception say.

**The tools** — Chisel, the compilers, Kiln — are under the same terms. The
linking permission covers them too, so a game's own `mygame-tools` binary
built on `kerosene::tools` ships the same way the game does. An unmodified
tool distributed on its own is a plain GPL binary: ship it with its source.

## Why the GPL with an exception, and not the LGPL or the MPL

Kerosene began under LGPL-3.0-or-later, then offered MPL-2.0 as an
alternative arm. Each had the flaw the other fixed:

* **The LGPL's mechanism is relinking.** It assumes a user can swap the
  library inside a program for their own build, and Rust's default static
  linking makes that a chore for anyone shipping a closed game — C-ABI
  plumbing, or an object-file distribution step `cargo` does not do. None of
  it bites at development time; the moment a binary leaves the door it
  demands work.
* **The MPL's unit is the file.** It has no linking stage, so it asks nothing
  of a closed game — but it also asks nothing of a *modified engine* beyond
  the files that changed, and nothing at all of a fork that presents itself
  as something new. The derivative-engine policy had to live outside the
  licence, as a condition on using the name.

GPL §7 gives a licensor the two levers that fix both at once. An **additional
permission** may be granted on any conditions the licensor likes, because a
recipient who declines the conditions simply has no permission and the plain
GPL applies. That is the linking exception: it does what the LGPL does for a
game — keep your code, keep the engine open — without the relinking clause,
and its conditions carry the policy the MPL could not. And §7 (b)–(f) allow
a short list of **additional requirements** — preserved notices, marking of
modified versions, no misrepresentation, limits on the licensor's name — that
cover the rest of the policy and that a recipient *cannot* strip.

Nothing in the exception is a "further restriction" in §7's sense. Every
requirement is one the section names, and every condition sits on the
permission rather than on the GPL.

## The exception, clause by clause

| Clause | What it encodes | Why it is allowed |
|---|---|---|
| §1 | You may link Kerosene into a game and ship the game under your own terms | An additional permission (§7, first paragraph) |
| §1(a) | The engine part, with your changes, stays GPL with source | A condition of the permission |
| §1(b) | The game carries "built with Kerosene" and a source pointer | A condition of the permission; also §7(b) |
| §1(c) | A modified engine's whole source is public, not only offered to recipients | A condition of the permission — the GPL alone could not require publication |
| §1(d) | The game shows the attribution screen | A condition of the permission |
| §2(a) | Copyright notices, `NOTICE`, and the built-with statement are preserved | §7(b): preservation of legal notices and author attributions |
| §2(b) | A modified engine says "modified from Kerosene", the version diverged from, and a link — in its notice, its games' notices and their attribution screens; it is not presented as the original | §7(b) attribution; §7(c) marking of modified versions and no misrepresentation |
| §2(c) | The authors' names are not used for publicity | §7(d) |
| §2(d) | "Kerosene" as a fork's name only with §2(b) and the whole source published | §7(e): declining to grant trademark rights, on stated conditions |
| §2(e) | The attribution screen, at start, before or with the first interactive screen; inherited by every fork | §7(b): an author attribution in the *Appropriate Legal Notices* the GPL's §5(d) already has interactive programs display |

A recipient may remove the §1 permission from their copy, as §7 allows, and
be left with the plain GPL plus §2. Nobody can remove §2.

The engine does not yet *draw* the attribution screen for you: a game adds
one through `Game::ui` today, and an engine-drawn one that satisfies §2(e) by
default is a listed gap in [Missing features](missing-features.md). The
obligation is on the shipped program either way.

## Third-party dependencies

325 distinct crates in the workspace dependency graph (normal edges, all
targets). Two facts matter more than any list:

* **Every licence in the tree is GPL-3.0-compatible.** Apache-2.0 is
  compatible with GPL version 3 (not 2, which is one reason the project is
  3-or-later), and MPL-2.0 lets its files be combined into a GPL work through
  its own §3.3. A copyleft dependency is ordinary here rather than something
  to quarantine.
* **No GPL crate appears at any depth, and no LGPL or AGPL crate either.**
  Kerosene is the only whole-work copyleft in a game built on it, and the
  exception is what decides what that asks of the game. Nothing in the tree
  adds an obligation of its own beyond a notice.

The only copyleft-licensed dependencies are MPL-2.0:

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

The MPL-2.0 rows above are `smartstring` and Symphonia's six crates.

`box3d-rust` (MIT) sits in the plain-`MIT` bucket and backs `kerosene-rigid`,
the rigid-body simulation. It is a pure-Rust port of Erin Catto's Box3D with
no dependencies of its own and no build script, chosen over a Jolt FFI wrapper
specifically so the tree never has to vendor a C++ physics library, carry
bindgen output, or require a C++ toolchain to link it.

### Steamworks, which only a Steam build links

The `steamworks` crate (MIT OR Apache-2.0) and the `steamworks-sys` crate
under it are behind the opt-in `steam` feature of `kerosene-platform`, which
is off in every default build. What they bind, and carry, is Valve's
Steamworks SDK: its headers and its redistributable `steam_api` libraries.
The SDK is not free software, and is distributed under the Steamworks SDK
Access Agreement.

That is why it is a feature and not a dependency. The Kerosene tree vendors
none of the SDK, and nothing in the default build links it. The table above
describes the default build, so the SDK is not in it.

A game that turns the feature on is a Combined Work under the Exception, with
the SDK as an Independent Module, which §1 permits. The engine part stays GPL
with its source available; the SDK stays under Valve's terms. The GPL alone
would not allow the combination, and this is one of the things the Exception
is for. `kiln --ship --steam` writes a notice saying all of this into the
shipped `README.txt`.

### The two MPL-2.0 dependencies

**What MPL-2.0 asks**: file-level copyleft. A shipped, *unmodified* binary
carrying MPL-2.0 code owes a notice — the licence text and a statement of where the MPL source
lives. Modified MPL *files* must be released as source under MPL-2.0. It
reaches nothing else.

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

#### glTF, which only a build tool links

```
gltf  gltf-json  gltf-derive  serde  serde_derive  serde_json
```

MIT or Apache-2.0, all reached only by `forge`, which reads `.gltf` and `.glb`
sources. The engine reads the `.keromdl` Forge writes and never links these,
for the same reason as Symphonia below: a tool is never shipped.

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

Apache-2.0 decided the GPL *version*: it cannot be bundled under GPL-2.0, but
it can under GPL-3.0, and Kerosene is 3-or-later. The list is kept here
because it is the set of crates whose "Apache-2.0 only" status is worth
knowing if anyone ever forks with different terms in mind.

**Bundled fonts — `epaint_default_fonts` and `egui-phosphor`.** egui ships
default typefaces under the SIL Open Font Licence 1.1 and the Ubuntu Font
Licence 1.0, and `egui-phosphor` (MIT OR Apache-2.0) embeds the Phosphor icon
font, itself MIT. Those travel inside any binary linking egui, which here
means the toolset and the engine's debug overlay. Both licences permit redistribution; both require
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
about why, since picking a licence might look like an answer to it. It isn't.
A licence governs what *this* project grants downstream; it cannot clear
anything upstream. Being GPL-licensed ourselves is about *combining* code
going forward, not about taking code *out of* a GPL-2.0 work — GPL-2.0 code
could not be moved under GPL-3.0-or-later with an exception in any case,
because the GPL does not permit relicensing its code without the author's
permission. What actually lowers the risk is what the project already does:
ship no Valve or id content, define formats theirs cannot read, and state
provenance plainly.

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

**For contributors.** Patches to Kerosene's own files are offered under the
same terms — GPL-3.0-or-later WITH the Kerosene Exception. That matters more
than it would for a plain GPL project: the exception is an additional
permission, and a contribution offered under the GPL alone would carry no
such permission, so the file it touched could no longer be linked into a
closed game. There is no CLA; the SPDX line in every file records the terms,
and sending a pull request is accepting them.

**For someone forking the engine.** The whole modified engine is under the
GPL with the exception. It says "modified from Kerosene", names the version
or commit it diverged from, and links to Kerosene's source — in its notice,
in the notice of every game built on it, and on their attribution screens.
Ship a closed game on it and the fork's whole source is published where
anyone can get it. Call it Kerosene-something and the same publication is
the price of the name. The project's preference, every time: contribute the
change back as a pull request and use the updated engine, so the fix exists
once rather than once per fork. [Forks and derived
engines](../gamedev/publishing.md#forks-and-derived-engines) has the three
cases side by side.

**For someone shipping a game.** Your game is yours. You owe the licence
texts, the notice with a source pointer, and an attribution screen at start;
`kiln --ship` writes the first two and the [Shipping
checklist](../gamedev/shipping-checklist.md) has the third. There is no
relinking clause and nothing resembling one.

**For someone shipping the tools.** Chisel and the compilers are under the
same terms. A game's own tools binary is a combined work like the game and
ships the same way. A tool on its own is a GPL binary: distribute the source
with it.

**Fonts, again, because it catches people.** Any binary linking egui — the
toolset, and the engine's debug overlay — carries OFL-1.1 and Ubuntu-Font-1.0
typefaces inside it, and the toolset carries the MIT Phosphor icon font too.
All three licences are satisfied by shipping their notices alongside the
binary. None conflicts with the GPL, because the fonts are data travelling
with the program rather than part of it.

**And `smartstring`, for the same reason.** It is MPL-2.0 and it is inside the
engine, so a shipped game carries it and owes its notice. `kiln --ship` writes
that too.

**What the licence does not do.** It does not make the provenance question
above go away, in either direction. Copyleft is a statement about what *you*
grant downstream; it is not a clearance of anything upstream. What actually
keeps this project clean is what it already does: ship no Valve or id
content, define formats theirs cannot read, and name every module that
follows the structure of published work.
