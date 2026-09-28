use super::*;
use crate::mesh::Surface;

/// A 256-unit floor at z = 0, as two triangles, normal up.
fn floor() -> WorldMesh {
    let v = |x: f32, y: f32| WorldVertex {
        position: [x, y, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 0.0],
        lightmap_uv: [x / 256.0, y / 256.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
        probe: 0,
    };
    WorldMesh {
        vertices: vec![
            v(-128.0, -128.0),
            v(128.0, -128.0),
            v(128.0, 128.0),
            v(-128.0, 128.0),
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
        surfaces: vec![Surface {
            face: 0,
            first_index: 0,
            index_count: 6,
            material: 0,
            bounds: Aabb::new(Vec3::new(-128.0, -128.0, 0.0), Vec3::new(128.0, 128.0, 0.0)),
            flags: 0,
            lit: true,
        }],
        materials: vec!["dev/floor".into()],
        leaf_surfaces: Vec::new(),
        model_surfaces: Vec::new(),
        model_bounds: Vec::new(),
        batches: Vec::new(),
    }
}

#[test]
fn a_decal_is_clipped_to_its_square() {
    let spec = DecalSpec::new("decals/bullet", Vec3::new(10.0, 20.0, 0.0), Vec3::Z, 8.0);
    let tris = build(&floor(), &spec, None);
    assert!(!tris.is_empty() && tris.len().is_multiple_of(3));
    for v in &tris {
        assert!((v.position[0] - 10.0).abs() <= 4.001, "{v:?}");
        assert!((v.position[1] - 20.0).abs() <= 4.001);
        assert!((v.position[2] - LIFT).abs() < 1e-4, "lifted off the floor");
        assert!((0.0..=1.0).contains(&v.uv[0]) && (0.0..=1.0).contains(&v.uv[1]));
    }
    // The lightmap coordinate is the floor's, interpolated.
    let v = tris[0];
    assert!((v.lightmap_uv[0] - v.position[0] / 256.0).abs() < 1e-4);
}

#[test]
fn the_decal_covers_its_whole_area() {
    let spec = DecalSpec::new("d", Vec3::ZERO, Vec3::Z, 16.0);
    let tris = build(&floor(), &spec, None);
    let area: f32 = tris
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let p = t
                .iter()
                .map(|v| Vec3::from_array(v.position))
                .collect::<Vec<_>>();
            (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5
        })
        .sum();
    assert!((area - 256.0).abs() < 0.01, "{area}");
}

#[test]
fn surfaces_facing_away_or_out_of_reach_take_nothing() {
    let mesh = floor();
    // Projected upward from below: the floor faces away.
    assert!(build(&mesh, &DecalSpec::new("d", Vec3::ZERO, -Vec3::Z, 8.0), None).is_empty());
    // Far above the floor, beyond the decal's depth.
    assert!(
        build(
            &mesh,
            &DecalSpec::new("d", Vec3::new(0.0, 0.0, 64.0), Vec3::Z, 8.0),
            None
        )
        .is_empty()
    );
    // Past the edge.
    assert!(
        build(
            &mesh,
            &DecalSpec::new("d", Vec3::new(500.0, 0.0, 0.0), Vec3::Z, 8.0),
            None
        )
        .is_empty()
    );
}

#[test]
fn the_basis_is_orthonormal_whatever_the_normal() {
    for n in [Vec3::Z, Vec3::X, Vec3::new(1.0, 1.0, 0.2), -Vec3::Z] {
        let mut spec = DecalSpec::new("d", Vec3::ZERO, n, 8.0);
        spec.rotation = 30.0;
        let (r, u, n) = spec.basis();
        assert!(r.dot(u).abs() < 1e-5 && r.dot(n).abs() < 1e-5 && u.dot(n).abs() < 1e-5);
        assert!((r.length() - 1.0).abs() < 1e-5 && (u.length() - 1.0).abs() < 1e-5);
    }
}
