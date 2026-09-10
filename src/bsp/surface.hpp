// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <string_view>

/// What a surface and a volume are made of.
///
/// A brush's behaviour comes from the material on its faces: `tools/clip` blocks
/// players, `tools/trigger` is not solid at all, `tools/nodraw` is solid but
/// never drawn. Source does the same thing, and the arrangement is worth
/// keeping because it means a designer changes what a brush *does* by changing
/// what it looks like in the editor, which is the same gesture.
///
/// The lookup lives here, in an engine library, rather than inside the
/// compiler -- so the editor showing a designer "blocks players only" and the
/// compiler deciding to emit a clip brush are reading the same table and cannot
/// disagree. A tooltip that lies about what a brush will compile to is worse
/// than no tooltip.
namespace kero::bsp {

/// What fills a volume. A brush has one; a leaf has the union of its brushes'.
enum class Contents : u32 {
    Empty = 0,
    Solid = 1u << 0,      ///< Blocks everything, and seals the level.
    Detail = 1u << 1,     ///< Solid, but kept out of the visibility tree.
    Window = 1u << 2,     ///< Solid, but see-through, so it does not seal.
    Water = 1u << 3,
    PlayerClip = 1u << 4, ///< Blocks players, not projectiles or NPCs.
    NpcClip = 1u << 5,
    Trigger = 1u << 6,    ///< Not solid; fires its entity's outputs on touch.
    Ladder = 1u << 7,
    /// Anything that stops a player walking through it.
    SolidMask = Solid | Detail | Window | PlayerClip,
};

[[nodiscard]] constexpr Contents operator|(Contents a, Contents b) {
    return static_cast<Contents>(static_cast<u32>(a) | static_cast<u32>(b));
}
[[nodiscard]] constexpr Contents operator&(Contents a, Contents b) {
    return static_cast<Contents>(static_cast<u32>(a) & static_cast<u32>(b));
}
constexpr Contents& operator|=(Contents& a, Contents b) { return a = a | b; }
[[nodiscard]] constexpr bool any(Contents value) { return static_cast<u32>(value) != 0; }

/// What a surface does when it is compiled and drawn.
enum class SurfaceFlags : u32 {
    None = 0,
    NoDraw = 1u << 0,   ///< Never rendered. Still collides unless also non-solid.
    Sky = 1u << 1,      ///< Drawn as the skybox, and lit as a light source.
    NoLight = 1u << 2,  ///< Not lit, and casts no shadow.
    Hint = 1u << 3,     ///< Not drawn; forces a BSP split on this plane.
    Skip = 1u << 4,     ///< Discarded entirely; the other sides of a hint brush.
    Trigger = 1u << 5,
    NoShadow = 1u << 6,
    /// Never drawn and never lit, whatever else is set.
    Invisible = NoDraw | NoLight,
};

[[nodiscard]] constexpr SurfaceFlags operator|(SurfaceFlags a, SurfaceFlags b) {
    return static_cast<SurfaceFlags>(static_cast<u32>(a) | static_cast<u32>(b));
}
[[nodiscard]] constexpr SurfaceFlags operator&(SurfaceFlags a, SurfaceFlags b) {
    return static_cast<SurfaceFlags>(static_cast<u32>(a) & static_cast<u32>(b));
}
constexpr SurfaceFlags& operator|=(SurfaceFlags& a, SurfaceFlags b) { return a = a | b; }
[[nodiscard]] constexpr bool any(SurfaceFlags value) { return static_cast<u32>(value) != 0; }

/// What a material means to the compiler.
struct SurfaceKind {
    Contents contents = Contents::Solid;
    SurfaceFlags flags = SurfaceFlags::None;

    /// Whether a brush of this kind stops the flood fill -- that is, whether it
    /// can form part of the shell that separates the level from the void.
    ///
    /// Deliberately narrower than "solid": a `func_detail` pillar is solid to a
    /// player but is not part of the visibility tree, so sealing a room with one
    /// would produce a level that leaks the moment detail is stripped. Making
    /// that a rule rather than a convention is the difference between a leak
    /// found at compile time and one found by a player.
    [[nodiscard]] bool seals() const {
        return any(contents & Contents::Solid) && !any(contents & Contents::Detail);
    }

    /// Whether a face with this material is emitted for the renderer.
    [[nodiscard]] bool visible() const { return !any(flags & SurfaceFlags::NoDraw); }
};

/// What `material` compiles to.
///
/// Anything under `tools/` is special; everything else is an ordinary solid,
/// drawn surface. Unknown tool materials are *not* silently ordinary -- a
/// misspelt `tools/playerclipp` that quietly became a visible solid wall is a
/// bug that takes an afternoon to find.
[[nodiscard]] SurfaceKind classify_material(std::string_view material);

/// Whether `material` names a tool material at all.
[[nodiscard]] bool is_tool_material(std::string_view material);

/// A one-line explanation of what a brush with this material will compile to,
/// for the editor to show when one is selected.
[[nodiscard]] std::string_view describe_material(std::string_view material);

}  // namespace kero::bsp
