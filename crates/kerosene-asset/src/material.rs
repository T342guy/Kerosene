// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `.kmat` -- material definitions, the VMT analogue.
//!
//! A material says which shader draws a surface and what to feed it. The
//! indirection matters: brush faces and models reference *materials*, never
//! textures, so retexturing a level or making every metal surface reflective
//! is one file change rather than a hunt through geometry.
//!
//! ```text
//! lit
//! {
//!     "$basetexture"    "dev/grid"
//!     "$bumpmap"        "dev/grid_normal"
//!     "$roughness"      "dev/grid_rough"
//!     "$selfillummask"  "dev/grid_emissive"
//!     "$ao"             "dev/grid_ao"
//!     "$metalnessmap"   "dev/grid_metal"
//!     "$metalness"      "1"
//!     "$roughnessfactor" "0.5"
//!     "$surfaceprop"    "concrete"
//! }
//! ```
//!
//! The block name is the shader. Parameters are `$`-prefixed by convention,
//! and unknown ones are preserved rather than dropped, so a game can add its
//! own without the engine needing to know about them.
//!
//! That text is the source. Alchemy compiles it to a `.kmat_c` -- the
//! resource container, kind `KMAT` -- and that is what the runtime loads:
//! the shader is resolved and the text parsed once, at build time, and the
//! textures it names are in the file's reference block for the packager.
//! [`Material::compile`] makes one; [`load`](Material::load) reads one.

use kerosene_kv::{FromKvValue, KeyValues, Vec3Value};
use kerosene_math::Vec3;
use kerosene_resource::bytes::{Reader, Writer};
use kerosene_resource::{EditInfo, ResourceError, ResourceFile, ResourceType, ResourceView, tag};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MaterialError {
    #[error(transparent)]
    Parse(#[from] kerosene_kv::ParseError),
    #[error("material file has no shader block")]
    NoShader,
}

impl ResourceType for Material {
    const KIND: [u8; 4] = *b"KMAT";
    const VERSION: u32 = 1;

    fn decode(file: &ResourceView<'_>) -> Result<Self, ResourceError> {
        let mut r = Reader::new(file.require(tag::DATA)?);
        let shader_name = r.str()?;
        let shader = Shader::from_name(shader_name)
            .ok_or_else(|| ResourceError::Invalid(format!("unknown shader {shader_name:?}")))?;
        let mut material = Material::new(shader);
        let count = r.u32()?;
        for _ in 0..count {
            let key = r.str()?;
            material.params.push(key, r.str()?);
        }
        Ok(material)
    }

    /// A material that is not in the container is `.kmat` source text,
    /// which [`WithMaterialSources`] hands over while projects migrate.
    fn decode_legacy(bytes: &[u8]) -> Option<Result<Self, ResourceError>> {
        let text = std::str::from_utf8(bytes).ok()?;
        Some(Material::parse(text).map_err(|e| ResourceError::Invalid(e.to_string())))
    }
}

/// A [`Source`](kerosene_resource::Source) that answers for a missing
/// `.kmat_c` with its `.kmat` source, once per file with a warning.
///
/// For the migration only: a project whose content has not been rebuilt
/// since materials began compiling has only the sources, and should keep
/// running until its next build. Nothing else at runtime reads a source
/// file, and when every project has been rebuilt this goes.
pub struct WithMaterialSources<'a>(pub &'a dyn kerosene_resource::Source);

impl kerosene_resource::Source for WithMaterialSources<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let not_compiled = match self.0.read(path) {
            Ok(bytes) => return Ok(bytes),
            Err(e) => e,
        };
        let Some(stem) = path.strip_suffix(crate::ext::MATERIAL_COMPILED) else {
            return Err(not_compiled);
        };
        let legacy = format!("{stem}{}", crate::ext::MATERIAL);
        let bytes = self.0.read(&legacy).map_err(|_| not_compiled)?;
        warn_uncompiled(&legacy);
        Ok(bytes)
    }
}

/// Say once per material that it was loaded from source.
fn warn_uncompiled(path: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if warned.get_or_insert_default().insert(path.to_string()) {
        log::warn!(
            "{path} has not been compiled; loading its source. Build the content (kiln) to fix"
        );
    }
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
    pub fn is_lit(self) -> bool {
        matches!(self, Shader::Lit | Shader::Water)
    }
}

