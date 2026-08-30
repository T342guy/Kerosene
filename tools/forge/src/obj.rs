// SPDX-License-Identifier: LGPL-3.0-or-later
//! Reading Wavefront OBJ, Forge's source mesh format.
//!
//! OBJ is chosen for the same reason `.keromap` is text: it is what every
//! modelling package can export, it is readable, and it diffs. It carries
//! positions, normals, texture coordinates and material groups, which is
//! everything a static model needs.
//!
//! Two conversions happen on the way in, and getting either wrong produces a
//! model that is subtly rotated or a hundred times too small:
//!
//! * **Axes.** OBJ is Y-up with -Z forward. Kerosene is Z-up with +X
//!   forward and +Y left. The remap preserves handedness, so faces do not end
//!   up inside out.
//! * **Scale.** Modelling packages usually work in metres; Kerosene works in
//!   inches. A crate exported at 1.0 units is 1 inch here unless scaled.

use std::collections::HashMap;
use thiserror::Error;
use kerosene_math::Vec3;

/// Kerosene units per metre, for the common case of a metric source file.
///
/// Re-exported from `kerosene-math` rather than written out again: the scale of
/// the world is one fact, and two copies of it is one copy too many.
pub use kerosene_math::units::VU_PER_METRE;

#[derive(Debug, Error)]
pub enum ObjError {
    #[error("line {line}: {detail}")]
    Malformed { line: usize, detail: String },
    #[error("the file contains no faces")]
    Empty,
}

/// Which axis the source file treats as up.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum UpAxis {
    /// The usual export convention.
    #[default]
    Y,
    /// Already in Kerosene's orientation.
    Z,
}

/// One triangle corner, referring to the file's shared arrays.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Corner {
    pub position: usize,
    pub uv: Option<usize>,
    pub normal: Option<usize>,
}

/// A run of triangles sharing one material.
#[derive(Clone, Debug)]
pub struct Group {
    pub material: String,
    pub triangles: Vec<[Corner; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct ObjMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<[f32; 2]>,
    pub groups: Vec<Group>,
}

impl ObjMesh {
    pub fn triangle_count(&self) -> usize {
        self.groups.iter().map(|g| g.triangles.len()).sum()
    }

    /// Parse an OBJ, converting axes and scaling as it goes.
    pub fn parse(text: &str, up: UpAxis, scale: f32) -> Result<ObjMesh, ObjError> {
        let mut mesh = ObjMesh::default();
        let mut groups: Vec<Group> = Vec::new();
        let mut current = String::from("default");
        let mut by_material: HashMap<String, usize> = HashMap::new();

        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() { continue; }
            let mut parts = line.split_whitespace();
            let Some(keyword) = parts.next() else { continue };
            let rest: Vec<&str> = parts.collect();

            let bad = |detail: &str| ObjError::Malformed { line: n + 1, detail: detail.to_string() };

            match keyword {
                "v" => {
                    if rest.len() < 3 { return Err(bad("a vertex needs three coordinates")); }
                    let v = parse3(&rest, n)?;
                    mesh.positions.push(convert(v, up) * scale);
                }
                "vn" => {
                    if rest.len() < 3 { return Err(bad("a normal needs three components")); }
                    // Normals are directions: rotated, never scaled.
                    mesh.normals.push(convert(parse3(&rest, n)?, up).normalize_or_zero());
                }
                "vt" => {
                    if rest.len() < 2 { return Err(bad("a texture coordinate needs two components")); }
                    let u: f32 = rest[0].parse().map_err(|_| bad("unreadable u"))?;
                    let v: f32 = rest[1].parse().map_err(|_| bad("unreadable v"))?;
                    // OBJ's V axis runs upward; texture space runs downward.
                    mesh.uvs.push([u, 1.0 - v]);
                }
                "usemtl" => {
                    current = rest.first().copied().unwrap_or("default").to_string();
                }
                "f" => {
                    if rest.len() < 3 { return Err(bad("a face needs at least three corners")); }
                    let corners: Vec<Corner> = rest
                        .iter()
                        .map(|token| parse_corner(token, &mesh, n))
                        .collect::<Result<_, _>>()?;

                    let index = *by_material.entry(current.clone()).or_insert_with(|| {
                        groups.push(Group { material: current.clone(), triangles: Vec::new() });
                        groups.len() - 1
                    });
                    // Fan-triangulate. OBJ faces are convex by convention, and
                    // a fan is correct for any convex polygon.
                    for i in 1..corners.len() - 1 {
                        groups[index].triangles.push([corners[0], corners[i], corners[i + 1]]);
                    }
                }
                // Object and group names, material libraries, smoothing groups:
                // recorded by exporters, not needed to build the mesh.
                "o" | "g" | "s" | "mtllib" => {}
                _ => {}
            }
        }

