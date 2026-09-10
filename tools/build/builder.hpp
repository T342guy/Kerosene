// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "common/project.hpp"
#include "core/types.hpp"

#include <atomic>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

/// Running the compile stages, off the UI thread.
///
/// Shared by the Build panel and by Chisel's F9, because they are the same
/// thing with and without a launch at the end -- and because two code paths for
/// "compile this level" would eventually disagree about what a compile is.
namespace kero::build {

/// One line of build output, with enough attached to act on it.
struct Line {
    enum class Kind : u8 { Info, Warning, Error, Heading };

    Kind kind = Kind::Info;
    std::string text;

    /// The brush this line is about, when it is about one. Cleave's diagnostics
    /// carry a brush id, and being able to click an error and have the editor
    /// select the brush is the whole reason to run the compiler from inside the
    /// editor rather than from a terminal.
    i32 brush_id = 0;
    std::string map;
};

/// Compiles maps in the background.
///
/// One job at a time: the stages already use every core through the job system,
/// so running two maps at once would only make both slower and the log
/// unreadable.
class Builder {
public:
    Builder() = default;
    ~Builder();

    Builder(const Builder&) = delete;
    Builder& operator=(const Builder&) = delete;

    struct Options {
        /// Skip the expensive visibility pass. For a layout still moving.
        bool fast = false;
        /// Stop after Cleave. Enough to see whether the geometry compiles.
        bool no_visibility = false;
    };

    /// Starts a build. Does nothing if one is already running.
    void start(std::vector<std::filesystem::path> maps, const Options& options);

    [[nodiscard]] bool running() const { return running_.load(std::memory_order_acquire); }
    [[nodiscard]] bool succeeded() const { return succeeded_.load(std::memory_order_acquire); }

    /// Whether the last finished build found a leak, and where its path file is.
    [[nodiscard]] bool leaked() const { return leaked_.load(std::memory_order_acquire); }
    [[nodiscard]] std::string leak_file() const;

    /// A copy of the log so far. Copied rather than locked-and-read, because
    /// the UI holds it across a whole frame of drawing and the worker must not
    /// wait on that.
    [[nodiscard]] std::vector<Line> log() const;
    void clear();

    /// 0 to 1, for a progress bar.
    [[nodiscard]] f32 progress() const;

    /// Blocks until the running build finishes. For tests, and for shutdown.
    void wait();

private:
    void append(Line line);
    void run(std::vector<std::filesystem::path> maps, Options options);

    mutable std::mutex mutex_;
    std::vector<Line> log_;
    std::string leak_file_;

    std::thread worker_;
    std::atomic<bool> running_{false};
    std::atomic<bool> succeeded_{false};
    std::atomic<bool> leaked_{false};
    std::atomic<u32> done_{0};
    std::atomic<u32> total_{0};
};

}  // namespace kero::build
