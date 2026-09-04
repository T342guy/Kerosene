// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Forge -- the Kerosene model compiler.
//!
//! Turns a source mesh into a `.keromdl` the engine can load, the studiomdl
//! analogue. It does the work that should happen once at build time rather
//! than on every load: welding vertices, splitting by material, computing
//! normals where the source has none, and converting axes and units.
//!
//! ```text
//! forge compile art/crate.obj -o models/props/crate.keromdl --scale-metres
//! forge info models/props/crate.keromdl
//! ```

mod obj;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use obj::{ObjMesh, UpAxis, VU_PER_METRE};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use kerosene_asset::{Mesh, Model, Vertex};
use kerosene_math::Vec3;

#[derive(Parser, Debug)]
#[command(name = "forge", version, about = "Compile source meshes into .keromdl models")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile an OBJ into a .keromdl.
    Compile {
        source: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Material for faces the source does not name one for.
        #[arg(long, default_value = "dev/grid")]
        default_material: String,

        /// Rename a source material: `--material old=new`. Repeatable.
        #[arg(long = "material")]
        materials: Vec<String>,

        /// Uniform scale applied to positions.
        #[arg(long, default_value_t = 1.0)]
        scale: f32,

        /// Treat the source as metres and convert to kerosene units.
        #[arg(long)]
        scale_metres: bool,

        /// The source is already Z-up; skip the axis conversion.
        #[arg(long)]
        z_up: bool,

        /// Recompute normals from face geometry, ignoring any in the source.
        #[arg(long)]
        recompute_normals: bool,
    },
    /// Describe a compiled model.
    Info { model: PathBuf },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    match Args::parse().command {
        Command::Compile {
            source, output, default_material, materials, scale, scale_metres, z_up,
            recompute_normals,
        } => {
            let out = output.unwrap_or_else(|| source.with_extension("keromdl"));
            let scale = scale * if scale_metres { VU_PER_METRE } else { 1.0 };
            let up = if z_up { UpAxis::Z } else { UpAxis::Y };
            compile(&source, &out, &default_material, &materials, scale, up, recompute_normals)
        }
        Command::Info { model } => info(&model),
    }
}

#[allow(clippy::too_many_arguments)]
fn compile(
    source: &Path,
    out: &Path,
    default_material: &str,
    material_renames: &[String],
    scale: f32,
    up: UpAxis,
    recompute_normals: bool,
) -> Result<()> {
    let text = std::fs::read_to_string(source)
        .with_context(|| format!("reading {}", source.display()))?;
    let mesh = ObjMesh::parse(&text, up, scale)
        .with_context(|| format!("parsing {}", source.display()))?;

    let mut renames: HashMap<&str, &str> = HashMap::new();
    for pair in material_renames {
        let Some((from, to)) = pair.split_once('=') else {
            bail!("--material expects old=new, got {pair:?}");
        };
        renames.insert(from, to);
    }

    println!("forge: {} -- {} positions, {} triangles, {} material groups",
        source.display(), mesh.positions.len(), mesh.triangle_count(), mesh.groups.len());

    let mut model = Model::new();
    // Weld by the full corner tuple, not by position: two faces meeting at a
    // hard edge legitimately share a position while needing different normals,
    // and merging them would round off every corner of the model.
    let mut seen: HashMap<obj::Corner, u32> = HashMap::new();

    for group in &mesh.groups {
        let name = renames
            .get(group.material.as_str())
            .copied()
            .unwrap_or(if group.material == "default" { default_material } else { &group.material });
        let material_offset = model.intern(name);

        let first_index = model.indices.len() as u32;
        for triangle in &group.triangles {
            // A face normal, used where the source provides none.
            let face_normal = face_normal(&mesh, triangle);
            for corner in triangle {
                let index = match seen.get(corner) {
                    Some(&i) if !recompute_normals && corner.normal.is_some() => i,
                    _ => {
                        let position = mesh.positions[corner.position];
                        let normal = match corner.normal {
                            Some(n) if !recompute_normals => mesh.normals[n],
                            _ => face_normal,
                        };
                        let uv = corner.uv.map(|i| mesh.uvs[i]).unwrap_or([0.0, 0.0]);
                        let i = model.vertices.len() as u32;
                        model.vertices.push(Vertex::rigid(position, normal, uv));
                        if corner.normal.is_some() && !recompute_normals {
                            seen.insert(*corner, i);
                        }
                        i
                    }
                };
                model.indices.push(index);
            }
        }

        let index_count = model.indices.len() as u32 - first_index;
        if index_count == 0 { continue; }
        model.meshes.push(Mesh { first_index, index_count, material_offset, flags: 0 });
    }

    model.recompute_bounds();
    model.validate().context("the compiled model is inconsistent")?;

    let size = model.bounds.size();
    println!("  {} vertices, {} triangles, {} meshes",
        model.vertices.len(), model.triangle_count(), model.meshes.len());
    println!("  bounds {:.1} x {:.1} x {:.1} inches", size.x, size.y, size.z);
    println!("  materials: {}", model.materials().join(", "));

    if let Some(parent) = out.parent() { std::fs::create_dir_all(parent)?; }
    let bytes = model.to_bytes();
    std::fs::write(out, &bytes).with_context(|| format!("writing {}", out.display()))?;
    println!("  wrote {} ({:.1} KiB)", out.display(), bytes.len() as f64 / 1024.0);
    Ok(())
}

