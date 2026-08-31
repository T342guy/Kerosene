// SPDX-License-Identifier: MPL-2.0
//! `.keromat` -- material definitions, the VMT analogue.
//!
//! A material says which shader draws a surface and what to feed it. The
//! indirection matters: brush faces and models reference *materials*, never
//! textures, so retexturing a level or making every metal surface reflective
//! is one file change rather than a hunt through geometry.
//!
//! ```text
//! lit
//! {
//!     "$basetexture"  "dev/grid"
//!     "$bumpmap"      "dev/grid_normal"
//!     "$surfaceprop"  "concrete"
//! }
//! ```
//!
//! The block name is the shader. Parameters are `$`-prefixed by convention,
//! and unknown ones are preserved rather than dropped, so a game can add its
//! own without the engine needing to know about them.

use thiserror::Error;
use kerosene_kv::{FromKvValue, KeyValues, Vec3Value};
use kerosene_math::Vec3;

#[derive(Debug, Error)]
pub enum MaterialError {
    #[error(transparent)]
    Parse(#[from] kerosene_kv::ParseError),
    #[error("material file has no shader block")]
    NoShader,
}

/// Which shader draws a surface.
///
/// A small closed set: every one is a real code path in the renderer, so an
/// open-ended string would just be a way to fail at draw time instead of load
/// time.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shader {
    /// The workhorse: lightmapped, optionally bump mapped.
    #[default]
    Lit,
    /// Ignores lighting entirely. For tool textures and effects.
    Unlit,
    /// The skybox.
    Sky,
    /// Scrolling, refracting surface.
    Water,
    /// Interface art, drawn in screen space.
    Ui,
}

impl Shader {
    pub fn from_name(name: &str) -> Option<Shader> {
        match name.to_lowercase().as_str() {
            "lit" | "lightmapped" => Some(Shader::Lit),
            "unlit" => Some(Shader::Unlit),
            "sky" | "skybox" => Some(Shader::Sky),
            "water" => Some(Shader::Water),
            "ui" => Some(Shader::Ui),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Shader::Lit => "lit",
            Shader::Unlit => "unlit",
            Shader::Sky => "sky",
            Shader::Water => "water",
            Shader::Ui => "ui",
        }
    }

    /// Whether surfaces with this shader receive baked lighting.
    pub fn is_lit(self) -> bool { matches!(self, Shader::Lit | Shader::Water) }
}

/// A parsed material.
#[derive(Clone, Debug)]
pub struct Material {
    pub shader: Shader,
    /// Every parameter as written, so unknown keys survive a round trip.
    params: KeyValues,
}

impl Default for Material {
    fn default() -> Self { Material::new(Shader::Lit) }
}

impl Material {
    pub fn new(shader: Shader) -> Self {
        Material { shader, params: KeyValues::new(shader.name()) }
    }

    pub fn parse(text: &str) -> Result<Material, MaterialError> {
        let root = KeyValues::parse(text)?;
        let block = root.all_blocks().next().ok_or(MaterialError::NoShader)?;
        // An unrecognised shader name falls back to `lit` rather than failing:
        // a material typo should make a surface look wrong, not make the map
        // refuse to load.
        let shader = Shader::from_name(&block.name).unwrap_or_else(|| {
            log::warn!("unknown shader '{}', falling back to lit", block.name);
            Shader::Lit
        });
        Ok(Material { shader, params: block.clone() })
    }

    pub fn to_text(&self) -> String {
        let mut block = self.params.clone();
        block.name = self.shader.name().to_string();
        block.to_text()
    }

    // ---- parameters ------------------------------------------------------

