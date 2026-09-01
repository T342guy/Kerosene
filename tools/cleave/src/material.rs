// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Tool materials: how a material name decides what a brush *means*.
//!
//! Source encodes compile-time intent in the texture applied to a face. A
//! brush textured `tools/toolsclip` blocks players but never renders; one
//! textured `tools/toolshint` exists only to force a BSP split. Kerosene
//! keeps the convention, with shorter names, because it is genuinely good
//! design: the level designer expresses intent with the same tool they use for
//! everything else, and it is visible in the 3D view.
//!
//! Everything under `tools/` is a tool material. Everything else is ordinary
//! world geometry that draws and blocks.

use kerosene_bsp::{contents, surf};

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
        // Not solid: you walk into a ladder, and then you climb it.
        "ladder" => MaterialFlags::new(contents::LADDER, surf::NODRAW, false),
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

/// What a material does, in a sentence.
///
/// Next to the table that decides it, so the editor's explanation and the
/// compiler's behaviour cannot drift apart. A tool texture whose effect you
/// have to compile the map to discover is a tool texture that appears to do
/// nothing -- which is exactly how they were reported.
pub fn describe(material: &str) -> &'static str {
    let lower = material.to_lowercase();
    let Some(tool) = lower.strip_prefix("tools/") else {
        return "world geometry: draws, blocks everything";
    };
    if !is_known_tool(material) {
        return "unknown tools/ material -- compiles as ordinary world geometry";
    }
    match tool {
        "nodraw" | "invisible" => "solid, and never drawn",
        "clip" | "playerclip" => "blocks players only; bullets and sight pass through",
        "npcclip" | "monsterclip" => "blocks NPCs only",
        "trigger" => "not solid; touching it fires its entity's outputs",
        "skybox" | "sky" => "draws the sky, and lets the sun's light in",
        "ladder" => "not solid; standing in it lets you climb",
        "hint" => "forces a BSP split along this face, then vanishes",
        "skip" => "does nothing at all -- the other faces of a hint brush",
        "blocklight" => "casts a shadow without being solid",
        "grate" => "blocks movement and bullets; you can see through it",
        "water" => "water: swimmable, translucent, unlit",
        _ => "world geometry: draws, blocks everything",
    }
}

/// Contents, in words.
///
/// The bits mean something specific and the names are not guessable from
/// them, so an editor showing `0x10001` is showing nothing.
pub fn contents_words(contents: u32) -> String {
    let mut what = Vec::new();
    for (bit, name) in [
        (contents::TRIGGER, "a trigger volume"),
        (contents::WATER, "water"),
        (contents::GRATE, "a grate"),
        (contents::PLAYER_CLIP, "blocks players"),
        (contents::MONSTER_CLIP, "blocks NPCs"),
        (contents::OPAQUE, "blocks light"),
        (contents::SOLID, "solid"),
        (contents::DETAIL, "detail: it does not seal the map or cut visibility"),
        (contents::MOVEABLE, "moves"),
        (contents::TRANSLUCENT, "see-through"),
    ] {
        if contents & bit != 0 { what.push(name) }
    }
    if what.is_empty() { return "empty -- it compiles to nothing".to_string() }
    what.join(", ")
}