/// Normal of a triangle, from its winding.
fn face_normal(mesh: &ObjMesh, triangle: &[obj::Corner; 3]) -> Vec3 {
    let a = mesh.positions[triangle[0].position];
    let b = mesh.positions[triangle[1].position];
    let c = mesh.positions[triangle[2].position];
    let n = (b - a).cross(c - a);
    if n.length_squared() < 1e-12 { Vec3::Z } else { n.normalize() }
}

fn info(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let model = Model::from_bytes(&bytes)?;
    let size = model.bounds.size();

    println!("{}", path.display());
    println!("  {} vertices, {} triangles", model.vertices.len(), model.triangle_count());
    println!("  {} meshes, {} bones", model.meshes.len(), model.bones.len());
    println!("  bounds {:.1} x {:.1} x {:.1} inches", size.x, size.y, size.z);
    println!("  {:.1} KiB", bytes.len() as f64 / 1024.0);
    for i in 0..model.meshes.len() {
        let m = &model.meshes[i];
        println!("    mesh {i}: {} triangles, material {}", m.index_count / 3, model.mesh_material(i));
    }
    for i in 0..model.bones.len() {
        let b = &model.bones[i];
        println!("    bone {i}: {} (parent {})", model.bone_name(i), b.parent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped prop models must not be inside out.
    ///
    /// OBJ carries an explicit normal per corner, but the GPU culls by the
    /// face's *winding*. A model whose winding disagrees with its normals
    /// still compiles and still gets physics, but renders inverted: back-face
    /// culling shows the inside of every face. This loads the real art files
    /// and checks that each triangle's winding points the same way its normal
    /// does, so a wrongly-wound export fails here instead of in a screenshot.
    #[test]
    fn the_shipped_prop_models_wind_the_way_their_normals_point() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
        for name in ["cube", "crate"] {
            let path = root.join(format!("art/props/{name}.obj"));
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let mesh = ObjMesh::parse(&text, UpAxis::Y, VU_PER_METRE).unwrap();

            let mut checked = 0;
            for group in &mesh.groups {
                for triangle in &group.triangles {
                    let p = [
                        mesh.positions[triangle[0].position],
                        mesh.positions[triangle[1].position],
                        mesh.positions[triangle[2].position],
                    ];
                    let wound = (p[1] - p[0]).cross(p[2] - p[0]);
                    assert!(
                        wound.length_squared() > 1e-12,
                        "{name}: degenerate triangle at indices {:?}",
                        triangle
                    );
                    // Every corner of a prop face shares one normal, so the
                    // first corner speaks for the whole triangle.
                    if let Some(normal) = triangle[0].normal {
                        let normal = mesh.normals[normal];
                        assert!(
                            wound.normalize().dot(normal) > 0.9,
                            "{name}: a face winds against its normal ({:?} vs {:?})",
                            wound.normalize(),
                            normal
                        );
                        checked += 1;
                    }
                }
            }
            assert!(checked >= 12, "{name}: expected every face to carry a normal");
        }
    }
}
