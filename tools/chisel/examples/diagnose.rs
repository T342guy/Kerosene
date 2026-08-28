// SPDX-License-Identifier: LGPL-3.0-or-later
//! What Chisel sees when it starts, without opening a window.
//!
//! ```text
//! cargo run -p chisel --example diagnose -- [map.voidmap] [--content <dir>]
//! ```
//!
//! For the one question a level editor cannot answer for itself: *why is there
//! nothing in here?* An editor that found no content looks identical to one
//! whose content is empty -- three hard-coded class names, flat-coloured
//! brushes -- so this prints the search it did, what it landed on, and what
//! came out of it. It takes the same arguments Chisel does and runs the same
//! discovery, so its answer is the editor's answer.
fn main() {
    let mut map: Option<std::path::PathBuf> = None;
    let mut explicit: Option<std::path::PathBuf> = None;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--content" => {
                i += 1;
                explicit = args.get(i).map(std::path::PathBuf::from);
            }
            other => map = Some(std::path::PathBuf::from(other)),
        }
        i += 1;
    }

    let found = chisel::content::find(explicit.as_deref(), map.as_deref());
    println!("discovery    : {}", chisel::content::describe(&found));
    let root = found.map(|f| f.root).unwrap_or_default();
    println!("content root : {}", root.display());

    let app = chisel::app::ChiselApp::new(root);
    println!("status       : {}", app.status);
    println!("schema       : {} classes", app.schema.len());
    println!("point classes: {:?}", app.point_classes());
    println!("brush classes: {:?}", app.brush_classes());
    println!("materials    : {} -> {:?}", app.materials.len(), app.materials);

    for class in ["light", "ambient_generic", "logic_script", "func_door"] {
        match app.schema.get(class) {
            Some(spec) => println!(
                "  {class}: kind {:?}, {} keys {:?}",
                spec.kind,
                spec.keys.len(),
                spec.keys.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(),
            ),
            None => println!("  {class}: NOT IN SCHEMA"),
        }
    }
}
