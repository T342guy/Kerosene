// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "core/jobs.hpp"

#include "core/assert.hpp"

#include <algorithm>
#include <chrono>
#include <random>

namespace kero {
namespace {

/// Which queue the current thread owns. Workers own theirs; a thread that only
/// submits and waits owns none and always steals, which is exactly the
/// behaviour wanted from the main thread inside `wait()`.
constexpr unsigned kNoQueue = ~0u;
thread_local unsigned t_queue_index = kNoQueue;
thread_local JobSystem* t_system = nullptr;

/// Per-thread victim selection. Seeded from the thread id so two workers do not
/// march through the same victims in the same order and collide repeatedly.
std::minstd_rand& steal_rng() {
    thread_local std::minstd_rand rng{std::random_device{}()};
    return rng;
}

}  // namespace

JobSystem::JobSystem(unsigned worker_count) {
    if (worker_count == 0) {
        const unsigned hardware = std::max(1u, std::thread::hardware_concurrency());
        // One fewer than the hardware allows: the submitting thread runs jobs
        // too, so spawning `hardware` workers oversubscribes by one and costs a
        // context switch on every join.
        worker_count = hardware - 1;
    }

    queues_.reserve(worker_count);
    for (unsigned i = 0; i < worker_count; ++i) {
        queues_.push_back(std::make_unique<Queue>());
    }

    workers_.reserve(worker_count);
    for (unsigned i = 0; i < worker_count; ++i) {
        workers_.emplace_back([this, i] { worker_main(i); });
    }
}

JobSystem::~JobSystem() {
    stopping_.store(true, std::memory_order_release);
    {
        std::lock_guard lock(sleep_mutex_);
    }
    wake_.notify_all();
    for (std::thread& worker : workers_) {
        if (worker.joinable()) {
            worker.join();
        }
    }
}

void JobSystem::submit(std::function<void()> job) {
    if (queues_.empty()) {
        // A single-threaded pool runs work inline. Not a special case to be
        // tolerated -- it is how the tests and `-threads 1` get deterministic
        // ordering when a parallel bug is being chased.
        job();
        return;
    }

    // A worker pushes to its own queue, keeping the data it just touched on the
    // core that touched it. Anyone else spreads submissions round-robin.
    unsigned index = (t_system == this && t_queue_index != kNoQueue)
                         ? t_queue_index
                         : submit_cursor_.fetch_add(1, std::memory_order_relaxed) %
                               static_cast<unsigned>(queues_.size());

    {
        std::lock_guard lock(queues_[index]->mutex);
        queues_[index]->jobs.push_back(std::move(job));
    }
    wake_.notify_one();
}

bool JobSystem::pop_local(std::function<void()>& out) {
    if (t_system != this || t_queue_index == kNoQueue) {
        return false;
    }
    Queue& queue = *queues_[t_queue_index];
    std::lock_guard lock(queue.mutex);
    if (queue.jobs.empty()) {
        return false;
    }
    // Last in, first out on the owner's end: the most recently spawned job is
    // the one whose inputs are still in cache.
    out = std::move(queue.jobs.back());
    queue.jobs.pop_back();
    return true;
}

bool JobSystem::steal(std::function<void()>& out) {
    const usize count = queues_.size();
    if (count == 0) {
        return false;
    }

    const usize start = steal_rng()() % count;
    for (usize n = 0; n < count; ++n) {
        const usize index = (start + n) % count;
        if (t_system == this && index == t_queue_index) {
            continue;
        }
        Queue& queue = *queues_[index];
        std::lock_guard lock(queue.mutex);
        if (queue.jobs.empty()) {
            continue;
        }
        // Stolen from the opposite end, so a thief takes the oldest job and
        // stays out of the owner's way at the other end of the deque.
        out = std::move(queue.jobs.front());
        queue.jobs.pop_front();
        return true;
    }
    return false;
}

bool JobSystem::run_one() {
    std::function<void()> job;
    if (pop_local(job) || steal(job)) {
        job();
        return true;
    }
    return false;
}

void JobSystem::worker_main(unsigned index) {
    t_system = this;
    t_queue_index = index;

    while (!stopping_.load(std::memory_order_acquire)) {
        if (run_one()) {
            continue;
        }

        // Nothing to do. Sleep on the condition variable rather than spinning:
        // a compile stage between phases can leave the pool idle for whole
        // milliseconds, and thirty-two spinning cores is a laptop fan and a
        // battery for no work done. The predicate is re-checked on wake, so a
        // job submitted just before the sleep is not missed for long.
        std::unique_lock lock(sleep_mutex_);
        wake_.wait_for(lock, std::chrono::milliseconds(1));
    }

    t_system = nullptr;
    t_queue_index = kNoQueue;
}

void JobSystem::Group::run(std::function<void()> job) {
    outstanding_.fetch_add(1, std::memory_order_relaxed);

    system_->submit([this, body = std::move(job)]() mutable {
        try {
            body();
        } catch (...) {
            // Only the first is kept. Once one job has failed the stage is
            // finished anyway, and the first failure is the one that explains
            // the others.
            std::call_once(exception_once_, [this] { exception_ = std::current_exception(); });
        }
        outstanding_.fetch_sub(1, std::memory_order_acq_rel);
    });
}

void JobSystem::Group::wait() {
    while (outstanding_.load(std::memory_order_acquire) != 0) {
        if (system_->run_one()) {
            continue;
        }
        // Outstanding jobs exist but none is available to run: they are all
        // being executed by other threads right now. Yield rather than spin
        // hard, and check again.
        std::this_thread::yield();
    }

    if (exception_) {
        std::exception_ptr held = exception_;
        exception_ = nullptr;
        std::rethrow_exception(held);
    }
}

JobSystem::Group::~Group() {
    // A group must not outlive its jobs, which hold a pointer to it. If the
    // scope is leaving because of an exception, the pending jobs still have to
    // finish before the counter they will decrement goes away.
    while (outstanding_.load(std::memory_order_acquire) != 0) {
        if (!system_->run_one()) {
            std::this_thread::yield();
        }
    }
}

void JobSystem::parallel_for(usize begin, usize end, const std::function<void(usize)>& body) {
    if (begin >= end) {
        return;
    }
    const usize count = end - begin;
    // Several chunks per thread, so an uneven workload rebalances by stealing.
    const usize target_chunks = static_cast<usize>(parallelism()) * 4;
    const usize grain = std::max<usize>(1, (count + target_chunks - 1) / target_chunks);

    parallel_for_chunks(begin, end, grain, [&body](usize lo, usize hi) {
        for (usize i = lo; i < hi; ++i) {
            body(i);
        }
    });
}

void JobSystem::parallel_for_chunks(usize begin, usize end, usize grain,
                                    const std::function<void(usize, usize)>& body) {
    if (begin >= end) {
        return;
    }
    KERO_ASSERT(grain > 0, "grain must be at least one");

    Group group(*this);
    for (usize lo = begin; lo < end; lo += grain) {
        const usize hi = std::min(lo + grain, end);
        group.run([&body, lo, hi] { body(lo, hi); });
    }
    group.wait();
}

JobSystem& jobs() {
    static JobSystem instance;
    return instance;
}

}  // namespace kero
