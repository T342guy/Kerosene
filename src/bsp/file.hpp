// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "bsp/format.hpp"

#include <expected>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace kero::bsp {

/// A `.kbsp` file, held as its lumps.
///
/// The lumps are owned rather than pointed into a mapped file, and that is what
/// makes the three-stage pipeline work: Umbra loads what Cleave wrote, replaces
/// the visibility lump, and writes the file back without knowing what is in any
/// of the others. Radiance then does the same for lighting. A stage that had to
/// understand the whole format to add one lump to it would not be replaceable,
/// and being replaceable is the point.
///
/// A level is a few megabytes, so owning the bytes costs little. If that stops
/// being true the engine's loader is the one to make zero-copy, not this.
class File {
public:
    [[nodiscard]] static std::expected<File, std::string> load(const std::string& path);
    [[nodiscard]] static std::expected<File, std::string> from_bytes(
        std::span<const std::byte> bytes, std::string_view name = "<memory>");

    File() = default;

    [[nodiscard]] std::span<const std::byte> lump(LumpId id) const;

    /// A lump viewed as an array of `T`. Empty if the lump is empty or its
    /// length is not a whole number of elements -- which `load` has already
    /// rejected, so in practice this means "the stage that writes it has not
    /// run yet".
    template <typename T>
    [[nodiscard]] std::span<const T> lump_as(LumpId id) const {
        const std::span<const std::byte> bytes = lump(id);
        if (bytes.size() < sizeof(T) || bytes.size() % sizeof(T) != 0) {
            return {};
        }
        return std::span<const T>(reinterpret_cast<const T*>(bytes.data()),
                                  bytes.size() / sizeof(T));
    }

    /// The entity lump, as KeyValues text.
    [[nodiscard]] std::string_view entities() const;

    /// The material name at `offset` in the Materials lump.
    [[nodiscard]] std::string_view material(u32 offset) const;

    void set_lump(LumpId id, std::vector<std::byte> bytes);

    template <typename T>
    void set_lump(LumpId id, const std::vector<T>& values) {
        const auto* begin = reinterpret_cast<const std::byte*>(values.data());
        set_lump(id, std::vector<std::byte>(begin, begin + values.size() * sizeof(T)));
    }

    [[nodiscard]] std::vector<std::byte> to_bytes() const;
    [[nodiscard]] std::expected<void, std::string> save(const std::string& path) const;

    /// Whether Umbra has run. An empty visibility lump is not an error: an
    /// unvised level loads and plays, it just draws everything.
    [[nodiscard]] bool has_visibility() const { return !lump(LumpId::Visibility).empty(); }
    /// Whether Radiance has run.
    [[nodiscard]] bool has_lighting() const { return !lump(LumpId::Lighting).empty(); }

private:
    std::array<std::vector<std::byte>, kLumpCount> lumps_;
};

/// Decompresses one visibility row.
///
/// `which` is kVisPvs (what a cluster can see) or kVisPas (what it can hear).
/// `out` is resized to hold `cluster_count` bits and filled; on failure -- an
/// empty lump, a cluster out of range, a truncated row -- it is filled with
/// ones instead and false is returned.
///
/// All-ones on failure is the deliberate choice. An unvised level has an empty
/// lump, and the right behaviour there is to draw everything: too large a PVS
/// costs frame rate, too small one puts holes in the world. A visibility bug
/// should look slow, not broken.
[[nodiscard]] bool decode_visibility(std::span<const std::byte> lump, i32 cluster,
                                     usize which, usize cluster_count,
                                     std::vector<u8>& out);

/// How many clusters the visibility lump was built for. Zero if it is empty.
[[nodiscard]] u32 visibility_cluster_count(std::span<const std::byte> lump);

}  // namespace kero::bsp
