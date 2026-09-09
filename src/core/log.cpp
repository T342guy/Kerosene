// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "core/log.hpp"

#include <algorithm>
#include <cstdio>
#include <mutex>
#include <vector>

#if defined(_WIN32)
#    include <io.h>
#    define KERO_ISATTY(fd) _isatty(fd)
#else
#    include <unistd.h>
#    define KERO_ISATTY(fd) isatty(fd)
#endif

namespace kero {
namespace {

/// The registry is a function-local static so that a LogCategory declared at
/// namespace scope in another translation unit can register itself during
/// static initialisation without depending on this one being initialised first.
struct Registry {
    std::mutex mutex;
    std::vector<LogCategory*> categories;
    std::vector<std::pair<u32, LogSink>> sinks;
    u32 next_handle = 1;
    bool to_stderr = true;
};

Registry& registry() {
    static Registry instance;
    return instance;
}

bool stderr_is_tty() {
    static const bool value = KERO_ISATTY(2) != 0;
    return value;
}

/// Colour is per level rather than per category: the thing you scan a wall of
/// compiler output for is the warnings, not which stage produced them.
const char* colour_for(LogLevel level) {
    if (!stderr_is_tty()) {
        return "";
    }
    switch (level) {
        case LogLevel::Trace: return "\033[2;37m";
        case LogLevel::Debug: return "\033[36m";
        case LogLevel::Info:  return "";
        case LogLevel::Warn:  return "\033[33m";
        case LogLevel::Error: return "\033[1;31m";
        case LogLevel::Off:   return "";
    }
    return "";
}

const char* colour_reset() { return stderr_is_tty() ? "\033[0m" : ""; }

}  // namespace

std::string_view to_string(LogLevel level) {
    switch (level) {
        case LogLevel::Trace: return "trace";
        case LogLevel::Debug: return "debug";
        case LogLevel::Info:  return "info";
        case LogLevel::Warn:  return "warn";
        case LogLevel::Error: return "error";
        case LogLevel::Off:   return "off";
    }
    return "?";
}

bool parse_log_level(std::string_view text, LogLevel& out) {
    constexpr LogLevel kLevels[] = {LogLevel::Trace, LogLevel::Debug, LogLevel::Info,
                                    LogLevel::Warn,  LogLevel::Error, LogLevel::Off};
    for (LogLevel level : kLevels) {
        if (text == to_string(level)) {
            out = level;
            return true;
        }
    }
    return false;
}

LogCategory::LogCategory(std::string_view name, LogLevel minimum)
    : name_(name), minimum_(minimum) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    reg.categories.push_back(this);
}

LogCategory* find_log_category(std::string_view name) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    for (LogCategory* category : reg.categories) {
        if (category->name() == name) {
            return category;
        }
    }
    return nullptr;
}

void for_each_log_category(const std::function<void(LogCategory&)>& fn) {
    Registry& reg = registry();
    // Copied under the lock, then called outside it: a callback that logs (or
    // that retunes a category) must not deadlock on the registry.
    std::vector<LogCategory*> snapshot;
    {
        std::lock_guard lock(reg.mutex);
        snapshot = reg.categories;
    }
    for (LogCategory* category : snapshot) {
        fn(*category);
    }
}

u32 add_log_sink(LogSink sink) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    const u32 handle = reg.next_handle++;
    reg.sinks.emplace_back(handle, std::move(sink));
    return handle;
}

void remove_log_sink(u32 handle) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    std::erase_if(reg.sinks, [handle](const auto& entry) { return entry.first == handle; });
}

void set_log_to_stderr(bool enabled) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    reg.to_stderr = enabled;
}

void log_write(LogLevel level, const LogCategory& category, std::string_view message) {
    const LogRecord record{level, category.name(), message};

    Registry& reg = registry();
    std::vector<std::pair<u32, LogSink>> sinks;
    bool to_stderr = false;
    {
        std::lock_guard lock(reg.mutex);
        sinks = reg.sinks;
        to_stderr = reg.to_stderr;
    }

    if (to_stderr) {
        // One fprintf rather than several, so lines from parallel compile jobs
        // interleave between records instead of within one.
        std::fprintf(stderr, "%s[%.*s] %.*s%s\n",
                     colour_for(level),
                     static_cast<int>(record.category.size()), record.category.data(),
                     static_cast<int>(message.size()), message.data(),
                     colour_reset());
    }

    for (const auto& [handle, sink] : sinks) {
        (void)handle;
        sink(record);
    }
}

}  // namespace kero
