// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Writes `content/maps/kerosene_room.keromap`, the room in the engine's base
//! content: what a game with no map of its own opens on.
//!
//! Run with `cargo run -p kerosene-map --example starter_room`;
//! `scripts/build-content.sh` does, then compiles it and packs it into
//! `base.vault`. The room is [`kerosene_map::starter::room`], the same one
//! `kerosene-tools new` gives a new game, so the two cannot drift apart.

fn main() -> std::io::Result<()> {
    let map = kerosene_map::starter::room(false);
    let path = std::path::Path::new("content/maps")
        .join(kerosene_map::starter::NAME)
        .with_extension("keromap");
    std::fs::write(&path, map.to_text())?;
    println!("wrote {}", path.display());
    Ok(())
}
