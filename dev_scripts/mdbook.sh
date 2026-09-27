#!/bin/bash
# Install what building the book needs: mdBook and its Mermaid preprocessor.
# Then `mdbook serve` in the repository root.
set -euo pipefail

cargo install mdbook mdbook-mermaid
