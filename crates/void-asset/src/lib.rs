// SPDX-License-Identifier: LGPL-3.0-or-later
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
//! | Texture | `.voidtex`   | VTF                | Alchemy  |
//! | Material| `.voidmat`   | VMT                | Alchemy  |
//! | Model   | `.voidmdl`   | MDL                | Forge    |
//!
//! [`alchemy`]: https://github.com/t342guy/voidengine
//! [`forge`]: https://github.com/t342guy/voidengine

pub mod material;
pub mod model;
pub mod texture;

pub use material::{Material, MaterialError, Shader};
pub use model::{Bone, Mesh, Model, ModelError, Vertex};
pub use texture::{Mip, PixelFormat, Texture, TextureError, TextureFlags};

/// Canonical extensions, so tools and the VFS agree on them in one place.
pub mod ext {
    pub const TEXTURE: &str = "voidtex";
    pub const MATERIAL: &str = "voidmat";
    pub const MODEL: &str = "voidmdl";
    pub const MAP_SOURCE: &str = "voidmap";
    pub const MAP_COMPILED: &str = "voidbsp";
    pub const ARCHIVE: &str = "vault";
}

/// Where a material lives, given the name geometry refers to it by.
///
/// Brush faces store `dev/grid`; the file is `materials/dev/grid.voidmat`. The
/// prefix and extension are added here rather than being written into every
/// map, so content can be reorganised without rewriting geometry.
pub fn material_path(name: &str) -> String {
    format!("materials/{}.{}", name.trim_start_matches('/'), ext::MATERIAL)
}

/// Where a texture lives, given the name a material refers to it by.
pub fn texture_path(name: &str) -> String {
    format!("materials/{}.{}", name.trim_start_matches('/'), ext::TEXTURE)
}

/// Where a model lives, given the name an entity refers to it by.
pub fn model_path(name: &str) -> String {
    let name = name.trim_start_matches('/');
    if name.ends_with(ext::MODEL) { name.to_string() } else { format!("models/{name}.{}", ext::MODEL) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_resolve_to_paths() {
        assert_eq!(material_path("dev/grid"), "materials/dev/grid.voidmat");
        assert_eq!(texture_path("dev/grid"), "materials/dev/grid.voidtex");
        assert_eq!(model_path("props/crate"), "models/props/crate.voidmdl");
    }

    #[test]
    fn a_leading_slash_does_not_produce_a_doubled_path() {
        assert_eq!(material_path("/dev/grid"), "materials/dev/grid.voidmat");
    }

    #[test]
    fn an_explicit_model_path_is_left_alone() {
        assert_eq!(model_path("models/props/crate.voidmdl"), "models/props/crate.voidmdl");
    }
}
