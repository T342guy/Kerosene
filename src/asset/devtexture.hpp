// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <span>
#include <string_view>
#include <vector>

/// Assets, and for now the one that stands in for all of them.
///
/// Alchemy will eventually compile `.ktex` from source art and this library
/// will load it. Until then a material has no pixels, and every surface would
/// be flat grey -- which makes a level impossible to read and impossible to
/// judge the scale of.
namespace kero::asset {

/// The side length of a developer texture, in pixels.
inline constexpr u32 kDevTextureSize = 64;

/// A procedural stand-in for a material that has not been compiled.
///
/// A checkerboard with grid lines, tinted by a hash of the material name, so
/// every surface is visibly a *particular* material and the grid gives a sense
/// of scale without a tape measure. Deliberately unsubtle: nobody should mistake
/// it for the finished look, and a level that reads clearly in developer
/// textures reads clearly in any textures.
///
/// It lives here rather than inside the renderer because the editor needs the
/// same pixels -- a material that looked one way in Chisel and another in the
/// engine would be worse than no preview at all.
///
/// Returns `kDevTextureSize * kDevTextureSize` RGBA pixels.
[[nodiscard]] std::vector<u8> dev_texture(std::string_view material);

/// The tint alone, for a swatch or a wireframe colour.
struct Colour {
    u8 r = 0;
    u8 g = 0;
    u8 b = 0;
};

/// The colour a material hashes to.
///
/// Stable across sessions and machines: `std::hash` is neither, and a material
/// that changed colour between runs would make a level unrecognisable from one
/// day to the next.
[[nodiscard]] Colour dev_colour(std::string_view material);

}  // namespace kero::asset
