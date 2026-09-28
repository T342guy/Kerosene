#!/usr/bin/env python3
# Check that every relative link in the book and the README leads to a file.
#
# mdBook builds a page with a broken link without a word, and the reader
# finds it. Links out of the repository (http, mailto) and anchors within a
# page are not checked; a link's own #anchor is dropped before its file is.
#
# Usage: scripts/check-links.py        (from anywhere; exits 1 on any)
import pathlib
import re
import sys

root = pathlib.Path(__file__).resolve().parent.parent
# A destination is bare, or in angle brackets when it has spaces or
# parentheses of its own: `[a](<Some Page (draft).md>)`.
link = re.compile(r"\]\((?:<([^>]+)>|([^)\s]+))\)")
fence = re.compile(r"^\s*(```|~~~)")

pages = sorted(root.glob("src/**/*.md")) + [root / "README.md"]
broken = []
for page in pages:
    in_code = False
    for number, line in enumerate(page.read_text(encoding="utf-8").splitlines(), 1):
        if fence.match(line):
            in_code = not in_code
            continue
        if in_code:
            continue
        for bracketed, bare in link.findall(line):
            target = bracketed or bare
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            # Rust intra-doc style paths in prose, `crate::x`, are not files.
            if "::" in target:
                continue
            path = target.split("#", 1)[0]
            if not path:
                continue
            if not (page.parent / path).exists():
                broken.append(f"{page.relative_to(root)}:{number}: {target}")

for b in broken:
    print(b)
if broken:
    print(f"{len(broken)} broken link(s)", file=sys.stderr)
    sys.exit(1)
print(f"{len(pages)} pages, every relative link leads somewhere")
