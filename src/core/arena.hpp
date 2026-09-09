// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/assert.hpp"
#include "core/types.hpp"

#include <memory>
#include <new>
#include <span>
#include <type_traits>
#include <vector>

namespace kero {

/// A bump allocator that frees everything at once.
///
/// The map compilers allocate in a shape general-purpose allocators are bad at:
/// millions of small, short-lived, same-sized objects -- windings, plane lists,
/// portal fragments -- created during one stage and all dead at the end of it.
/// Bumping a pointer costs a few instructions where malloc costs a few hundred,
/// and freeing a whole stage is one pass over a handful of chunks instead of
/// millions of individual frees.
///
/// The tradeoff is deliberate and narrow: an arena cannot free one object. Use
/// it where the lifetime really is "until this stage ends", and ordinary
/// containers everywhere else. Objects placed here must be trivially
/// destructible -- the arena does not run destructors, and silently not running
/// one is a worse bug than not being allowed to ask.
class Arena {
public:
    static constexpr usize kDefaultChunkSize = 1u << 20;  // 1 MiB

    explicit Arena(usize chunk_size = kDefaultChunkSize);
    ~Arena();

    Arena(const Arena&) = delete;
    Arena& operator=(const Arena&) = delete;
    Arena(Arena&&) noexcept;
    Arena& operator=(Arena&&) noexcept;

    /// Raw bytes, aligned as asked. Never returns null: exhaustion throws
    /// std::bad_alloc, because a compile stage that cannot allocate has nothing
    /// useful left to do and a null check at every call site would be noise.
    [[nodiscard]] void* allocate(usize bytes, usize alignment);

    template <typename T, typename... Args>
    [[nodiscard]] T* create(Args&&... args) {
        static_assert(std::is_trivially_destructible_v<T>,
                      "Arena does not run destructors; use a container for types that need one");
        void* memory = allocate(sizeof(T), alignof(T));
        return std::construct_at(static_cast<T*>(memory), std::forward<Args>(args)...);
    }

    /// An uninitialised array. Value-initialise it yourself if you need that --
    /// the caller usually overwrites every element immediately, and zeroing a
    /// few million vertices that are about to be written is pure cost.
    template <typename T>
    [[nodiscard]] std::span<T> allocate_array(usize count) {
        static_assert(std::is_trivially_destructible_v<T>,
                      "Arena does not run destructors; use a container for types that need one");
        static_assert(std::is_trivially_default_constructible_v<T>,
                      "Arena arrays are uninitialised; T must not need a constructor");
        if (count == 0) {
            return {};
        }
        void* memory = allocate(sizeof(T) * count, alignof(T));
        return std::span<T>(static_cast<T*>(memory), count);
    }

    /// A saved position, for rewinding back to.
    struct Marker {
        usize chunk = 0;
        usize offset = 0;
    };

    [[nodiscard]] Marker mark() const { return Marker{current_chunk_, used_}; }

    /// Frees everything allocated since `marker`. Chunks past it are kept for
    /// reuse rather than returned to the system, which is the point: a loop that
    /// marks, works and rewinds settles at its high-water mark and then stops
    /// calling the system allocator entirely.
    void rewind(Marker marker);

    /// Frees everything, keeping the chunks.
    void reset();

    /// Frees everything and returns the memory to the system.
    void release();

    /// Bytes in use up to the current position, padding included. Computed
    /// rather than tracked, so it stays correct across a rewind.
    [[nodiscard]] usize bytes_used() const;

    /// Bytes held from the system, including chunks kept empty for reuse.
    [[nodiscard]] usize bytes_reserved() const;

private:
    struct Chunk {
        std::unique_ptr<std::byte[]> memory;
        usize size = 0;
    };

    void grow(usize minimum_bytes);

    std::vector<Chunk> chunks_;
    usize current_chunk_ = 0;   ///< Index into chunks_; meaningless when empty.
    usize used_ = 0;            ///< Bytes used in the current chunk.
    usize chunk_size_ = kDefaultChunkSize;
};

/// Marks an arena on construction and rewinds it on destruction.
///
/// The natural way to use an arena inside a function that borrows one: scratch
/// space that disappears on the way out, including on the way out through an
/// exception.
class ArenaScope {
public:
    explicit ArenaScope(Arena& arena) : arena_(&arena), marker_(arena.mark()) {}
    ~ArenaScope() { arena_->rewind(marker_); }

    ArenaScope(const ArenaScope&) = delete;
    ArenaScope& operator=(const ArenaScope&) = delete;

private:
    Arena* arena_;
    Arena::Marker marker_;
};

}  // namespace kero