/// What a brush made of these materials compiles as.
///
/// The same rule the compiler uses, so the editor can say what a brush *is*
/// before anyone runs a compile to find out. Two parts of that rule surprise
/// people, and both are worth saying out loud:
///
/// * A single tool face changes the whole brush. `SOLID` is the absence of
///   anything more specific, so one `tools/clip` face on an otherwise ordinary
///   box makes the box a clip brush -- it stops being a wall.
/// * A class can overrule the faces entirely. A `trigger_multiple` is a
///   trigger whatever it is textured with.
pub fn describe_brush(materials: &[String], classname: Option<&str>) -> String {
    let by_class = classname.and_then(contents_for_classname);
    let contents = by_class.unwrap_or_else(|| {
        let face_contents: Vec<u32> = materials.iter().map(|m| flags_for(m).contents).collect();
        crate::brush::resolve_contents(&face_contents)
    });

    let words = contents_words(contents);
    match classname {
        Some(class) if by_class.is_some() => {
            format!("{words} -- decided by {class}, whatever its faces wear")
        }
        _ => words,
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
            | "ladder"
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
        "func_ladder" => Some(contents::LADDER),
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

    #[test]
    fn every_known_tool_material_says_what_it_does() {
        // A tool texture whose effect you have to compile the map to discover
        // is a tool texture that appears to do nothing.
        for tool in [
            "nodraw", "invisible", "clip", "playerclip", "npcclip", "monsterclip",
            "trigger", "skybox", "sky", "hint", "skip", "blocklight", "grate", "water",
        ] {
            let name = format!("tools/{tool}");
            let said = describe(&name);
            assert!(!said.is_empty(), "{name}");
            assert_ne!(
                said, "world geometry: draws, blocks everything",
                "{name} is not world geometry and should not say so"
            );
        }
    }

    #[test]
    fn an_ordinary_material_is_described_as_world_geometry() {
        assert_eq!(describe("dev/grid"), "world geometry: draws, blocks everything");
    }

    #[test]
    fn a_misspelt_tool_material_says_it_is_one() {
        // The failure mode this catches is a doorway silently walled off by a
        // typo, so the description has to name the problem rather than
        // quietly agreeing it is a wall.
        let said = describe("tools/clipp");
        assert!(said.contains("unknown"), "{said}");
    }

    #[test]
    fn a_brush_is_described_by_what_it_will_compile_as() {
        assert_eq!(describe_brush(&["dev/grid".into()], None), "solid");
        assert_eq!(describe_brush(&["tools/clip".into()], None), "blocks players");
        assert_eq!(describe_brush(&["tools/trigger".into()], None), "a trigger volume");
    }

    #[test]
    fn one_tool_face_changes_what_the_whole_brush_is() {
        // Solid is the absence of anything more specific, so a single
        // `tools/clip` face on an otherwise ordinary box stops the box being
        // a wall. It is the compiler's rule and it surprises everybody, which
        // is precisely why the editor has to say so.
        let mixed = vec!["tools/clip".into(), "dev/grid".into(), "dev/grid".into()];
        assert_eq!(describe_brush(&mixed, None), "blocks players");
        assert_ne!(describe_brush(&mixed, None), describe_brush(&["dev/grid".into()], None));
    }

    #[test]
    fn a_class_that_decides_its_own_contents_says_so() {
        // A trigger_multiple is a trigger whatever its faces wear, and
        // someone wondering why their world-textured brush is not solid
        // deserves to be told here rather than after a compile.
        let said = describe_brush(&["dev/grid".into()], Some("trigger_multiple"));
        assert!(said.starts_with("a trigger volume"), "{said}");
        assert!(said.contains("trigger_multiple"), "{said}");
    }

    #[test]
    fn a_door_is_described_as_something_that_moves() {
        // Which is why it is its own model, and why the renderer has to draw
        // it separately from the world.
        let said = describe_brush(&["dev/door".into()], Some("func_door"));
        assert!(said.contains("solid"), "{said}");
        assert!(said.contains("moves"), "{said}");
    }

    #[test]
    fn a_class_the_compiler_has_no_opinion_about_falls_back_to_the_faces() {
        assert_eq!(describe_brush(&["dev/grid".into()], Some("info_target")), "solid");
    }

    #[test]
    fn contents_of_nothing_says_so_rather_than_showing_a_zero() {
        assert!(contents_words(0).contains("nothing"));
    }

    #[test]
    fn a_ladder_is_not_solid_and_is_never_drawn() {
        // Both halves matter. A solid ladder is a wall you cannot climb, and
        // a drawn one puts a grey rectangle over whatever art is behind it.
        let f = flags_for("tools/ladder");
        assert_eq!(f.contents & contents::SOLID, 0, "a ladder must not block the player");
        assert!(f.contents & contents::LADDER != 0);
        assert!(f.surface & surf::NODRAW != 0);
        assert!(!f.emits_face);
    }

    #[test]
    fn a_func_ladder_is_a_ladder_whatever_its_faces_are_textured_with() {
        // The same override triggers get: a designer should not have to
        // texture every face of a ladder by hand to make the class work.
        assert_eq!(contents_for_classname("func_ladder"), Some(contents::LADDER));
        let described = describe_brush(&["dev/grid".to_string()], Some("func_ladder"));
        assert!(described.to_lowercase().contains("ladder"), "{described}");
    }
}
