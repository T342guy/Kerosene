// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <atomic>
#include <format>
#include <functional>
#include <string_view>

namespace kero {

enum class LogLevel : u8 {
    Trace,    ///< Per-item detail: one line per winding, one per portal.
    Debug,    ///< Per-stage detail useful while working on that stage.
    Info,     ///< What the program is doing, at the granularity a user cares about.
    Warn,     ///< Something is wrong with the content, and we carried on anyway.
    Error,    ///< Something is wrong and we could not carry on.
    Off,      ///< Not a level to log at; a threshold that admits nothing.
};

[[nodiscard]] std::string_view to_string(LogLevel level);

/// Parses "info", "warn", ... Returns false and leaves `out` alone if unknown.
[[nodiscard]] bool parse_log_level(std::string_view text, LogLevel& out);

/// A named logging channel with its own threshold.
///
/// Declared once per subsystem, at namespace scope, and filtered independently:
/// `+log_level vis trace` while debugging portal flow should not also turn on
/// every winding operation in the CSG stage. The threshold is atomic because
/// the console can move it from another thread while compile jobs are running.
class LogCategory {
public:
    explicit LogCategory(std::string_view name, LogLevel minimum = LogLevel::Info);

    [[nodiscard]] std::string_view name() const { return name_; }
    [[nodiscard]] LogLevel minimum() const { return minimum_.load(std::memory_order_relaxed); }
    void set_minimum(LogLevel level) { minimum_.store(level, std::memory_order_relaxed); }
    [[nodiscard]] bool enabled(LogLevel level) const { return level >= minimum(); }

private:
    std::string_view name_;
    std::atomic<LogLevel> minimum_;
};

/// Finds a registered category by name, for the console to retune. Null if none.
[[nodiscard]] LogCategory* find_log_category(std::string_view name);

/// Calls `fn` with every registered category, for `log_categories` to list them.
void for_each_log_category(const std::function<void(LogCategory&)>& fn);

struct LogRecord {
    LogLevel level;
    std::string_view category;
    std::string_view message;
};

/// A destination for log records. The developer console registers one of these,
/// which is why it can show engine output without the engine knowing it exists.
using LogSink = std::function<void(const LogRecord&)>;

/// Registers a sink and returns a handle for removing it.
[[nodiscard]] u32 add_log_sink(LogSink sink);
void remove_log_sink(u32 handle);

/// Whether to keep writing to stderr once other sinks exist. On by default: a
/// tool that logs only into a window nobody opened has not reported anything.
void set_log_to_stderr(bool enabled);

/// Emits one record. Prefer the macros, which skip formatting when disabled.
void log_write(LogLevel level, const LogCategory& category, std::string_view message);

namespace detail {
template <typename... Args>
void log_formatted(LogLevel level, const LogCategory& category,
                   std::format_string<Args...> fmt, Args&&... args) {
    log_write(level, category, std::format(fmt, std::forward<Args>(args)...));
}
}  // namespace detail

}  // namespace kero

/// Declares a logging channel. One per subsystem, at namespace scope.
#define KERO_LOG_CATEGORY(symbol, name) \
    ::kero::LogCategory symbol { name }

/// The threshold test happens before the arguments are formatted, so a trace
/// call in the inner loop of the CSG stage costs one relaxed load when it is
/// switched off. That is the difference between leaving the instrumentation in
/// the code and deleting it once the bug is found.
#define KERO_LOG(category, level, ...)                                          \
    do {                                                                        \
        if ((category).enabled(level)) {                                        \
            ::kero::detail::log_formatted(level, (category), __VA_ARGS__);       \
        }                                                                       \
    } while (false)

#define KERO_TRACE(category, ...) KERO_LOG(category, ::kero::LogLevel::Trace, __VA_ARGS__)
#define KERO_DEBUG(category, ...) KERO_LOG(category, ::kero::LogLevel::Debug, __VA_ARGS__)
#define KERO_INFO(category, ...)  KERO_LOG(category, ::kero::LogLevel::Info,  __VA_ARGS__)
#define KERO_WARN(category, ...)  KERO_LOG(category, ::kero::LogLevel::Warn,  __VA_ARGS__)
#define KERO_ERROR(category, ...) KERO_LOG(category, ::kero::LogLevel::Error, __VA_ARGS__)
