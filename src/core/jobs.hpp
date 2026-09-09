// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <atomic>
#include <condition_variable>
#include <deque>
#include <exception>
#include <functional>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

namespace kero {

/// A work-stealing thread pool, and the reason the compilers are quick.
///
/// Source's map compilers thread their innermost loops and leave the rest
/// serial, which is why `vvis` on a large level is measured in hours on a
/// machine with thirty-two idle cores. Kerosene's stages are written against
/// this scheduler from the start: CSG chops brushes in parallel, portal flow
/// runs a job per portal, and a lightmap bake will run a job per luxel block.
///
/// Two properties make that safe to rely on:
///
///   * **Waiting threads work.** `Group::wait()` executes jobs while it waits,
///     so a job that spawns jobs and waits for them cannot deadlock the pool,
///     and the calling thread is never idle while work is outstanding. This is
///     what lets a stage be written as ordinary recursive fork/join.
///
///   * **Exceptions come back.** A job that throws does not terminate the
///     process; the first exception is re-thrown from `wait()` on the waiting
///     thread, where a compile stage can report which brush was at fault.
///
/// The per-worker queues are mutex-guarded rather than lock-free. At the job
/// sizes this engine actually uses -- a brush, a portal, a lightmap block, all
/// microseconds or longer -- the lock is far below the noise floor, and a
/// correct mutex is worth more than a clever deque nobody can prove.
class JobSystem {
public:
    /// `worker_count` of 0 means one worker per hardware thread, minus one for
    /// the thread that will be calling `wait()` and helping out.
    explicit JobSystem(unsigned worker_count = 0);
    ~JobSystem();

    JobSystem(const JobSystem&) = delete;
    JobSystem& operator=(const JobSystem&) = delete;

    [[nodiscard]] unsigned worker_count() const { return static_cast<unsigned>(workers_.size()); }

    /// Workers plus the submitting thread, which also runs jobs. This is the
    /// number to divide work by.
    [[nodiscard]] unsigned parallelism() const { return worker_count() + 1; }

    /// A fork/join scope. Jobs are submitted to it and waited for together.
    class Group {
    public:
        explicit Group(JobSystem& system) : system_(&system) {}
        ~Group();

        Group(const Group&) = delete;
        Group& operator=(const Group&) = delete;

        /// Submits a job. Safe to call from inside another job in the group.
        void run(std::function<void()> job);

        /// Runs outstanding jobs until the group is empty, then re-throws the
        /// first exception any of them raised.
        void wait();

    private:
        friend class JobSystem;

        JobSystem* system_;
        std::atomic<u32> outstanding_{0};
        std::once_flag exception_once_;
        std::exception_ptr exception_;
    };

    /// `body(i)` for every i in [begin, end), split into chunks.
    ///
    /// The default chunking aims for several chunks per thread, so an uneven
    /// workload -- and geometry workloads are always uneven -- still balances,
    /// without making the per-chunk overhead visible.
    void parallel_for(usize begin, usize end, const std::function<void(usize)>& body);

    /// `body(chunk_begin, chunk_end)` over half-open chunks of at least `grain`.
    ///
    /// Prefer this when the body has per-chunk setup worth amortising: a
    /// scratch arena, a plane hash, a local result vector to merge at the end.
    void parallel_for_chunks(usize begin, usize end, usize grain,
                             const std::function<void(usize, usize)>& body);

private:
    struct Queue {
        std::mutex mutex;
        std::deque<std::function<void()>> jobs;
    };

    void submit(std::function<void()> job);
    /// Runs one job from anywhere. False if there was nothing to run.
    bool run_one();
    bool pop_local(std::function<void()>& out);
    bool steal(std::function<void()>& out);
    void worker_main(unsigned index);

    std::vector<std::unique_ptr<Queue>> queues_;
    std::vector<std::thread> workers_;

    std::mutex sleep_mutex_;
    std::condition_variable wake_;
    std::atomic<bool> stopping_{false};
    std::atomic<u32> submit_cursor_{0};
};

/// The process-wide job system, created on first use.
///
/// Tools and the engine share one pool: two pools on one machine would each
/// size themselves to the whole machine and then fight over it.
[[nodiscard]] JobSystem& jobs();

}  // namespace kero
