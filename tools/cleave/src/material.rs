//! Tool materials: how a material name decides what a brush *means*.
//!
//! Source encodes compile-time intent in the texture applied to a face. A
//! brush textured `tools/toolsclip` blocks players but never renders; one
//! textured `tools/toolshint` exists only to force a BSP split. VoidEngine
//! keeps the convention, with shorter names, because it is genuinely good
//! design: the level designer expresses intent with the same tool they use for
//! everything else, and it is visible in the 3D view.
//!
//! Everything under `tools/` is a tool material. Everything else is ordinary
//! world geometry that draws and blocks.

use void_bsp::{contents, surf};

/// What a material means to the compiler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialFlags {
    /// Contents this material contributes to its brush.
    pub contents: u32,
    /// Surface flags for faces wearing it.
    pub surface: u32,
    /// Whether faces with this material survive into the face lump at all.
    pub emits_face: bool,
}

impl MaterialFlags {
    const fn new(contents: u32, surface: u32, emits_face: bool) -> Self {
        MaterialFlags { contents, surface, emits_face }
    }
}

/// Ordinary world geometry: draws, blocks everything.
pub const WORLD: MaterialFlags = MaterialFlags::new(contents::SOLID, 0, true);

/// Look up what a material name means.
///
/// Matching is on the path under `tools/`, case-insensitively, so
/// `TOOLS/Clip` and `tools/clip` agree.
pub fn flags_for(material: &str) -> MaterialFlags {
    let lower = material.to_lowercase();
    let Some(tool) = lower.strip_prefix("tools/") else { return WORLD };

    match tool {
        // Solid, but never drawn. The workhorse: the outside faces of a
        // sealed room, where geometry must block but is never seen.
        "nodraw" | "invisible" => MaterialFlags::new(contents::SOLID, surf::NODRAW, false),

        // Blocks players, invisible, does not block bullets or sight.
        "clip" | "playerclip" => {
            MaterialFlags::new(contents::PLAYER_CLIP, surf::NODRAW, false)
        }
        "npcclip" | "monsterclip" => {
            MaterialFlags::new(contents::MONSTER_CLIP, surf::NODRAW, false)
        }

        // A trigger volume: not solid, but traces find it and the engine
        // fires its entity's outputs on touch.
        "trigger" => MaterialFlags::new(
            contents::TRIGGER,
            surf::NODRAW | surf::TRIGGER,
            false,
        ),

        // The skybox. Draws (as the sky), and during the lighting compile it
        // is where sunlight enters the world.
        "skybox" | "sky" => MaterialFlags::new(
            contents::SOLID,
            surf::SKY | surf::NOLIGHT,
            true,
        ),

        // Hint forces a BSP split along its plane and then vanishes; skip is
        // what the hint brush's other five faces wear so they do nothing.
        // Together they are how a designer hand-tunes visibility.
        "hint" => MaterialFlags::new(contents::EMPTY, surf::HINT | surf::NODRAW, false),
        "skip" => MaterialFlags::new(contents::EMPTY, surf::SKIP | surf::NODRAW, false),

        // Blocks light during the lighting compile without being solid --
        // for casting a shadow from geometry that is not really there.
        "blocklight" => MaterialFlags::new(contents::OPAQUE, surf::NODRAW, false),

        // Blocks movement and bullets, but you can see through it.
        "grate" => MaterialFlags::new(contents::GRATE, 0, true),

        "water" => MaterialFlags::new(
            contents::WATER | contents::TRANSLUCENT,
            surf::WARP | surf::TRANS | surf::NOLIGHT,
            true,
        ),

        // An unrecognised tools/ material is a typo, and treating it as world
        // geometry would silently wall off a doorway. Caller warns.
        _ => WORLD,
    }
}

/// Whether a `tools/` name is one we actually know.
pub fn is_known_tool(material: &str) -> bool {
    let lower = material.to_lowercase();
    let Some(tool) = lower.strip_prefix("tools/") else { return true };
    matches!(
        tool,
        "nodraw" | "invisible" | "clip" | "playerclip" | "npcclip" | "monsterclip"
            | "trigger" | "skybox" | "sky" | "hint" | "skip" | "blocklight" | "grate" | "water"
    )
}

