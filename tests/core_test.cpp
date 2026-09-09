// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include <doctest/doctest.h>

#include "core/arena.hpp"
#include "core/jobs.hpp"
#include "core/log.hpp"
#include "core/types.hpp"

#include <atomic>
#include <cstdint>
#include <numeric>
#include <stdexcept>
#include <string>
#include <vector>

using namespace kero;

TEST_CASE("Index distinguishes absent from present") {
    Index none;
    CHECK_FALSE(none.valid());

    Index zero{0};
    CHECK(zero.valid());
    CHECK(zero.get() == 0u);
    CHECK(zero != none);
}

TEST_CASE("Arena hands out aligned, distinct, usable memory") {
    Arena arena(4096);

    SUBCASE("respects alignment requests") {
        for (usize alignment : {alignof(u8), alignof(u32), usize{16}, usize{64}, usize{256}}) {
            void* p = arena.allocate(7, alignment);
            CHECK(reinterpret_cast<std::uintptr_t>(p) % alignment == 0);
        }
    }

    SUBCASE("zero-byte allocations still get distinct addresses") {
        void* a = arena.allocate(0, 1);
        void* b = arena.allocate(0, 1);
        CHECK(a != b);
    }

    SUBCASE("serves a request larger than the chunk size") {
        std::span<u32> big = arena.allocate_array<u32>(100'000);
        REQUIRE(big.size() == 100'000);
        std::iota(big.begin(), big.end(), 0u);
        CHECK(big[99'999] == 99'999u);
    }

    SUBCASE("objects survive later allocations") {
        struct Point { f64 x, y, z; };
        std::vector<Point*> points;
        for (usize i = 0; i < 10'000; ++i) {
            points.push_back(arena.create<Point>(static_cast<f64>(i), 0.0, 0.0));
        }
        for (usize i = 0; i < points.size(); ++i) {
            CHECK(points[i]->x == doctest::Approx(static_cast<f64>(i)));
        }
    }
}

TEST_CASE("Arena rewinds to a marker and reuses the memory") {
    Arena arena(4096);
    (void)arena.allocate(1000, 8);

    const usize before = arena.bytes_used();
    const Arena::Marker marker = arena.mark();

    void* first = arena.allocate(64, 8);
    (void)arena.allocate(2000, 8);
    CHECK(arena.bytes_used() > before);

    arena.rewind(marker);
    CHECK(arena.bytes_used() == before);

    // The same address comes back, which is the property that makes a
    // mark/work/rewind loop stop calling the system allocator.
    void* again = arena.allocate(64, 8);
    CHECK(again == first);
}

TEST_CASE("ArenaScope rewinds even when the scope leaves by exception") {
    Arena arena(4096);
    (void)arena.allocate(128, 8);
    const usize before = arena.bytes_used();

    try {
        ArenaScope scope(arena);
        (void)arena.allocate(4096 * 4, 8);
        throw std::runtime_error("stage failed");
    } catch (const std::runtime_error&) {  // NOLINT
    }

    CHECK(arena.bytes_used() == before);
    // The chunks it grew are kept, not returned -- that is the point of rewind.
    CHECK(arena.bytes_reserved() > before);
}

TEST_CASE("JobSystem runs every job exactly once") {
    JobSystem system;
    constexpr usize kCount = 10'000;

    std::vector<std::atomic<u32>> counts(kCount);
    system.parallel_for(0, kCount, [&counts](usize i) {
        counts[i].fetch_add(1, std::memory_order_relaxed);
    });

    for (usize i = 0; i < kCount; ++i) {
        CHECK(counts[i].load() == 1u);
    }
}

TEST_CASE("parallel_for_chunks covers the range exactly once, in order within a chunk") {
    JobSystem system;
    constexpr usize kCount = 5'000;

    std::vector<u32> seen(kCount, 0);
    std::atomic<usize> chunks{0};

    system.parallel_for_chunks(0, kCount, 64, [&](usize lo, usize hi) {
        chunks.fetch_add(1, std::memory_order_relaxed);
        for (usize i = lo; i < hi; ++i) {
            seen[i] = static_cast<u32>(i) + 1;
        }
    });

    CHECK(chunks.load() > 1);
    for (usize i = 0; i < kCount; ++i) {
        CHECK(seen[i] == static_cast<u32>(i) + 1);
    }
}

TEST_CASE("a waiting thread runs jobs, so nested fork/join cannot deadlock") {
    // Nesting deeper than the pool is wide: if waiting did not execute work,
    // this would starve and hang rather than fail.
    JobSystem system(2);
    std::atomic<u32> leaves{0};

    std::function<void(unsigned)> recurse = [&](unsigned depth) {
        if (depth == 0) {
            leaves.fetch_add(1, std::memory_order_relaxed);
            return;
        }
        JobSystem::Group group(system);
        for (int i = 0; i < 2; ++i) {
            group.run([&recurse, depth] { recurse(depth - 1); });
        }
        group.wait();
    };

    recurse(8);
    CHECK(leaves.load() == 256u);
}

TEST_CASE("an exception in a job is re-thrown from wait") {
    JobSystem system;
    JobSystem::Group group(system);

    for (int i = 0; i < 64; ++i) {
        group.run([i] {
            if (i == 40) {
                throw std::runtime_error("brush 40 is degenerate");
            }
        });
    }

    CHECK_THROWS_WITH_AS(group.wait(), "brush 40 is degenerate", std::runtime_error);
}

TEST_CASE("a single-threaded pool still runs everything") {
    JobSystem system(0);
    (void)system;

    JobSystem inline_system(1);
    std::atomic<u32> total{0};
    inline_system.parallel_for(0, 100, [&total](usize) {
        total.fetch_add(1, std::memory_order_relaxed);
    });
    CHECK(total.load() == 100u);
}

TEST_CASE("log categories filter by level and reach registered sinks") {
    static LogCategory category("test_category", LogLevel::Info);

    std::vector<std::string> lines;
    set_log_to_stderr(false);
    const u32 sink = add_log_sink([&lines](const LogRecord& record) {
        lines.emplace_back(std::string(record.category) + ": " + std::string(record.message));
    });

    KERO_DEBUG(category, "filtered out at {}", "info");
    KERO_INFO(category, "brush {} of {}", 3, 7);
    KERO_ERROR(category, "leak");

    category.set_minimum(LogLevel::Error);
    KERO_INFO(category, "now filtered too");

    remove_log_sink(sink);
    set_log_to_stderr(true);

    REQUIRE(lines.size() == 2);
    CHECK(lines[0] == "test_category: brush 3 of 7");
    CHECK(lines[1] == "test_category: leak");

    CHECK(find_log_category("test_category") == &category);
    CHECK(find_log_category("no_such_category") == nullptr);
}

TEST_CASE("log levels round-trip through their names") {
    LogLevel level = LogLevel::Off;
    CHECK(parse_log_level("warn", level));
    CHECK(level == LogLevel::Warn);
    CHECK(to_string(level) == "warn");
    CHECK_FALSE(parse_log_level("shout", level));
    CHECK(level == LogLevel::Warn);
}
