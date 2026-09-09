// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include <cstddef>
#include <cstdint>

namespace kero {

using i8  = std::int8_t;
using i16 = std::int16_t;
using i32 = std::int32_t;
using i64 = std::int64_t;

using u8  = std::uint8_t;
using u16 = std::uint16_t;
using u32 = std::uint32_t;
using u64 = std::uint64_t;

using f32 = float;
using f64 = double;

using usize = std::size_t;
using isize = std::ptrdiff_t;

/// An index that may be absent, stored in the space an index already takes.
///
/// BSP structures are dense arrays referring to each other by index, and about
/// a third of those references are optional -- a node's child, a face's
/// lightmap, a leaf's cluster. A separate flag per reference would cost more
/// memory than the index, and a signed -1 sentinel invites arithmetic on a
/// value that is not an index. This is the sentinel, named.
struct Index {
    static constexpr u32 kNone = 0xFFFF'FFFFu;

    u32 value = kNone;

    constexpr Index() = default;
    constexpr explicit Index(u32 v) : value(v) {}

    [[nodiscard]] constexpr bool valid() const { return value != kNone; }
    [[nodiscard]] constexpr u32 get() const { return value; }

    friend constexpr bool operator==(Index, Index) = default;
};

/// Deleted copy and move, for types that own a resource or a thread.
struct NonCopyable {
    NonCopyable() = default;
    NonCopyable(const NonCopyable&) = delete;
    NonCopyable& operator=(const NonCopyable&) = delete;
    NonCopyable(NonCopyable&&) = delete;
    NonCopyable& operator=(NonCopyable&&) = delete;
    ~NonCopyable() = default;
};

}  // namespace kero
