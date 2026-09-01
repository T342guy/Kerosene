// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Every shape the shape tool draws, built and rendered, in one picture.
//!
//! ```text
//! cargo run -p chisel --example shape_sheet -- shapes.png
//! ```
//!
//! Geometry has a way of being valid and still wrong -- an arch whose
//! segments meet at the wrong radius passes every assertion about convexity
//! and still looks like a broken fan. A test can tell you the brushes are
//! solid. Only a picture can tell you they are an arch.
use chisel::raster::{Settings, Shading, render_with};
use chisel::shapes::{Options, Shape};
use kerosene_math::{Aabb, Vec3};

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "shapes.png".into());

    let mut document = chisel::Document::new();
    document.map.world.solids.clear();

    // A floor to stand them on, so the shapes read as objects in a room
    // rather than as diagrams floating in the dark.
    document.create_block(Vec3::new(-128.0, -128.0, -16.0), Vec3::new(1800.0, 1000.0, 0.0));

    // Two rows: drawn from the top, and drawn from the front. Which pane a
    // shape is drawn in decides which way it stands, and that rule is far
    // easier to check in a picture than in an assertion about a bounding box.
    let mut x = 0.0;
    for shape in Shape::all() {
        for (row, axis) in [(0.0, 2usize), (620.0, 1usize)] {
            let min = Vec3::new(x, row, 0.0);
            let bounds = Aabb::new(min, min + Vec3::splat(256.0));
            let solids = chisel::shapes::build(shape, bounds, axis, Options::default(), "dev/grid");
            if row == 0.0 {
                println!("{:9} {} brush(es)", shape.label(), solids.len());
            }
            document.create_shape(solids, shape.label());
        }
        x += 340.0;
    }
    document.selection.clear();

    // Framed on the row rather than aimed by hand: the shapes are laid out
    // in a line of known length, so the camera can be put where the whole
    // line fits and pointed at the middle of it.
    let middle = Vec3::new((x - 340.0 + 256.0) * 0.5, 440.0, 90.0);
    let back = x * 0.85;
    let eye = Vec3::new(middle.x, middle.y - back, middle.z + back * 0.60);
    let angles = kerosene_math::Angles::from_direction(middle - eye);

    let mut settings = Settings { shading: Shading::Flat, resolve: None };
    let image = render_with(&document, eye, angles.vectors(), 75.0, 1400, 600, &mut settings);

    let mut buffer = image::RgbaImage::new(image.width as u32, image.height as u32);
    for (i, p) in image.pixels.iter().enumerate() {
        buffer.put_pixel((i % image.width) as u32, (i / image.width) as u32, image::Rgba(*p));
    }
    buffer.save(&out)?;
    println!("wrote {out}");
    Ok(())
}