/// A parsed material.
#[derive(Clone, Debug)]
pub struct Material {
    pub shader: Shader,
    /// Every parameter as written, so unknown keys survive a round trip.
    params: KeyValues,
}

impl Default for Material {
    fn default() -> Self {
        Material::new(Shader::Lit)
    }
}

impl Material {
    pub fn new(shader: Shader) -> Self {
        Material {
            shader,
            params: KeyValues::new(shader.name()),
        }
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
        Ok(Material {
            shader,
            params: block.clone(),
        })
    }

    pub fn to_text(&self) -> String {
        let mut block = self.params.clone();
        block.name = self.shader.name().to_string();
        block.to_text()
    }

    /// Compile `.kmat` source text into the bytes of a `.kmat_c`.
    ///
    /// `source_path` is recorded in the edit info, as whatever the caller
    /// calls the file; it changes nothing else.
    pub fn compile(text: &str, source_path: &str) -> Result<Vec<u8>, MaterialError> {
        let material = Material::parse(text)?;
        // Hashed without carriage returns, so a checkout that turned LF into
        // CRLF compiles to the same bytes: the text means the same thing.
        let normalized: Vec<u8> = text.bytes().filter(|&b| b != b'\r').collect();
        let mut file = material.to_resource().with_source(&normalized);
        file.set_edit_info(&EditInfo {
            compiler: "alchemy".into(),
            args: Vec::new(),
            inputs: vec![(
                source_path.to_string(),
                kerosene_resource::source_hash(&normalized),
            )],
        });
        Ok(file.to_bytes())
    }

    /// The material as a compiled resource, without edit info or a source
    /// hash: [`Material::compile`] adds those when there is a source.
    pub fn to_resource(&self) -> ResourceFile {
        let mut data = Writer::new();
        data.str(self.shader.name());
        let pairs: Vec<(&str, &str)> = self.params.pairs().collect();
        data.u32(pairs.len() as u32);
        for (k, v) in pairs {
            data.str(k).str(v);
        }
        let mut file = ResourceFile::new(Self::KIND, Self::VERSION);
        file.push(tag::DATA, data.finish());
        file.set_refs(
            self.referenced_textures()
                .into_iter()
                .map(crate::texture_path),
        );
        file
    }

    /// Load the material geometry calls `name`: `materials/<name>.kmat_c`,
    /// or its source while projects migrate (see [`WithMaterialSources`]).
    pub fn load(source: &dyn kerosene_resource::Source, name: &str) -> Result<Material, String> {
        use kerosene_resource::Source as _;
        let path = crate::material_path(name);
        let bytes = WithMaterialSources(source).read(&path)?;
        kerosene_resource::decode(&bytes).map_err(|e| format!("{path}: {e}"))
    }

    // ---- parameters ------------------------------------------------------

    pub fn get(&self, key: &str) -> Option<&str> {
        self.params.get(key)
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.params.set(key, value);
        self
    }

    pub fn params(&self) -> impl Iterator<Item = (&str, &str)> {
        self.params.pairs()
    }

    /// The main colour texture.
    pub fn base_texture(&self) -> Option<&str> {
        self.get("$basetexture")
    }

    /// Tangent-space normal map, if any.
    pub fn bump_map(&self) -> Option<&str> {
        self.get("$bumpmap")
    }

    /// Microfacet roughness map, read from the red channel.
    pub fn roughness_map(&self) -> Option<&str> {
        self.get("$roughness")
    }

    /// What the surface emits on its own, independent of any light reaching
    /// it.
    ///
    /// `$selfillummask` rather than `$emissive`: it is the key Source uses,
    /// and the one the packer already knew about.
    pub fn emissive_map(&self) -> Option<&str> {
        self.get("$selfillummask")
    }

    /// Baked ambient occlusion, darkening what the surface shadows itself.
    pub fn ao_map(&self) -> Option<&str> {
        self.get("$ao")
    }

    /// Where the surface is bare metal, read from the red channel.
    pub fn metalness_map(&self) -> Option<&str> {
        self.get("$metalnessmap")
    }

