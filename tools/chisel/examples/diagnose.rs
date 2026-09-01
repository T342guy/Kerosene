// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! What Chisel sees when it starts, without opening a window.
//!
//! ```text
//! cargo run -p chisel --example diagnose -- [map.keromap] [--content <dir>] [--build]
//! ```
//!
//! For the one question a level editor cannot answer for itself: *why is there
//! nothing in here?* An editor that found no content looks identical to one
//! whose content is empty -- three hard-coded class names, flat-coloured
//! brushes -- so this prints the search it did, what it landed on, and what
//! came out of it. It takes the same arguments Chisel does and runs the same
//! discovery, so its answer is the editor's answer.
//!
//! It does not build anything unless asked with `--build`. Chisel builds the
//! textures on startup; a diagnostic that quietly changed what it was
//! diagnosing would be no use for working out why they were missing.
fn main() {
    let mut map: Option<std::path::PathBuf> = None;
    let mut explicit: Option<std::path::PathBuf> = None;
    let mut build = false;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--content" => {
                i += 1;
                explicit = args.get(i).map(std::path::PathBuf::from);
            }
            "--build" => build = true,
            other => map = Some(std::path::PathBuf::from(other)),
        }
        i += 1;
    }

    let found = kerosene_vfs::root::find(explicit.as_deref(), map.as_deref());
    println!("discovery    : {}", kerosene_vfs::root::describe(&found));
    let root = found.map(|f| f.root).unwrap_or_default();
    println!("content root : {}", root.display());

    if build {
        match alchemy::build_textures(&root) {
            Ok(report) => println!("build        : {report}"),
            Err(e) => println!("build        : FAILED -- {e:#}"),
        }
    }

    let app = chisel::app::ChiselApp::new(root.clone());
    println!("status       : {}", app.status);
    println!("schema       : {} classes", app.schema.len());
    println!("point classes: {:?}", app.point_classes());
    println!("brush classes: {:?}", app.brush_classes());
    println!("materials    : {} -> {:?}", app.materials.len(), app.materials);

    // Every material the editor offers, and whether there is a texture behind
    // it. A material with no texture draws as a flat colour, which is the
    // symptom people report as "textures do not load".
    let mut cache = chisel::textures::TextureCache::new();
    let missing: Vec<&String> = app
        .materials
        .iter()
        .filter(|m| cache.get(&app.vfs, m).is_none())
        .collect();
    println!(
        "textures     : {} of {} materials have one{}",
        app.materials.len() - missing.len(),
        app.materials.len(),
        if missing.is_empty() { String::new() } else { format!("; missing {missing:?}") },
    );

    // Which maps exist, and which of them the game could actually load. A
    // `.keromap` is a source file; only a `.kerobsp` is a level.
    let maps = chisel::files::maps_in(&root);
    println!("maps         : {}", maps.len());
    for map in &maps {
        println!(
            "  {}{}",
            chisel::files::label(map, &root),
            if map.with_extension("kerobsp").is_file() {
                " -- compiled"
            } else {
                " -- never compiled; the game cannot load this one"
            }
        );
    }

    println!("classes      :");

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
