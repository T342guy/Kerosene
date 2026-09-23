// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
use super::*;

/// Append `values` as little-endian bytes.
fn put<T: bytemuck::Pod>(buf: &mut Vec<u8>, values: &[T]) -> (usize, usize) {
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
    let offset = buf.len();
    buf.extend_from_slice(bytemuck::cast_slice(values));
    (offset, buf.len() - offset)
}

/// A binary glTF: a 1 x 2 metre quad standing on the origin in glTF's Y-up
/// space, skinned to a root bone and an "arm" one metre up, with a "raise"
/// animation turning the arm a quarter turn about glTF's Z over a second.
fn skinned_glb() -> Vec<u8> {
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 4] = [
        [-0.5, 0.0, 0.0],
        [0.5, 0.0, 0.0],
        [0.5, 2.0, 0.0],
        [-0.5, 2.0, 0.0],
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let joints: [[u8; 4]; 4] = [[0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]];
    let weights: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0]; 4];
    let ibm: [[f32; 16]; 2] = [
        Mat4::IDENTITY.to_cols_array(),
        Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0)).to_cols_array(),
    ];
    let times: [f32; 2] = [0.0, 1.0];
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let rotations: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, s, s]];

    let views: Vec<(usize, usize)> = vec![
        put(&mut bin, &positions),
        put(&mut bin, &indices),
        put(&mut bin, &joints),
        put(&mut bin, &weights),
        put(&mut bin, &ibm),
        put(&mut bin, &times),
        put(&mut bin, &rotations),
    ];
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let view_json: Vec<String> = views
        .iter()
        .map(|(o, l)| format!(r#"{{"buffer":0,"byteOffset":{o},"byteLength":{l}}}"#))
        .collect();
    let json = format!(
        r#"{{
"asset":{{"version":"2.0"}},
"scene":0,
"scenes":[{{"nodes":[0,2]}}],
"nodes":[
 {{"name":"root","children":[1]}},
 {{"name":"arm","translation":[0,1,0]}},
 {{"name":"body","mesh":0,"skin":0}}
],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"JOINTS_0":2,"WEIGHTS_0":3}},"indices":1,"material":0}}]}}],
"materials":[{{"name":"props/skin"}}],
"skins":[{{"joints":[0,1],"inverseBindMatrices":4}}],
"animations":[{{"name":"raise","channels":[{{"sampler":0,"target":{{"node":1,"path":"rotation"}}}}],
  "samplers":[{{"input":5,"output":6,"interpolation":"LINEAR"}}]}}],
"accessors":[
 {{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3","min":[-0.5,0,0],"max":[0.5,2,0]}},
 {{"bufferView":1,"componentType":5123,"count":6,"type":"SCALAR"}},
 {{"bufferView":2,"componentType":5121,"count":4,"type":"VEC4"}},
 {{"bufferView":3,"componentType":5126,"count":4,"type":"VEC4"}},
 {{"bufferView":4,"componentType":5126,"count":2,"type":"MAT4"}},
 {{"bufferView":5,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]}},
 {{"bufferView":6,"componentType":5126,"count":2,"type":"VEC4"}}
],
"bufferViews":[{views}],
"buffers":[{{"byteLength":{len}}}]
}}"#,
        views = view_json.join(","),
        len = bin.len()
    );
    let mut json = json.into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }

    let mut glb = Vec::new();
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    let total = 12 + 8 + json.len() + 8 + bin.len();
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);
    glb
}

fn import_fixture(once: &[String]) -> Model {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forge-gltf-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("arm.glb");
    std::fs::write(&path, skinned_glb()).unwrap();
    let renames = HashMap::new();
    let model = import(
        &path,
        &ImportOptions {
            scale: 1.0,
            default_material: "dev/grid",
            renames: &renames,
            once,
        },
    )
    .expect("the fixture imports");
    let _ = std::fs::remove_dir_all(&dir);
    model
}

#[test]
fn a_skinned_mesh_comes_in_z_up_in_inches_with_its_bones() {
    let m = import_fixture(&[]);
    m.validate().unwrap();
    let ku = kerosene_math::units::KU_PER_METRE;

    // Two metres tall along glTF's Y is two metres up Kerosene's Z.
    assert!((m.bounds.max.z - 2.0 * ku).abs() < 1e-3, "{:?}", m.bounds);
    assert!(m.bounds.min.z.abs() < 1e-3);
    assert_eq!(m.mesh_material(0), "props/skin");

    assert_eq!(m.bones.len(), 2);
    assert_eq!(m.bone_name(0), "root");
    assert_eq!(m.bone_name(1), "arm");
    assert_eq!(m.bones[1].parent, 0);
    let arm = Vec3::from_array(m.bones[1].position);
    assert!(arm.abs_diff_eq(Vec3::new(0.0, 0.0, ku), 1e-3), "{arm}");

    // The top corners follow the arm, wholly; the bottom ones the root.
    for v in &m.vertices {
        let expected = if v.position[2] > ku { 1 } else { 0 };
        assert_eq!(v.bone_indices[0], expected);
        assert_eq!(v.bone_weights, [255, 0, 0, 0]);
    }
}

#[test]
fn an_animation_is_resampled_and_converted_to_kerosene_axes() {
    let m = import_fixture(&[]);
    assert_eq!(m.animations.len(), 1);
    let a = &m.animations[0];
    assert_eq!(a.name, "raise");
    assert_eq!(a.fps, SAMPLE_RATE);
    assert_eq!(a.frame_count, 31, "one second at 30 frames, both ends");
    assert!(a.looping);

    // A quarter turn about glTF's Z is a quarter turn about Kerosene's X.
    let last = Quat::from_array(a.frame(30, 2)[1].rotation);
    let expected = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    assert!(
        last.abs_diff_eq(expected, 1e-4) || last.abs_diff_eq(-expected, 1e-4),
        "{last}"
    );
    // Halfway is halfway.
    let mid = Quat::from_array(a.frame(15, 2)[1].rotation);
    assert!(mid.angle_between(Quat::IDENTITY) - std::f32::consts::FRAC_PI_4 < 1e-3);
    // The arm keeps its rest translation throughout.
    let ku = kerosene_math::units::KU_PER_METRE;
    assert!(
        Vec3::from_array(a.frame(15, 2)[1].translation).abs_diff_eq(Vec3::new(0.0, 0.0, ku), 1e-3)
    );
}

#[test]
fn an_animation_named_once_holds_instead_of_looping() {
    let m = import_fixture(&["RAISE".to_string()]);
    assert!(!m.animations[0].looping);
}

#[test]
fn weights_quantise_to_exactly_255_strongest_first() {
    let (i, w) = quantise_weights([3, 1, 0, 0], [0.2, 0.5, 0.3, 0.0], &[10, 11, 12, 13]);
    assert_eq!(i[0], 11, "the strongest influence first");
    assert_eq!(w.iter().map(|&b| b as u32).sum::<u32>(), 255);
}