    /// How metallic the surface is, 0 to 1, defaulting to 0.
    ///
    /// A scalar and a map at once, the way `$color` tints `$basetexture`:
    /// with a map it scales the map, so a set whose map says "this is metal"
    /// can be toned down without re-baking it; without one it is the whole
    /// answer, so a plain steel material is one line rather than a texture.
    /// A set that has a map and says nothing reads as 1, because a map that
    /// exists and is then multiplied by zero would be a map that does nothing.
    pub fn metalness(&self) -> f32 {
        let default = if self.metalness_map().is_some() {
            1.0
        } else {
            0.0
        };
        self.get_f32("$metalness", default).clamp(0.0, 1.0)
    }

    /// A scale on roughness, 0 to 1, defaulting to 1.
    ///
    /// Multiplies the roughness map, whose absence reads as fully rough --
    /// so on its own it *is* the roughness, and polished chrome is
    /// `$metalness 1` and `$roughnessfactor 0.1` with no textures at all.
    /// glTF's `roughnessFactor`, under the `$` spelling every other key here
    /// uses. (`$roughness` names the map, so it could not be the scalar too.)
    pub fn roughness_factor(&self) -> f32 {
        self.get_f32("$roughnessfactor", 1.0).clamp(0.0, 1.0)
    }

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
                matches!(
                    *k,
                    "$basetexture"
                        | "$bumpmap"
                        | "$roughness"
                        | "$ao"
                        | "$metalnessmap"
                        | "$detail"
                        | "$selfillummask"
                        | "$envmapmask"
                        | "$blendmodulatetexture"
                        | "$basetexture2"
                        | "$bumpmap2"
                )
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

    /// Whether the surface is blended over what is behind it (`$translucent`):
    /// glass, water, smoke. Drawn after everything solid, back to front.
    pub fn is_blended(&self) -> bool {
        self.get_bool("$translucent")
    }

    /// Whether texels below [`alpha_test_reference`](Material::alpha_test_reference)
    /// are cut out (`$alphatest`): a fence, a grate, leaves. Solid where it
    /// is not cut, so it sorts and shadows like any wall.
    pub fn is_alpha_tested(&self) -> bool {
        self.get_bool("$alphatest")
    }

    /// The alpha below which an alpha-tested texel is cut out:
    /// `$alphatestreference`, 0.5 unless it says.
    pub fn alpha_test_reference(&self) -> f32 {
        self.get_f32("$alphatestreference", 0.5).clamp(0.0, 1.0)
    }

    /// Whether the surface should be drawn from both sides.
    pub fn is_two_sided(&self) -> bool {
        self.get_bool("$nocull")
    }

    /// Physical surface type, driving footstep sounds and impact effects.
    pub fn surface_property(&self) -> &str {
        self.get("$surfaceprop").unwrap_or("default")
    }

    /// The parsed surface type, for callers that want to branch on it rather
    /// than compare strings.
    pub fn surface_type(&self) -> SurfaceProperty {
        SurfaceProperty::parse(self.surface_property())
    }

    /// How much sound this surface soaks up, per band.
    ///
    /// `$acoustics` says so outright -- four numbers, or the name of a
    /// surface whose numbers to borrow -- and otherwise it follows from
    /// `$surfaceprop`, so a level made of ordinary materials sounds right
    /// without anyone having tuned a thing. The override exists for the
    /// texture that *looks* like concrete and is meant to be acoustic tile.
    pub fn acoustics(&self) -> AcousticProfile {
        match self.get("$acoustics").map(AcousticProfile::parse) {
            Some(Some(profile)) => profile,
            _ => AcousticProfile::of(&self.surface_type()),
        }
    }

    /// Uniform colour tint, defaulting to white.
    pub fn color_tint(&self) -> Vec3 {
        self.get("$color")
            .and_then(|v| Vec3Value::from_kv(v).ok())
            .map(|v| Vec3::from_array(v.to_array()))
            .unwrap_or(Vec3::ONE)
    }

    pub fn get_bool(&self, key: &str) -> bool {
        self.get(key)
            .and_then(|v| bool::from_kv(v).ok())
            .unwrap_or(false)
    }

