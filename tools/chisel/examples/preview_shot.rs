// SPDX-License-Identifier: LGPL-3.0-or-later
//! Render a map through Chisel's 3D pane and write it out as a PNG.
//!
//! ```text
//! cargo run -p chisel --example preview_shot -- <map.keromap> <out.png> [x y z yaw pitch]
//! ```
//!
//! The 3D pane is software-rasterised, which means it can be run without a
//! window -- so what the editor shows can be looked at outside it, attached to
//! a bug report, or diffed between two builds. An editor whose only output is
//! a window is one whose rendering can only ever be checked by a person.
use anyhow::Result;
use chisel::raster::{Settings, Shading};
use chisel::textures::TextureCache;
use kerosene_math::{Angles, Vec3};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let map = args.first().map(String::as_str).unwrap_or("content/maps/kero_start.keromap");
    let out = args.get(1).map(String::as_str).unwrap_or("preview.png");
    let number = |i: usize, fallback: f32| -> f32 {
        args.get(i).and_then(|v| v.parse().ok()).unwrap_or(fallback)
    };
    let eye = Vec3::new(number(2, 80.0), number(3, 80.0), number(4, 96.0));

    let map_path = std::path::PathBuf::from(map);
    let document = chisel::Document::open(map_path.clone())?;

    // The same search the editor does, so a shot taken from anywhere shows
    // the same textures the editor would.
    let found = kerosene_vfs::root::find(None, Some(&map_path));
    eprintln!("{}", kerosene_vfs::root::describe(&found));
    let mut vfs = kerosene_vfs::Vfs::new();
    if let Some(found) = &found {
        vfs.add_directory(&found.root, "GAME");
    }
    let mut cache = TextureCache::new();

    let (w, h) = (960usize, 600usize);
    let mut resolve = |material: &str| cache.get(&vfs, material);
    let mut settings = Settings { shading: Shading::Textured, resolve: Some(&mut resolve) };
    let image = chisel::raster::render_with(
        &document,
        eye,
        Angles::new(number(6, 10.0), number(5, 35.0), 0.0).vectors(),
        90.0,
        w,
        h,
        &mut settings,
    );

    let flat: Vec<u8> = image.pixels.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    image::RgbImage::from_raw(w as u32, h as u32, flat)
        .expect("dimensions match")
        .save(out)?;
    println!("wrote {out} ({} textures, {} problems)", cache.len(), cache.problem_count());
    Ok(())
}
