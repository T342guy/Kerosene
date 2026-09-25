# FAQ

Short answers. Each links to the page with the long one.

**Can I sell a game made with Kerosene?**
Yes. The licence does not restrict commercial use.
([Publishing](publishing.md#you-can))

**Do I have to open-source my game?**
No. The Kerosene Exception lets you link the engine into a closed game.
Your code, levels, assets and scripts are yours; the engine part stays GPL
with its source available. ([Licensing](../docs/licensing.md))

**Isn't the GPL viral? How can my game be closed?**
On its own it would be. The exception is an additional permission under
GPL §7 that says a game linking Kerosene is not a covered work, on
conditions: engine source, the built-with notice, an attribution screen,
and — if you changed the engine — the whole modified engine published.
([Licensing](../docs/licensing.md#the-exception-clause-by-clause))

**What do I have to ship alongside the game?**
Both texts (`LICENSE`, `LICENSE-EXCEPTION`) and a `README.txt` with the
notices. `kiln --ship` writes all of it; you fill in where the engine source
can be obtained. And the game itself shows an attribution screen at start.
([Shipping checklist](shipping-checklist.md))

**What is the attribution screen?**
"Built with Kerosene", shown each time the game starts, before or with the
first interactive screen. It can be brief and dismissable; it cannot be
switched off. Draw it in `Game::ui` for now — the engine will draw it for
you eventually. A game on a modified engine shows "built with *Fork*,
modified from Kerosene". ([Publishing](publishing.md#requirements))

**Can I remove the "Built with Kerosene" notice?**
No. It is a condition of the linking permission and a notice §7(b) of the
GPL lets the project require. Remove it and the permission goes with it,
which makes the whole game GPL. ([Publishing](publishing.md#you-cannot))

**Can I ship Chisel or the compilers with my game, for modders?**
Not inside the distribution `kiln --ship` makes, and not without also
distributing their source: the tools are GPL binaries in their own right.
Point modders at the Kerosene repository instead, or build your own
`mygame-tools` on `kerosene::tools`, which ships like the game.
([Publishing](publishing.md#you-cannot))

**I changed the engine. What do I owe?**
The whole modified engine stays GPL with the exception; it says "modified
from Kerosene" with the version you diverged from and a link back — in its
notice, the game's `README.txt` and the attribution screen; and if your
game is closed, the modified engine's complete source is published where
anyone can get it. The project would rather have a pull request.
([Publishing](publishing.md#forks-and-derived-engines))

**Can I make my own engine out of Kerosene and call it "Kerosene: Ultimate"?**
Yes, and the project would like that — the way Titanfall's engine was a
rewritten branch of Source. The name costs what a modified engine already
owes plus publishing its whole source whether or not a closed game ships on
it, and saying plainly it is a derivative rather than the official one.
([Publishing](publishing.md#the-naming-policy))

**Do I owe anything for Symphonia (the MP3/FLAC decoder)?**
No. Only Timbre, a build tool, links it, and `kiln --ship` never puts a tool
in a distribution. The one MPL-2.0 dependency a game *does* carry is
`smartstring`, and its notice is written for you.
([Licensing](../docs/licensing.md#the-two-mpl-20-dependencies))

**Can I use Source or Hammer assets, or open a `.vmf`?**
No, on both counts. Kerosene's formats are its own and deliberately cannot
read theirs, and the project ships no Valve or id content. Bring your own.
([Licensing](../docs/licensing.md#names-and-trademarks))

**Is it game-ready?**
No. It is pre-alpha. Everything works end to end — draw a level, compile it,
walk around it, save and load — but menus, gamepads, networking and crash
reporting do not exist yet. ([Publishing](publishing.md#you-can-but))

**Does it run on Windows or macOS?**
It builds on both in CI. Nobody has run the result by hand.
([Platforms](platforms.md))

**Can players mod a shipped game?**
Yes, trivially: a loose file under `content/` shadows the same path in the
`.vault`. That is also why there is no tamper protection.
([Publishing](publishing.md#you-can-but))

**Can I make a game without writing Rust?**
Yes — a content-only project: maps, entity I/O and Rhai scripts on the stock
runtime. The moment you need something the stock entity classes cannot do,
you need a game crate. ([Making a game](making-a-game.md#two-shapes-of-project))