    pub fn get_f32(&self, key: &str, default: f32) -> f32 {
        self.get(key)
            .and_then(|v| f32::from_kv(v).ok())
            .unwrap_or(default)
    }
}

/// The physical surface a material is made of, parsed from `$surfaceprop`.
///
/// This is what the format's `$surfaceprop` key was always for: it lets a game
/// say "that was a footstep on metal" or "that impact was on concrete" without
/// coupling the sound to the texture. It is deliberately a small, open set —
/// an unknown string is preserved as [`SurfaceProperty::Other`] rather than
/// lost, so a game can ship its own surface types without the engine knowing
/// them.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum SurfaceProperty {
    /// The default; ordinary, generic ground.
    #[default]
    Default,
    Concrete,
    Metal,
    Wood,
    Dirt,
    Grass,
    Glass,
    Water,
    Snow,
    Carpet,
    /// A named type the engine does not recognise, kept as its string
    /// (lower-cased), so a game's own `footstep/<name>/` sounds still resolve.
    Other(String),
}

impl SurfaceProperty {
    /// Parse the spelling a `.kmat` uses. Unknown names become
    /// [`SurfaceProperty::Other`] so they round-trip rather than being lost,
    /// which is the same treatment unknown material parameters get.
    pub fn parse(s: &str) -> SurfaceProperty {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "default" => SurfaceProperty::Default,
            "concrete" => SurfaceProperty::Concrete,
            "metal" => SurfaceProperty::Metal,
            "wood" => SurfaceProperty::Wood,
            "dirt" => SurfaceProperty::Dirt,
            "grass" => SurfaceProperty::Grass,
            "glass" => SurfaceProperty::Glass,
            "water" => SurfaceProperty::Water,
            "snow" => SurfaceProperty::Snow,
            "carpet" => SurfaceProperty::Carpet,
            other => SurfaceProperty::Other(other.to_string()),
        }
    }

    /// The canonical name, for logging and for re-serialising.
    pub fn as_str(&self) -> &str {
        match self {
            SurfaceProperty::Default => "default",
            SurfaceProperty::Concrete => "concrete",
            SurfaceProperty::Metal => "metal",
            SurfaceProperty::Wood => "wood",
            SurfaceProperty::Dirt => "dirt",
            SurfaceProperty::Grass => "grass",
            SurfaceProperty::Glass => "glass",
            SurfaceProperty::Water => "water",
            SurfaceProperty::Snow => "snow",
            SurfaceProperty::Carpet => "carpet",
            SurfaceProperty::Other(name) => name,
        }
    }

    /// The footstep sound a game plays for this surface, by the naming
    /// convention `footstep/<surface>/<step number>`.
    pub fn footstep_sound(&self, step: u8) -> String {
        format!("footstep/{}/{}", self.as_str(), step % 4 + 1)
    }
}

/// The bands acoustics are tabulated in, in hertz. The same four the mixer's
/// reverb decays independently; defined here too so the asset crate does not
/// have to know there is a mixer.
pub const ACOUSTIC_BANDS_HZ: [f32; 4] = [125.0, 500.0, 2000.0, 8000.0];

/// The most a surface may absorb. Nothing is a perfect sink, and a compiler
/// that took one literally would divide by the log of zero.
pub const MAX_ABSORPTION: f32 = 0.98;

/// How much of the sound striking a surface does not come back, per band,
/// 0 to [`MAX_ABSORPTION`].
///
/// Absorption coefficients in the sense an acoustician uses them -- the
/// figures are the published ones for the materials named, rounded to the
/// four bands. Every surface in a room contributes its own, and the
/// compiler's ray probe turns the lot into how long the room rings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AcousticProfile(pub [f32; 4]);

