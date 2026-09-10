// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "bsp/surface.hpp"

#include <array>

namespace kero::bsp {
namespace {

struct ToolMaterial {
    std::string_view name;
    Contents contents;
    SurfaceFlags flags;
    std::string_view description;
};

/// The whole table, in one place. Adding a tool material means adding a row
/// here and nothing else: the compiler, the engine and the editor all read it.
constexpr std::array kToolMaterials{
    ToolMaterial{"tools/nodraw", Contents::Solid, SurfaceFlags::NoDraw,
                 "solid, but never drawn"},
    ToolMaterial{"tools/skip", Contents::Empty, SurfaceFlags::Skip | SurfaceFlags::NoDraw,
                 "discarded; use it for the sides of a hint brush"},
    ToolMaterial{"tools/hint", Contents::Empty,
                 SurfaceFlags::Hint | SurfaceFlags::NoDraw,
                 "not solid and not drawn; forces the tree to split on this plane"},
    ToolMaterial{"tools/clip", Contents::PlayerClip | Contents::NpcClip,
                 SurfaceFlags::Invisible, "blocks players and NPCs; not drawn"},
    ToolMaterial{"tools/playerclip", Contents::PlayerClip, SurfaceFlags::Invisible,
                 "blocks players only; not drawn"},
    ToolMaterial{"tools/npcclip", Contents::NpcClip, SurfaceFlags::Invisible,
                 "blocks NPCs only; not drawn"},
    ToolMaterial{"tools/trigger", Contents::Trigger, SurfaceFlags::Invisible,
                 "not solid; touching it fires its entity's outputs"},
    ToolMaterial{"tools/skybox", Contents::Solid, SurfaceFlags::Sky | SurfaceFlags::NoLight,
                 "the sky; seals the level and lights it"},
    ToolMaterial{"tools/ladder", Contents::Ladder, SurfaceFlags::Invisible,
                 "climbable; not solid and not drawn"},
    ToolMaterial{"tools/blocklight", Contents::Empty, SurfaceFlags::Invisible,
                 "casts a shadow; nothing else"},
    ToolMaterial{"tools/water", Contents::Water, SurfaceFlags::None, "water"},
    ToolMaterial{"tools/grate", Contents::Window, SurfaceFlags::None,
                 "solid but see-through, so it does not seal the level"},
};

const ToolMaterial* find_tool(std::string_view material) {
    for (const ToolMaterial& entry : kToolMaterials) {
        if (entry.name == material) {
            return &entry;
        }
    }
    return nullptr;
}

}  // namespace

bool is_tool_material(std::string_view material) {
    return material.starts_with("tools/");
}

SurfaceKind classify_material(std::string_view material) {
    if (const ToolMaterial* tool = find_tool(material)) {
        return SurfaceKind{tool->contents, tool->flags};
    }
    if (is_tool_material(material)) {
        // A misspelt tool material must not quietly become a visible solid
        // wall. Cleave reports this; the kind returned here keeps it out of the
        // render and out of the seal so the mistake is loud rather than subtle.
        return SurfaceKind{Contents::Solid, SurfaceFlags::NoDraw};
    }
    return SurfaceKind{Contents::Solid, SurfaceFlags::None};
}

std::string_view describe_material(std::string_view material) {
    if (const ToolMaterial* tool = find_tool(material)) {
        return tool->description;
    }
    if (is_tool_material(material)) {
        return "unknown tool material; check the spelling";
    }
    return "solid and drawn";
}

}  // namespace kero::bsp
