// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include <source_location>
#include <string_view>

namespace kero {

/// Reports a failed assertion and terminates. Never returns.
[[noreturn]] void assert_failed(std::string_view expression,
                                std::string_view message,
                                std::source_location where = std::source_location::current());

}  // namespace kero

/// KERO_ASSERT -- a claim about the program's own logic, checked in debug only.
///
/// Use it for things a correct program cannot violate: an index within bounds,
/// a winding still convex after a clip. It compiles to nothing in release.
#ifdef NDEBUG
#    define KERO_ASSERT(expr, ...) ((void)0)
#else
#    define KERO_ASSERT(expr, ...)                                                     \
        ((expr) ? (void)0                                                              \
                : ::kero::assert_failed(#expr, "" __VA_ARGS__ ""))
#endif

/// KERO_VERIFY -- a claim that is checked in every build, release included.
///
/// Use it where being wrong means writing a corrupt file or reading past the end
/// of one: file-format invariants, allocation results, anything driven by data
/// rather than by code. A map compiler that produces a broken .kbsp silently is
/// worse than one that stops, so these do not get compiled out.
#define KERO_VERIFY(expr, ...)                                                         \
    ((expr) ? (void)0 : ::kero::assert_failed(#expr, "" __VA_ARGS__ ""))

/// A branch that cannot be taken.
#define KERO_UNREACHABLE(...) ::kero::assert_failed("unreachable", "" __VA_ARGS__ "")