impl AcousticProfile {
    /// What `$surfaceprop` implies, absent an override.
    pub fn of(surface: &SurfaceProperty) -> AcousticProfile {
        AcousticProfile(match surface {
            SurfaceProperty::Default => [0.05, 0.06, 0.07, 0.08],
            SurfaceProperty::Concrete => [0.02, 0.03, 0.05, 0.07],
            SurfaceProperty::Metal => [0.05, 0.04, 0.03, 0.03],
            SurfaceProperty::Wood => [0.15, 0.10, 0.07, 0.07],
            SurfaceProperty::Glass => [0.18, 0.04, 0.03, 0.02],
            SurfaceProperty::Carpet => [0.02, 0.14, 0.60, 0.65],
            SurfaceProperty::Dirt => [0.15, 0.30, 0.40, 0.50],
            SurfaceProperty::Grass => [0.11, 0.26, 0.60, 0.69],
            SurfaceProperty::Snow => [0.45, 0.75, 0.90, 0.95],
            SurfaceProperty::Water => [0.01, 0.01, 0.02, 0.03],
            SurfaceProperty::Other(_) => [0.05, 0.06, 0.07, 0.08],
        })
    }

    /// Parse the value of `$acoustics`: four numbers per band, or the name
    /// of a surface property to borrow from. Anything else is `None`, and
    /// the caller falls back to `$surfaceprop` rather than guessing.
    pub fn parse(value: &str) -> Option<AcousticProfile> {
        let value = value.trim();
        let words: Vec<&str> = value.split_whitespace().collect();
        match words.as_slice() {
            [a, b, c, d] => {
                let mut bands = [0.0; 4];
                for (band, word) in bands.iter_mut().zip([a, b, c, d]) {
                    let n = word.parse::<f32>().ok()?;
                    *band = if n.is_finite() {
                        n.clamp(0.0, MAX_ABSORPTION)
                    } else {
                        0.0
                    };
                }
                Some(AcousticProfile(bands))
            }
            // A name is looked up. An unknown one is unknown rather than
            // `Other`, which would silently mean "default", and a lone
            // number is a mistake, not a name.
            [name] if name.parse::<f32>().is_err() => match SurfaceProperty::parse(name) {
                SurfaceProperty::Other(_) => None,
                known => Some(AcousticProfile::of(&known)),
            },
            _ => None,
        }
    }