/// Contents implied by an entity's classname, overriding its brushes'.
///
/// A `trigger_multiple` is a trigger whatever its faces are textured with; a
/// `func_detail` is detail geometry. This is the same override Source applies,
/// and it is why a designer does not have to texture every trigger by hand.
pub fn contents_for_classname(classname: &str) -> Option<u32> {
    let lower = classname.to_lowercase();
    if lower.starts_with("trigger_") { return Some(contents::TRIGGER); }
    match lower.as_str() {
        "func_detail" => Some(contents::SOLID | contents::DETAIL),
        "func_water" | "func_liquid" => Some(contents::WATER | contents::TRANSLUCENT),
        "func_illusionary" => Some(contents::EMPTY),
        // A door or platform moves, so traces have to know it is not the world.
        "func_door" | "func_door_rotating" | "func_movelinear" | "func_platform"
        | "func_rotating" | "func_tracktrain" | "func_brush" => {
            Some(contents::SOLID | contents::MOVEABLE)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_materials_are_solid_and_visible() {
        let f = flags_for("dev/grid");
        assert_eq!(f.contents, contents::SOLID);
        assert!(f.emits_face);
        assert_eq!(f.surface, 0);
    }

    #[test]
    fn nodraw_is_solid_but_invisible() {
        let f = flags_for("tools/nodraw");
        assert_eq!(f.contents, contents::SOLID);
        assert!(!f.emits_face, "nodraw must not reach the face lump");
        assert!(f.surface & surf::NODRAW != 0);
    }

    #[test]
    fn clip_blocks_players_but_not_bullets() {
        let f = flags_for("tools/clip");
        assert!(f.contents & contents::PLAYER_CLIP != 0);
        assert!(f.contents & contents::SOLID == 0, "a clip brush is not world solid");
        assert!(contents::MASK_SHOT & f.contents == 0, "bullets pass through clips");
        assert!(contents::MASK_PLAYER_SOLID & f.contents != 0);
    }

    #[test]
    fn sky_draws_but_takes_no_lightmap() {
        let f = flags_for("tools/skybox");
        assert!(f.emits_face, "the sky has to be drawn");
        assert!(f.surface & surf::SKY != 0);
        assert!(f.surface & surf::NOLIGHT != 0);
    }

    #[test]
    fn hint_and_skip_are_not_solid() {
        for name in ["tools/hint", "tools/skip"] {
            let f = flags_for(name);
            assert_eq!(f.contents, contents::EMPTY, "{name} must not block anything");
            assert!(!f.emits_face);
        }
        assert!(flags_for("tools/hint").surface & surf::HINT != 0);
        assert!(flags_for("tools/skip").surface & surf::SKIP != 0);
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(flags_for("TOOLS/Clip"), flags_for("tools/clip"));
    }

    #[test]
    fn a_material_merely_named_like_a_tool_is_not_one() {
        // `dev/toolsclip` is a real texture in a folder called dev.
        assert_eq!(flags_for("dev/toolsclip"), WORLD);
        assert!(is_known_tool("dev/anything"));
    }

    #[test]
    fn unknown_tool_materials_are_flagged() {
        assert!(!is_known_tool("tools/clipp"));
        assert_eq!(flags_for("tools/clipp"), WORLD, "unknown tools fall back to solid");
    }

    #[test]
    fn classnames_override_brush_contents() {
        assert_eq!(
            contents_for_classname("func_detail"),
            Some(contents::SOLID | contents::DETAIL)
        );
        assert_eq!(contents_for_classname("trigger_multiple"), Some(contents::TRIGGER));
        assert_eq!(contents_for_classname("trigger_hurt"), Some(contents::TRIGGER));
        assert!(contents_for_classname("func_door").unwrap() & contents::MOVEABLE != 0);
        assert_eq!(contents_for_classname("info_player_start"), None);
    }
}
