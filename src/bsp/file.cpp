// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "bsp/file.hpp"

#include <algorithm>
#include <array>
#include <cstring>
#include <format>
#include <fstream>

namespace kero::bsp {
namespace {

/// Every lump whose contents are a fixed-size record, so `load` can reject a
/// truncated file up front rather than letting a half-record reach a reader.
struct RecordSize {
    LumpId id;
    usize size;
};

constexpr RecordSize kRecordSizes[] = {
    {LumpId::Planes, sizeof(DiskPlane)},
    {LumpId::Vertices, sizeof(DiskVertex)},
    {LumpId::FaceVertices, sizeof(u32)},
    {LumpId::Faces, sizeof(DiskFace)},
    {LumpId::Nodes, sizeof(DiskNode)},
    {LumpId::Leaves, sizeof(DiskLeaf)},
    {LumpId::LeafFaces, sizeof(u32)},
    {LumpId::LeafBrushes, sizeof(u32)},
    {LumpId::Brushes, sizeof(DiskBrush)},
    {LumpId::BrushSides, sizeof(DiskBrushSide)},
    {LumpId::TexInfo, sizeof(DiskTexInfo)},
    {LumpId::Models, sizeof(DiskModel)},
};

std::string_view name_of(LumpId id) {
    switch (id) {
        case LumpId::Entities:     return "entities";
        case LumpId::Planes:       return "planes";
        case LumpId::Vertices:     return "vertices";
        case LumpId::FaceVertices: return "face vertices";
        case LumpId::Faces:        return "faces";
        case LumpId::Nodes:        return "nodes";
        case LumpId::Leaves:       return "leaves";
        case LumpId::LeafFaces:    return "leaf faces";
        case LumpId::LeafBrushes:  return "leaf brushes";
        case LumpId::Brushes:      return "brushes";
        case LumpId::BrushSides:   return "brush sides";
        case LumpId::TexInfo:      return "texinfo";
        case LumpId::Materials:    return "materials";
        case LumpId::Visibility:   return "visibility";
        case LumpId::Lighting:     return "lighting";
        case LumpId::Models:       return "models";
        case LumpId::Count:        break;
    }
    return "?";
}

}  // namespace

std::expected<File, std::string> File::from_bytes(std::span<const std::byte> bytes,
                                                  std::string_view name) {
    if (bytes.size() < sizeof(Header)) {
        return std::unexpected(std::format(
            "{}: too short to be a Kerosene .kbsp -- it is {} bytes and the header "
            "alone is {}", name, bytes.size(), sizeof(Header)));
    }

    Header header{};
    std::memcpy(&header, bytes.data(), sizeof(header));

    if (header.magic != kMagic) {
        return std::unexpected(std::format(
            "{}: not a Kerosene .kbsp (wrong magic). Kerosene cannot open Source "
            "or Quake maps, and does not try to", name));
    }
    if (header.version != kVersion) {
        // Strict on purpose. Compiled output is regenerable from the .kmap
        // beside it, so a best-effort read of a format this build does not
        // understand buys nothing and risks a level that is subtly wrong.
        return std::unexpected(std::format(
            "{}: .kbsp version {} but this build writes version {}. Recompile "
            "the map: kerosene-tools cleave <map>.kmap",
            name, header.version, kVersion));
    }

    File file;
    for (usize i = 0; i < kLumpCount; ++i) {
        const Lump& lump = header.lumps[i];
        if (static_cast<usize>(lump.offset) + lump.length > bytes.size()) {
            return std::unexpected(std::format(
                "{}: the {} lump runs past the end of the file; it is truncated",
                name, name_of(static_cast<LumpId>(i))));
        }
        const auto* begin = bytes.data() + lump.offset;
        file.lumps_[i].assign(begin, begin + lump.length);
    }

    for (const RecordSize& record : kRecordSizes) {
        const usize length = file.lumps_[static_cast<usize>(record.id)].size();
        if (length % record.size != 0) {
            return std::unexpected(std::format(
                "{}: the {} lump is {} bytes, which is not a whole number of "
                "{}-byte records", name, name_of(record.id), length, record.size));
        }
    }

    return file;
}

std::expected<File, std::string> File::load(const std::string& path) {
    std::ifstream stream(path, std::ios::binary);
    if (!stream) {
        return std::unexpected(std::format("cannot open {}", path));
    }

    stream.seekg(0, std::ios::end);
    const auto size = static_cast<usize>(stream.tellg());
    stream.seekg(0, std::ios::beg);

    std::vector<std::byte> bytes(size);
    stream.read(reinterpret_cast<char*>(bytes.data()), static_cast<std::streamsize>(size));
    if (!stream) {
        return std::unexpected(std::format("reading {} failed", path));
    }

    return from_bytes(bytes, path);
}

std::span<const std::byte> File::lump(LumpId id) const {
    return lumps_[static_cast<usize>(id)];
}

void File::set_lump(LumpId id, std::vector<std::byte> bytes) {
    lumps_[static_cast<usize>(id)] = std::move(bytes);
}

std::string_view File::entities() const {
    const std::span<const std::byte> bytes = lump(LumpId::Entities);
    return std::string_view(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

std::string_view File::material(u32 offset) const {
    const std::span<const std::byte> bytes = lump(LumpId::Materials);
    if (offset >= bytes.size()) {
        return {};
    }
    const auto* begin = reinterpret_cast<const char*>(bytes.data()) + offset;
    const usize available = bytes.size() - offset;
    const usize length = ::strnlen(begin, available);
    return std::string_view(begin, length);
}

std::vector<std::byte> File::to_bytes() const {
    Header header;
    std::vector<std::byte> out(sizeof(Header));

    for (usize i = 0; i < kLumpCount; ++i) {
        // Four-byte aligned, so a reader can point a struct at a lump rather
        // than copying it out.
        while (out.size() % 4 != 0) {
            out.push_back(std::byte{0});
        }
        header.lumps[i].offset = static_cast<u32>(out.size());
        header.lumps[i].length = static_cast<u32>(lumps_[i].size());
        out.insert(out.end(), lumps_[i].begin(), lumps_[i].end());
    }

    std::memcpy(out.data(), &header, sizeof(header));
    return out;
}

std::expected<void, std::string> File::save(const std::string& path) const {
    const std::vector<std::byte> bytes = to_bytes();
    std::ofstream stream(path, std::ios::binary | std::ios::trunc);
    if (!stream) {
        return std::unexpected(std::format("cannot write {}", path));
    }
    stream.write(reinterpret_cast<const char*>(bytes.data()),
                 static_cast<std::streamsize>(bytes.size()));
    if (!stream) {
        return std::unexpected(std::format("writing {} failed", path));
    }
    return {};
}

u32 visibility_cluster_count(std::span<const std::byte> lump) {
    if (lump.size() < sizeof(DiskVisHeader)) {
        return 0;
    }
    DiskVisHeader header{};
    std::memcpy(&header, lump.data(), sizeof(header));
    return header.cluster_count;
}

bool decode_visibility(std::span<const std::byte> lump, i32 cluster, usize which,
                       usize cluster_count, std::vector<u8>& out) {
    const usize stride = (cluster_count + 7) / 8;
    out.assign(stride, 0xFF);

    if (which > kVisPas || cluster < 0 || lump.size() < sizeof(DiskVisHeader)) {
        return false;
    }

    DiskVisHeader header{};
    std::memcpy(&header, lump.data(), sizeof(header));
    if (static_cast<u32>(cluster) >= header.cluster_count) {
        return false;
    }

    const usize table = sizeof(DiskVisHeader);
    const usize entries = static_cast<usize>(header.cluster_count) * 2;
    if (lump.size() < table + entries * sizeof(u32)) {
        return false;
    }

    u32 offset = 0;
    std::memcpy(&offset,
                lump.data() + table +
                    (static_cast<usize>(cluster) * 2 + which) * sizeof(u32),
                sizeof(offset));
    if (offset >= lump.size()) {
        return false;
    }

    // Run-length decode: a zero byte is followed by the length of its run, and
    // any other byte stands for itself.
    std::fill(out.begin(), out.end(), 0);
    usize written = 0;
    usize read = offset;
    while (written < stride) {
        if (read >= lump.size()) {
            std::fill(out.begin(), out.end(), 0xFF);
            return false;
        }
        const auto byte = static_cast<u8>(lump[read++]);
        if (byte != 0) {
            out[written++] = byte;
            continue;
        }
        if (read >= lump.size()) {
            std::fill(out.begin(), out.end(), 0xFF);
            return false;
        }
        usize run = static_cast<u8>(lump[read++]);
        while (run-- > 0 && written < stride) {
            out[written++] = 0;
        }
    }

    // Bits past the last cluster are meaningless; cleared so a caller counting
    // set bits gets the right answer.
    if (const usize slack = stride * 8 - cluster_count; slack > 0 && stride > 0) {
        out[stride - 1] = static_cast<u8>(out[stride - 1] & (0xFFu >> slack));
    }
    return true;
}

}  // namespace kero::bsp
