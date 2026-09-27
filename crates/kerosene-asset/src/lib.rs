// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Asset formats: textures, materials and models.
//!
//! These are the *compiled* forms the engine loads. Source art -- PNGs, mesh
//! files -- is turned into them by the tools ([`alchemy`] for textures and
//! materials, [`forge`] for models), which is the same split Source uses and
//! for the same reasons: the engine should never parse a format it did not
//! write, and everything expensive should happen once at build time rather
//! than on every launch.
//!
//! | Format  | Extension | Analogue in Source | Built by |
//! |---------|-----------|--------------------|----------|
//! | Texture | `.ktex`   | VTF                | Alchemy  |
//! | Material| `.kmat`   | VMT                | Alchemy  |
//! | Model   | `.kmdl`   | MDL                | Forge    |
//!
//! Textures have two source forms. A loose image under `art/` compiles to one
//! `.ktex`, which is all a tool texture or a skybox needs. A *folder* under
//! `textures/` compiles to a whole set -- colour, normals, roughness, emissive
//! and occlusion -- plus the material that binds them together; see
//! [`textureset`].
//!
//! [`alchemy`]: https://github.com/t342guy/kerosene
//! [`forge`]: https://github.com/t342guy/kerosene

pub mod material;
pub mod model;
pub mod texture;
pub mod textureset;

pub use material::{
    ACOUSTIC_BANDS_HZ, AcousticProfile, MAX_ABSORPTION, Material, MaterialError, Shader,
    SurfaceProperty,
};
pub use model::{Animation, Bone, BoneKey, Mesh, Model, ModelError, Vertex};
pub use texture::{Mip, PixelFormat, Texture, TextureError, TextureFlags};
pub use textureset::{MapKind, TextureSet};

/// The extensions this crate reads and writes. `kerosene_vfs::ext` is the
/// whole table; this crate sits below it, so it keeps its own copy, and a
/// test in the engine checks the two agree.
pub mod ext {
    pub const TEXTURE: &str = "ktex";
    pub const MATERIAL: &str = "kmat";
    pub const MODEL: &str = "kmdl";
    pub const MAP_SOURCE: &str = "kmap";
    pub const MAP_COMPILED: &str = "kbsp";
    pub const ARCHIVE: &str = "vault";
}

/// Where a material lives, given the name geometry refers to it by.
///
/// Brush faces store `dev/grid`; the file is `materials/dev/grid.kmat`. The
/// prefix and extension are added here rather than being written into every
/// map, so content can be reorganised without rewriting geometry.
pub fn material_path(name: &str) -> String {
    format!(
        "materials/{}.{}",
        name.trim_start_matches('/'),
        ext::MATERIAL
    )
}

/// Where a texture lives, given the name a material refers to it by.
pub fn texture_path(name: &str) -> String {
    format!(
        "materials/{}.{}",
        name.trim_start_matches('/'),
        ext::TEXTURE
    )
}

/// Where a model lives, given the name an entity refers to it by.
pub fn model_path(name: &str) -> String {
    let name = name.trim_start_matches('/');
    if name.ends_with(ext::MODEL) {
        name.to_string()
    } else {
        format!("models/{name}.{}", ext::MODEL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_resolve_to_paths() {
        assert_eq!(material_path("dev/grid"), "materials/dev/grid.kmat");
        assert_eq!(texture_path("dev/grid"), "materials/dev/grid.ktex");
        assert_eq!(model_path("props/crate"), "models/props/crate.kmdl");
    }

    #[test]
    fn a_leading_slash_does_not_produce_a_doubled_path() {
        assert_eq!(material_path("/dev/grid"), "materials/dev/grid.kmat");
    }

    #[test]
    fn an_explicit_model_path_is_left_alone() {
        assert_eq!(
            model_path("models/props/crate.kmdl"),
            "models/props/crate.kmdl"
        );
    }
}
