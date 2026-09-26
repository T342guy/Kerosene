// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Write Chisel's starter room out as a `.keromap`, so the compilers can be
//! pointed at exactly what a new document contains.
fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "starter.keromap".into());
    let document = chisel::app::starter_document();
    std::fs::write(&out, document.map.to_text())?;
    println!("wrote {out}");
    Ok(())
}
