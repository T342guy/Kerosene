# FAQ

Short answers. Each links to the page with the long one.

**Can I sell a game made with Kerosene?**
Yes. Neither licence restricts commercial use.
([Publishing](publishing.md#you-can))

**Do I have to open-source my game?**
No. Under the MPL arm, copyleft is per file and reaches only Kerosene's own
files; your code, levels, assets and scripts are yours. Under the LGPL arm
your game code can stay closed too, but the engine part must be replaceable.
([Licensing](../docs/licensing.md))

**Which licence should I pick?**
MPL-2.0, unless you specifically want the LGPL's stronger guarantee that
modified engines stay open. MPL has no linking clause, so a statically
linked closed-source game owes a notice and a source pointer, nothing more.
([Publishing](publishing.md#requirements))

**What do I have to ship alongside the game?**
Both licence texts and a `README.txt` with the notices. `kiln --ship` writes
all of it; you fill in where the engine source can be obtained.
([Shipping checklist](shipping-checklist.md))

**Can I remove the "Built with Kerosene" notice?**
No. It is the notice both licences require of a program carrying their code.
([Publishing](publishing.md#you-cannot))

**Can I ship Chisel or the compilers with my game, for modders?**
Not inside the distribution `kiln --ship` makes, and not without also
distributing their source: the tools are copyleft binaries in their own
right. Point modders at the Kerosene repository instead.
([Publishing](publishing.md#you-cannot))

**I changed the engine. What do I owe?**
It depends on the arm and on the name. Under MPL, the changed files, as
source, under MPL-2.0. Under LGPL, the modified engine, under LGPL, kept
replaceable. If you call the result *Kerosene-anything*, add the naming
policy: the whole engine as source, a link to it, and a "built from
Kerosene" link back. Either way the project would rather have a pull request.
([Publishing](publishing.md#forks-and-derived-engines))

**Can I make my own engine out of Kerosene and call it "Kerosene: Ultimate"?**
Yes, and the project would like that — the way Titanfall's engine was a
rewritten branch of Source. The name comes with four conditions: publish the
whole modified engine as source, link to it, link to Kerosene with "built
from" or "modified from" and the commit you left at, and say it is a
derivative rather than the official one. Pick a name that is not Kerosene's
and only the licence applies. ([Publishing](publishing.md#the-naming-policy))

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
walk around it — but save/load, menus, gamepads, networking, animation and
crash reporting do not exist. ([Publishing](publishing.md#you-can-but))

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
