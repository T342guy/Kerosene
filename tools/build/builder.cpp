// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "build/builder.hpp"

#include "cleave/cleave.hpp"
#include "core/log.hpp"
#include "umbra/umbra.hpp"

#include <chrono>
#include <format>

namespace kero::build {
namespace {

KERO_LOG_CATEGORY(build_log, "build");

namespace fs = std::filesystem;

}  // namespace

Builder::~Builder() { wait(); }

void Builder::wait() {
    if (worker_.joinable()) {
        worker_.join();
    }
}

void Builder::append(Line line) {
    std::lock_guard lock(mutex_);
    log_.push_back(std::move(line));
}

std::vector<Line> Builder::log() const {
    std::lock_guard lock(mutex_);
    return log_;
}

std::string Builder::leak_file() const {
    std::lock_guard lock(mutex_);
    return leak_file_;
}

void Builder::clear() {
    std::lock_guard lock(mutex_);
    log_.clear();
    leak_file_.clear();
}

f32 Builder::progress() const {
    const u32 total = total_.load(std::memory_order_acquire);
    if (total == 0) {
        return 0.0f;
    }
    return static_cast<f32>(done_.load(std::memory_order_acquire)) /
           static_cast<f32>(total);
}

void Builder::start(std::vector<fs::path> maps, const Options& options) {
    if (running_.load(std::memory_order_acquire)) {
        return;
    }
    wait();  // Reap the previous worker before starting another.

    clear();
    running_.store(true, std::memory_order_release);
    succeeded_.store(false, std::memory_order_release);
    leaked_.store(false, std::memory_order_release);
    done_.store(0, std::memory_order_release);
    total_.store(static_cast<u32>(maps.size()), std::memory_order_release);

    worker_ = std::thread([this, maps = std::move(maps), options] {
        run(std::move(maps), options);
    });
}

void Builder::run(std::vector<fs::path> maps, Options options) {
    using Clock = std::chrono::steady_clock;
    const auto started = Clock::now();

    bool all_well = true;

    for (const fs::path& map : maps) {
        const std::string name = map.filename().string();
        append(Line{Line::Kind::Heading, name, 0, map.string()});

        cleave::Options cleave_options;
        cleave_options.fast = options.fast;
        cleave_options.no_portals = options.no_visibility;
        // The panel reports a leak rather than treating it as a failure -- an
        // unsealed level still loads and plays, and refusing to produce one
        // would make the diagnostic harder to act on rather than easier.
        cleave_options.allow_leaks = true;

        const auto stats = cleave::run(map.string(), cleave_options);
        if (!stats) {
            append(Line{Line::Kind::Error, stats.error().message, 0, map.string()});
            all_well = false;
            done_.fetch_add(1, std::memory_order_acq_rel);
            continue;
        }

        append(Line{Line::Kind::Info,
                    std::format("{} brushes, {} faces, {} leaves ({} open), {} portals",
                                stats->brushes, stats->faces, stats->tree.leaves,
                                stats->tree.empty_leaves, stats->tree.portals),
                    0, map.string()});

        for (const cleave::BrushProblem& problem : stats->problems) {
            // Carries the brush id, so the panel can offer to select it.
            append(Line{Line::Kind::Warning,
                        std::format("brush {}: {}", problem.map_id, problem.message),
                        problem.map_id, map.string()});
        }

        if (stats->leaked) {
            leaked_.store(true, std::memory_order_release);
            {
                std::lock_guard lock(mutex_);
                leak_file_ = stats->leak_path_file;
            }
            append(Line{Line::Kind::Error,
                        std::format("LEAK: the level is open to the void, reached from "
                                    "{}. Visibility and lighting will both be wrong "
                                    "until it is sealed",
                                    stats->leak_entity),
                        0, map.string()});
            all_well = false;
            // Umbra is skipped: flooding an unsealed level computes visibility
            // for the void, which takes a long time to produce a wrong answer.
            done_.fetch_add(1, std::memory_order_acq_rel);
            continue;
        }

        if (options.no_visibility) {
            done_.fetch_add(1, std::memory_order_acq_rel);
            continue;
        }

        fs::path compiled = map;
        compiled.replace_extension(".kbsp");

        umbra::Options umbra_options;
        umbra_options.fast = options.fast;
        const auto vis = umbra::run(compiled.string(), umbra_options);
        if (!vis) {
            append(Line{Line::Kind::Error, vis.error(), 0, map.string()});
            all_well = false;
            done_.fetch_add(1, std::memory_order_acq_rel);
            continue;
        }

        append(Line{Line::Kind::Info,
                    std::format("{} clusters, {:.1f} visible on average",
                                vis->clusters, vis->average_visible),
                    0, map.string()});
        done_.fetch_add(1, std::memory_order_acq_rel);
    }

    const auto elapsed = std::chrono::duration<f64>(Clock::now() - started).count();
    append(Line{all_well ? Line::Kind::Info : Line::Kind::Error,
                std::format("{} in {:.2f}s", all_well ? "built" : "finished with errors",
                            elapsed),
                0, {}});

    succeeded_.store(all_well, std::memory_order_release);
    running_.store(false, std::memory_order_release);
    KERO_INFO(build_log, "build {} in {:.2f}s", all_well ? "succeeded" : "failed", elapsed);
}

}  // namespace kero::build