        mesh.groups = groups;
        if mesh.triangle_count() == 0 { return Err(ObjError::Empty); }
        Ok(mesh)
    }
}

fn parse3(parts: &[&str], line: usize) -> Result<Vec3, ObjError> {
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        out[i] = parts[i].parse().map_err(|_| ObjError::Malformed {
            line: line + 1,
            detail: format!("{:?} is not a number", parts[i]),
        })?;
    }
    Ok(Vec3::from_array(out))
}

/// Remap a vector from the source file's axes into Kerosene's.
fn convert(v: Vec3, up: UpAxis) -> Vec3 {
    match up {
        // OBJ: +X right, +Y up, -Z forward.
        // Kerosene: +X forward, +Y left, +Z up.
        UpAxis::Y => Vec3::new(-v.z, -v.x, v.y),
        UpAxis::Z => v,
    }
}

/// Parse `v`, `v/vt`, `v//vn` or `v/vt/vn`, resolving 1-based and negative
/// indices.
fn parse_corner(token: &str, mesh: &ObjMesh, line: usize) -> Result<Corner, ObjError> {
    let bad = |detail: String| ObjError::Malformed { line: line + 1, detail };
    let mut fields = token.split('/');

    let resolve = |raw: &str, count: usize| -> Option<usize> {
        let value: i64 = raw.parse().ok()?;
        match value.cmp(&0) {
            // Negative indices count backward from the end, which exporters
            // use for streaming output.
            std::cmp::Ordering::Less => count.checked_sub((-value) as usize),
            std::cmp::Ordering::Greater => Some(value as usize - 1),
            std::cmp::Ordering::Equal => None,
        }
        .filter(|&i| i < count)
    };

    let position_raw = fields.next().unwrap_or("");
    let position = resolve(position_raw, mesh.positions.len())
        .ok_or_else(|| bad(format!("vertex index {position_raw:?} is out of range")))?;

    let uv = match fields.next() {
        Some(s) if !s.is_empty() => Some(
            resolve(s, mesh.uvs.len())
                .ok_or_else(|| bad(format!("texture index {s:?} is out of range")))?,
        ),
        _ => None,
    };
    let normal = match fields.next() {
        Some(s) if !s.is_empty() => Some(
            resolve(s, mesh.normals.len())
                .ok_or_else(|| bad(format!("normal index {s:?} is out of range")))?,
        ),
        _ => None,
    };

    Ok(Corner { position, uv, normal })
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUAD: &str = "\
# a unit quad on the ground, Y-up
v 0 0 0
v 1 0 0
v 1 0 -1
v 0 0 -1
vt 0 0
vt 1 0
vt 1 1
vt 0 1
vn 0 1 0
usemtl props/wood
f 1/1/1 2/2/1 3/3/1 4/4/1
";

    #[test]
    fn a_quad_becomes_two_triangles() {
        let m = ObjMesh::parse(QUAD, UpAxis::Y, 1.0).unwrap();
        assert_eq!(m.positions.len(), 4);
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.groups[0].material, "props/wood");
        assert_eq!(m.triangle_count(), 2);
    }

    #[test]
    fn y_up_becomes_z_up() {
        // The source quad lies in the OBJ ground plane, so it should end up
        // flat in Z with its normal pointing up.
        let m = ObjMesh::parse(QUAD, UpAxis::Y, 1.0).unwrap();
        assert!(m.positions.iter().all(|p| p.z == 0.0), "{:?}", m.positions);
        assert!((m.normals[0] - Vec3::Z).length() < 1e-5, "{:?}", m.normals[0]);
    }

    #[test]
    fn the_axis_remap_preserves_handedness() {
        // If it did not, every face would come out inside out.
        let x = convert(Vec3::X, UpAxis::Y);
        let y = convert(Vec3::Y, UpAxis::Y);
        let z = convert(Vec3::Z, UpAxis::Y);
        assert!((x.cross(y) - z).length() < 1e-6, "{x:?} x {y:?} != {z:?}");
    }

    #[test]
    fn z_up_sources_are_left_alone() {
        let m = ObjMesh::parse("v 1 2 3\nvn 0 0 1\nf 1 1 1\n", UpAxis::Z, 1.0).unwrap();
        assert_eq!(m.positions[0], Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn scale_applies_to_positions_but_not_normals() {
        let m = ObjMesh::parse(QUAD, UpAxis::Y, 10.0).unwrap();
        assert_eq!(m.positions[1].y, -10.0, "{:?}", m.positions[1]);
        assert!((m.normals[0].length() - 1.0).abs() < 1e-5, "normals must stay unit length");
    }

    #[test]
    fn the_v_axis_is_flipped_into_texture_space() {
        let m = ObjMesh::parse(QUAD, UpAxis::Y, 1.0).unwrap();
        assert_eq!(m.uvs[0], [0.0, 1.0]);
        assert_eq!(m.uvs[2], [1.0, 0.0]);
    }

    #[test]
    fn faces_split_by_material() {
        let src = "\
v 0 0 0
v 1 0 0
v 1 0 1
usemtl a
f 1 2 3
usemtl b
f 1 2 3
usemtl a
f 3 2 1
";
        let m = ObjMesh::parse(src, UpAxis::Y, 1.0).unwrap();
        assert_eq!(m.groups.len(), 2);
        // Returning to a material must reuse its group, not make a third.
        let a = m.groups.iter().find(|g| g.material == "a").unwrap();
        assert_eq!(a.triangles.len(), 2);
    }

    #[test]
    fn faces_without_uvs_or_normals_are_accepted() {
        let m = ObjMesh::parse("v 0 0 0\nv 1 0 0\nv 1 0 1\nf 1 2 3\n", UpAxis::Y, 1.0).unwrap();
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(m.groups[0].triangles[0][0].uv, None);
        assert_eq!(m.groups[0].triangles[0][0].normal, None);
    }

    #[test]
    fn the_v_slash_slash_n_form_is_understood() {
        let src = "v 0 0 0\nv 1 0 0\nv 1 0 1\nvn 0 1 0\nf 1//1 2//1 3//1\n";
        let m = ObjMesh::parse(src, UpAxis::Y, 1.0).unwrap();
        let c = m.groups[0].triangles[0][0];
        assert_eq!(c.normal, Some(0));
        assert_eq!(c.uv, None);
    }

    #[test]
    fn negative_indices_count_back_from_the_end() {
        let src = "v 0 0 0\nv 1 0 0\nv 1 0 1\nf -3 -2 -1\n";
        let m = ObjMesh::parse(src, UpAxis::Y, 1.0).unwrap();
        let t = m.groups[0].triangles[0];
        assert_eq!([t[0].position, t[1].position, t[2].position], [0, 1, 2]);
    }

    #[test]
    fn an_n_gon_is_fan_triangulated() {
        let src = "v 0 0 0\nv 1 0 0\nv 2 0 1\nv 1 0 2\nv 0 0 2\nf 1 2 3 4 5\n";
        let m = ObjMesh::parse(src, UpAxis::Y, 1.0).unwrap();
        assert_eq!(m.triangle_count(), 3, "a pentagon is three triangles");
    }

    #[test]
    fn comments_and_ignored_directives_do_not_break_parsing() {
        let src = "# comment\nmtllib x.mtl\no thing\ng part\ns 1\nv 0 0 0\nv 1 0 0\nv 1 0 1\nf 1 2 3\n";
        assert_eq!(ObjMesh::parse(src, UpAxis::Y, 1.0).unwrap().triangle_count(), 1);
    }

    #[test]
    fn malformed_files_are_rejected_with_a_line_number() {
        assert!(matches!(ObjMesh::parse("v 0 0\n", UpAxis::Y, 1.0), Err(ObjError::Malformed { line: 1, .. })));
        assert!(matches!(ObjMesh::parse("v a b c\n", UpAxis::Y, 1.0), Err(ObjError::Malformed { line: 1, .. })));
        assert!(matches!(
            ObjMesh::parse("v 0 0 0\nf 1 2 3\n", UpAxis::Y, 1.0),
            Err(ObjError::Malformed { line: 2, .. })
        ));
        assert!(matches!(ObjMesh::parse("v 0 0 0\n", UpAxis::Y, 1.0), Err(ObjError::Empty)));
    }
}
