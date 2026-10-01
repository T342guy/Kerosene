# Software design document

This is the top-down design of the Kerosene engine: what it is for, how it is
put together, why it is put together that way, and where it is going. It is
the place to start when you need the whole picture, and the place to record a
design decision so it is not lost.

It is deliberately **not** a second copy of the rest of the book. Each chapter
states the design and the reasoning, then links to the page that holds the
detail:

| If you want… | Read |
|---|---|
| to *use* the engine | [Making games](../gamedev/getting-started.md) |
| a reference for one feature | [Documentation](../docs/architecture.md) |
| the internals of one subsystem | [Devnotes](../devnotes/README.md) |

| | |
|---|---|
| **Status** | 1.0.0-a4 (alpha) |
| **Last reviewed** | 2026-09-30 |
| **Owner** | T342 |
| **Scope** | The workspace in this repository. The demo game lives in a separate repository (`kerosene-demo`). |

## Chapters

1. [Goals and scope](goals-and-scope.md)
2. [Design principles](principles.md)
3. [Architecture](architecture.md)
4. [Runtime](runtime.md)
5. [Data and the build pipeline](data-and-pipeline.md)
6. [World and entities](world-and-entities.md)
7. [Subsystems](subsystems.md)
8. [Tools](tools.md)
9. [Public API](public-api.md)
10. [Quality](quality.md)
11. [Decision log](decisions.md)
12. [Status and roadmap](status-and-roadmap.md)
13. [Open issues](open-issues.md)

## Editing this document

- One chapter is one file in `src/sdd/`. Edit it, and keep the chapter list
  above and in `src/SUMMARY.md` in step.
- Change **Last reviewed** above when you review the document, not on every
  typo fix.
- State a design as a rule plus a reason. A rule with no reason cannot be
  re-examined later.
- Anything that must stay true of the code belongs in a check (`cargo xtask
  layers`, a test), with the chapter pointing at it. A claim only in prose
  drifts.
- New decisions go in the [decision log](decisions.md); known gaps go in
  [open issues](open-issues.md). Remove an issue when it is fixed.
- Diagrams are [mermaid](https://mermaid.js.org/) blocks, as in the devnotes.
- Relative links only: CI runs `scripts/check-links.py` over the book.

To preview: `mdbook serve` in the repository root.
