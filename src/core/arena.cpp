// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "core/arena.hpp"

#include <algorithm>
#include <cstdint>

namespace kero {
namespace {

/// Past this, doubling the chunk size costs more in wasted tail than it saves
/// in chunk count.
constexpr usize kMaxChunkSize = 64u << 20;  // 64 MiB

}  // namespace

Arena::Arena(usize chunk_size)
    : chunk_size_(std::max<usize>(chunk_size, 4096)) {}

Arena::~Arena() = default;

Arena::Arena(Arena&& other) noexcept
    : chunks_(std::move(other.chunks_)),
      current_chunk_(other.current_chunk_),
      used_(other.used_),
      chunk_size_(other.chunk_size_) {
    other.current_chunk_ = 0;
    other.used_ = 0;
}

Arena& Arena::operator=(Arena&& other) noexcept {
    if (this != &other) {
        chunks_ = std::move(other.chunks_);
        current_chunk_ = other.current_chunk_;
        used_ = other.used_;
        chunk_size_ = other.chunk_size_;
        other.chunks_.clear();
        other.current_chunk_ = 0;
        other.used_ = 0;
    }
    return *this;
}

void Arena::grow(usize minimum_bytes) {
    // Chunks double up to a cap, so an arena that ends up holding a gigabyte
    // does it in tens of allocations rather than a thousand.
    const usize size = std::max(chunk_size_, minimum_bytes);
    chunks_.push_back(Chunk{std::make_unique<std::byte[]>(size), size});
    chunk_size_ = std::min(chunk_size_ * 2, kMaxChunkSize);
    current_chunk_ = chunks_.size() - 1;
    used_ = 0;
}

void* Arena::allocate(usize bytes, usize alignment) {
    KERO_VERIFY(alignment != 0 && (alignment & (alignment - 1)) == 0,
                "alignment must be a power of two");

    // A zero-byte request still gets a distinct address, so two objects are
    // never the same pointer.
    if (bytes == 0) {
        bytes = 1;
    }

    for (;;) {
        if (current_chunk_ < chunks_.size()) {
            Chunk& chunk = chunks_[current_chunk_];
            auto* cursor = chunk.memory.get() + used_;
            const auto address = reinterpret_cast<std::uintptr_t>(cursor);
            const auto aligned = (address + (alignment - 1)) & ~static_cast<std::uintptr_t>(alignment - 1);
            const usize padding = static_cast<usize>(aligned - address);

            // Written as a subtraction so an enormous `bytes` cannot overflow
            // the comparison into looking like it fits.
            if (padding <= chunk.size - used_ && bytes <= chunk.size - used_ - padding) {
                used_ += padding + bytes;
                return reinterpret_cast<void*>(aligned);
            }
        }

        // A chunk left over from a rewind is reused before the system is asked
        // for another one.
        if (current_chunk_ + 1 < chunks_.size() &&
            chunks_[current_chunk_ + 1].size >= bytes + alignment) {
            ++current_chunk_;
            used_ = 0;
            continue;
        }

        grow(bytes + alignment);
    }
}

void Arena::rewind(Marker marker) {
    KERO_ASSERT(marker.chunk <= chunks_.size(), "marker is from a different arena");
    current_chunk_ = marker.chunk;
    used_ = marker.offset;
}

void Arena::reset() {
    current_chunk_ = 0;
    used_ = 0;
}

void Arena::release() {
    chunks_.clear();
    current_chunk_ = 0;
    used_ = 0;
}

usize Arena::bytes_used() const {
    usize total = used_;
    for (usize i = 0; i < current_chunk_ && i < chunks_.size(); ++i) {
        total += chunks_[i].size;
    }
    return total;
}

usize Arena::bytes_reserved() const {
    usize total = 0;
    for (const Chunk& chunk : chunks_) {
        total += chunk.size;
    }
    return total;
}

}  // namespace kero
