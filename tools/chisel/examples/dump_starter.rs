// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Write Chisel's starter room out as a `.keromap`, so the compilers can be
//! pointed at exactly what a new document contains.
fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "starter.keromap".into());
    let document = chisel::app::starter_document();
    std::fs::write(&out, document.map.to_text())?;
    println!("wrote {out}");
    Ok(())
}