    /// Absorption in one band, clamped to what the compiler can use.
    pub fn band(&self, band: usize) -> f32 {
        self.0[band.min(3)].clamp(0.0, MAX_ABSORPTION)
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
    fn blending_and_cutting_out_are_told_apart() {
        let glass = Material::parse(r#"lit { "$translucent" "1" "$nocull" "1" }"#).unwrap();
        let fence =
            Material::parse(r#"lit { "$alphatest" "1" "$alphatestreference" "0.3" }"#).unwrap();
        assert!(glass.is_blended() && !glass.is_alpha_tested() && glass.is_two_sided());
        assert!(fence.is_alpha_tested() && !fence.is_blended() && !fence.is_two_sided());
        assert_eq!(fence.alpha_test_reference(), 0.3);
        assert_eq!(glass.alpha_test_reference(), 0.5, "the default");
    }

    #[test]
    fn referenced_textures_finds_every_map() {
        let m = Material::parse(
            r#"lit { "$basetexture" "a" "$bumpmap" "b" "$detail" "c" "$surfaceprop" "metal" }"#,
        )
        .unwrap();
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
    fn acoustics_follow_the_surface_property() {
        let m = Material::parse(SAMPLE).unwrap();
        assert_eq!(
            m.acoustics(),
            AcousticProfile::of(&SurfaceProperty::Concrete)
        );
        let bare = Material::parse("lit { }").unwrap();
        assert_eq!(
            bare.acoustics(),
            AcousticProfile::of(&SurfaceProperty::Default)
        );
        let odd = Material::parse(r#"lit { "$surfaceprop" "cheese" }"#).unwrap();
        assert_eq!(
            odd.acoustics(),
            AcousticProfile::of(&SurfaceProperty::Default)
        );
    }

    #[test]
    fn acoustics_can_be_stated_outright() {
        let m = Material::parse(r#"lit { "$acoustics" "0.02 0.14 0.60 0.65" }"#).unwrap();
        assert_eq!(m.acoustics(), AcousticProfile([0.02, 0.14, 0.60, 0.65]));
        // Clamped, not trusted.
        let m = Material::parse(r#"lit { "$acoustics" "-1 2 nan 0.5" }"#).unwrap();
        assert_eq!(
            m.acoustics(),
            AcousticProfile([0.0, MAX_ABSORPTION, 0.0, 0.5])
        );
    }

    #[test]
    fn acoustics_can_borrow_another_surface() {
        let m =
            Material::parse(r#"lit { "$surfaceprop" "concrete" "$acoustics" "carpet" }"#).unwrap();
        assert_eq!(m.acoustics(), AcousticProfile::of(&SurfaceProperty::Carpet));
        assert_eq!(
            m.surface_type(),
            SurfaceProperty::Concrete,
            "the footsteps stay concrete"
        );
    }

    #[test]
    fn a_bad_acoustics_value_falls_back_to_the_surface() {
        for bad in ["0.1 0.2", "0.1 0.2 0.3 0.4 0.5", "velvet", "", "0.5"] {
            let text = format!(r#"lit {{ "$surfaceprop" "metal" "$acoustics" "{bad}" }}"#);
            let m = Material::parse(&text).unwrap();
            assert_eq!(
                m.acoustics(),
                AcousticProfile::of(&SurfaceProperty::Metal),
                "{bad:?} should have been ignored"
            );
        }
    }

    #[test]
    fn every_surface_absorbs_something_and_not_everything() {
        for s in [
            SurfaceProperty::Default,
            SurfaceProperty::Concrete,
            SurfaceProperty::Metal,
            SurfaceProperty::Wood,
            SurfaceProperty::Glass,
            SurfaceProperty::Carpet,
            SurfaceProperty::Dirt,
            SurfaceProperty::Grass,
            SurfaceProperty::Snow,
            SurfaceProperty::Water,
        ] {
            for b in 0..4 {
                let a = AcousticProfile::of(&s).band(b);
                assert!(a > 0.0 && a <= MAX_ABSORPTION, "{s:?} band {b} = {a}");
            }
        }
        // Soft things eat the highs; hard things barely eat anything.
        assert!(AcousticProfile::of(&SurfaceProperty::Carpet).band(3) > 0.5);
        assert!(AcousticProfile::of(&SurfaceProperty::Concrete).band(3) < 0.1);
    }

    #[test]
    fn tint_defaults_to_white() {
        assert_eq!(Material::parse("lit { }").unwrap().color_tint(), Vec3::ONE);
        let tinted = Material::parse(r#"lit { "$color" "[1 0.5 0.25]" }"#).unwrap();
        assert_eq!(tinted.color_tint(), Vec3::new(1.0, 0.5, 0.25));
    }

    #[test]
    fn surface_types_parse_case_insensitively() {
        assert_eq!(
            SurfaceProperty::parse("CONCRETE"),
            SurfaceProperty::Concrete
        );
        assert_eq!(SurfaceProperty::parse("Metal"), SurfaceProperty::Metal);
        assert_eq!(SurfaceProperty::parse(""), SurfaceProperty::Default);
        assert_eq!(SurfaceProperty::parse("default"), SurfaceProperty::Default);
    }

    #[test]
    fn an_unknown_surface_is_preserved_as_other() {
        let rubber = SurfaceProperty::parse("Rubber");
        assert_eq!(rubber, SurfaceProperty::Other("rubber".to_string()));
        assert_eq!(rubber.as_str(), "rubber");
        assert_eq!(rubber.footstep_sound(0), "footstep/rubber/1");
    }

    #[test]
    fn footstep_sounds_follow_the_convention() {
        assert_eq!(
            SurfaceProperty::Concrete.footstep_sound(0),
            "footstep/concrete/1"
        );
        assert_eq!(SurfaceProperty::Metal.footstep_sound(4), "footstep/metal/1");
        assert_eq!(SurfaceProperty::Metal.footstep_sound(2), "footstep/metal/3");
    }

    #[test]
    fn metalness_defaults_to_dielectric_without_a_map() {
        let m = Material::parse(SAMPLE).unwrap();
        assert_eq!(m.metalness(), 0.0);
        assert_eq!(m.metalness_map(), None);
    }

    #[test]
    fn a_metalness_map_on_its_own_means_fully_metal_where_it_says_so() {
        let m = Material::parse(r#"lit { "$metalnessmap" "x_metal" }"#).unwrap();
        assert_eq!(m.metalness_map(), Some("x_metal"));
        assert_eq!(m.metalness(), 1.0);
        assert!(m.referenced_textures().contains(&"x_metal"));
    }

    #[test]
    fn the_roughness_factor_defaults_to_fully_rough_and_is_clamped() {
        assert_eq!(Material::parse(SAMPLE).unwrap().roughness_factor(), 1.0);
        let m = Material::parse(r#"lit { "$roughnessfactor" "0.1" }"#).unwrap();
        assert_eq!(m.roughness_factor(), 0.1);
        let m = Material::parse(r#"lit { "$roughnessfactor" "-3" }"#).unwrap();
        assert_eq!(m.roughness_factor(), 0.0);
    }

    #[test]
    fn the_metalness_scalar_scales_and_is_clamped() {
        let m = Material::parse(r#"lit { "$metalness" "0.25" }"#).unwrap();
        assert_eq!(m.metalness(), 0.25);
        let m = Material::parse(r#"lit { "$metalness" "7" }"#).unwrap();
        assert_eq!(m.metalness(), 1.0);
    }

    const SOURCE: &str = r#"lit
{
    "$basetexture" "dev/grid"
    "$bumpmap" "dev/grid_normal"
    "$surfaceprop" "metal"
    "$mymodkey" "kept"
}
"#;

    #[test]
    fn a_compiled_material_reads_back_as_its_source() {
        let bytes = Material::compile(SOURCE, "materials/dev/grid.kmat").unwrap();
        let compiled: Material = kerosene_resource::decode(&bytes).unwrap();
        let source = Material::parse(SOURCE).unwrap();
        assert_eq!(compiled.shader, source.shader);
        assert_eq!(
            compiled.params().collect::<Vec<_>>(),
            source.params().collect::<Vec<_>>(),
            "every parameter, in order, unknown ones included"
        );
    }

    #[test]
    fn a_compiled_material_names_its_textures_and_its_source() {
        let bytes = Material::compile(SOURCE, "materials/dev/grid.kmat").unwrap();
        let view = ResourceView::parse(&bytes).unwrap();
        assert_eq!(view.kind(), *b"KMAT");
        assert_eq!(
            view.refs().unwrap(),
            ["materials/dev/grid.ktex", "materials/dev/grid_normal.ktex"]
        );
        let edit = view.edit_info().unwrap().unwrap();
        assert_eq!(edit.inputs[0].0, "materials/dev/grid.kmat");
        assert_eq!(edit.inputs[0].1, view.source_hash());
    }

    #[test]
    fn line_endings_do_not_change_the_compiled_bytes() {
        let crlf = SOURCE.replace('\n', "\r\n");
        assert_eq!(
            Material::compile(SOURCE, "a.kmat").unwrap(),
            Material::compile(&crlf, "a.kmat").unwrap()
        );
    }

    #[test]
    fn the_runtime_prefers_the_compiled_file_and_falls_back_to_source() {
        use std::collections::HashMap;
        let mut files: HashMap<String, Vec<u8>> = HashMap::new();
        files.insert(
            "materials/a.kmat".into(),
            br#"unlit { "$basetexture" "from_source" }"#.to_vec(),
        );
        assert_eq!(
            Material::load(&files, "a").unwrap().base_texture(),
            Some("from_source"),
            "a project not yet rebuilt still loads"
        );
        let compiled = Material::compile(r#"lit { "$basetexture" "compiled" }"#, "a").unwrap();
        files.insert("materials/a.kmat_c".into(), compiled);
        let m = Material::load(&files, "a").unwrap();
        assert_eq!(m.base_texture(), Some("compiled"));
        assert_eq!(m.shader, Shader::Lit);
        assert!(Material::load(&files, "missing").is_err());
    }

    #[test]
    fn an_unknown_shader_in_a_compiled_file_is_an_error() {
        let mut data = Writer::new();
        data.str("hologram").u32(0);
        let mut file = ResourceFile::new(Material::KIND, Material::VERSION);
        file.push(tag::DATA, data.finish());
        assert!(kerosene_resource::decode::<Material>(&file.to_bytes()).is_err());
    }
}
