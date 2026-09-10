// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "asset/devtexture.hpp"

namespace kero::asset {
namespace {

/// FNV-1a. Chosen for being short, well-mixed enough for this, and -- unlike
/// std::hash -- specified, so the same name gives the same colour on every
/// machine and in every build.
u32 hash_of(std::string_view text) {
    u32 hash = 2166136261u;
    for (char c : text) {
        hash = (hash ^ static_cast<u8>(c)) * 16777619u;
    }
    return hash;
}

}  // namespace

Colour dev_colour(std::string_view material) {
    const u32 hash = hash_of(material);
    // The low bit of each channel's range is dropped so nothing comes out too
    // dark to read a grid line against.
    return Colour{static_cast<u8>(96 + (hash & 0x7Fu)),
                  static_cast<u8>(96 + ((hash >> 8) & 0x7Fu)),
                  static_cast<u8>(96 + ((hash >> 16) & 0x7Fu))};
}

std::vector<u8> dev_texture(std::string_view material) {
    const Colour base = dev_colour(material);

    std::vector<u8> pixels(static_cast<usize>(kDevTextureSize) * kDevTextureSize * 4);
    for (u32 y = 0; y < kDevTextureSize; ++y) {
        for (u32 x = 0; x < kDevTextureSize; ++x) {
            // A 16-pixel check plus a one-pixel grid line, so the eye can
            // measure a surface without a tape.
            const bool check = ((x / 16) + (y / 16)) % 2 == 0;
            const bool line = (x % 16 == 0) || (y % 16 == 0);

            f32 scale = check ? 1.0f : 0.72f;
            if (line) {
                scale *= 0.55f;
            }

            const usize offset = (static_cast<usize>(y) * kDevTextureSize + x) * 4;
            pixels[offset + 0] = static_cast<u8>(static_cast<f32>(base.r) * scale);
            pixels[offset + 1] = static_cast<u8>(static_cast<f32>(base.g) * scale);
            pixels[offset + 2] = static_cast<u8>(static_cast<f32>(base.b) * scale);
            pixels[offset + 3] = 255;
        }
    }
    return pixels;
}

}  // namespace kero::asset