    pub fn get(&self, key: &str) -> Option<&str> { self.params.get(key) }

    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.params.set(key, value);
        self
    }

    pub fn params(&self) -> impl Iterator<Item = (&str, &str)> { self.params.pairs() }

    /// The main colour texture.
    pub fn base_texture(&self) -> Option<&str> { self.get("$basetexture") }

    /// Tangent-space normal map, if any.
    pub fn bump_map(&self) -> Option<&str> { self.get("$bumpmap") }

    /// Every texture this material references, for content packing.
    ///
    /// Vault uses this to work out what a map actually needs: walking the
    /// materials is the only way to know, since geometry never names a
    /// texture directly.
    pub fn referenced_textures(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .params
            .pairs()
            .filter(|(k, _)| {
                // Any parameter naming a texture uses one of these keys.
                matches!(*k, "$basetexture" | "$bumpmap" | "$detail" | "$selfillummask"
                    | "$envmapmask" | "$blendmodulatetexture" | "$basetexture2" | "$bumpmap2")
            })
            .map(|(_, v)| v)
            .filter(|v| !v.is_empty())
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn is_translucent(&self) -> bool {
        self.get_bool("$translucent") || self.get_bool("$alphatest")
    }

    /// Whether the surface should be drawn from both sides.
    pub fn is_two_sided(&self) -> bool { self.get_bool("$nocull") }

    /// Physical surface type, driving footstep sounds and impact effects.
    pub fn surface_property(&self) -> &str {
        self.get("$surfaceprop").unwrap_or("default")
    }

    /// Uniform colour tint, defaulting to white.
    pub fn color_tint(&self) -> Vec3 {
        self.get("$color")
            .and_then(|v| Vec3Value::from_kv(v).ok())
            .map(|v| Vec3::from_array(v.to_array()))
            .unwrap_or(Vec3::ONE)
    }

    pub fn get_bool(&self, key: &str) -> bool {
        self.get(key).and_then(|v| bool::from_kv(v).ok()).unwrap_or(false)
    }

    pub fn get_f32(&self, key: &str, default: f32) -> f32 {
        self.get(key).and_then(|v| f32::from_kv(v).ok()).unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
lit
{
    "$basetexture"  "dev/grid"
    "$bumpmap"      "dev/grid_normal"
    "$surfaceprop"  "concrete"
    "$translucent"  "0"
    "$mymod_custom" "keep me"
}
"#;

    #[test]
    fn parses_shader_and_parameters() {
        let m = Material::parse(SAMPLE).unwrap();
        assert_eq!(m.shader, Shader::Lit);
        assert_eq!(m.base_texture(), Some("dev/grid"));
        assert_eq!(m.bump_map(), Some("dev/grid_normal"));
        assert_eq!(m.surface_property(), "concrete");
        assert!(!m.is_translucent());
    }

    #[test]
    fn unknown_parameters_survive_a_round_trip() {
        // A game will invent parameters the engine has never heard of.
        let m = Material::parse(SAMPLE).unwrap();
        let text = m.to_text();
        assert!(text.contains("$mymod_custom"), "{text}");
        let back = Material::parse(&text).unwrap();
        assert_eq!(back.get("$mymod_custom"), Some("keep me"));
    }

    #[test]
    fn an_unknown_shader_falls_back_rather_than_failing() {
        let m = Material::parse(r#"SomeFutureShader { "$basetexture" "x" }"#).unwrap();
        assert_eq!(m.shader, Shader::Lit);
        assert_eq!(m.base_texture(), Some("x"));
    }

    #[test]
    fn shader_names_are_case_insensitive() {
        assert_eq!(Shader::from_name("LIT"), Some(Shader::Lit));
        assert_eq!(Shader::from_name("SkyBox"), Some(Shader::Sky));
        assert_eq!(Shader::from_name("nonsense"), None);
    }

    #[test]
    fn only_lit_shaders_take_lightmaps() {
        assert!(Shader::Lit.is_lit());
        assert!(!Shader::Unlit.is_lit());
        assert!(!Shader::Sky.is_lit(), "the sky is its own light source");
        assert!(!Shader::Ui.is_lit());
    }

    #[test]
    fn referenced_textures_finds_every_map() {
        let m = Material::parse(
            r#"lit { "$basetexture" "a" "$bumpmap" "b" "$detail" "c" "$surfaceprop" "metal" }"#,
        ).unwrap();
        assert_eq!(m.referenced_textures(), vec!["a", "b", "c"]);
    }

    #[test]
    fn referenced_textures_does_not_pick_up_non_texture_keys() {
        // "$surfaceprop" "concrete" must not be mistaken for a texture path.
        let m = Material::parse(SAMPLE).unwrap();
        let textures = m.referenced_textures();
        assert!(!textures.contains(&"concrete"));
        assert_eq!(textures, vec!["dev/grid", "dev/grid_normal"]);
    }

    #[test]
    fn translucency_is_either_flag() {
        let a = Material::parse(r#"lit { "$translucent" "1" }"#).unwrap();
        let b = Material::parse(r#"lit { "$alphatest" "1" }"#).unwrap();
        let c = Material::parse(r#"lit { }"#).unwrap();
        assert!(a.is_translucent() && b.is_translucent() && !c.is_translucent());
    }

    #[test]
    fn a_material_can_be_built_and_written() {
        let mut m = Material::new(Shader::Unlit);
        m.set("$basetexture", "tools/nodraw");
        let text = m.to_text();
        assert!(text.starts_with("unlit"), "{text}");
        let back = Material::parse(&text).unwrap();
        assert_eq!(back.shader, Shader::Unlit);
        assert_eq!(back.base_texture(), Some("tools/nodraw"));
    }

    #[test]
    fn an_empty_file_is_an_error() {
        assert!(matches!(Material::parse(""), Err(MaterialError::NoShader)));
    }

    #[test]
    fn tint_defaults_to_white() {
        assert_eq!(Material::parse("lit { }").unwrap().color_tint(), Vec3::ONE);
        let tinted = Material::parse(r#"lit { "$color" "[1 0.5 0.25]" }"#).unwrap();
        assert_eq!(tinted.color_tint(), Vec3::new(1.0, 0.5, 0.25));
    }
}
