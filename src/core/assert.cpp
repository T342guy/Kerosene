// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "core/assert.hpp"

#include <cstdio>
#include <cstdlib>

namespace kero {

void assert_failed(std::string_view expression,
                   std::string_view message,
                   std::source_location where) {
    // Written straight to stderr rather than through Log: an assertion may fire
    // while the log itself is half-initialised, and the one job left at that
    // point is to say what happened before the process goes away.
    std::fprintf(stderr, "\nassertion failed: %.*s\n",
                 static_cast<int>(expression.size()), expression.data());
    if (!message.empty()) {
        std::fprintf(stderr, "  %.*s\n",
                     static_cast<int>(message.size()), message.data());
    }
    std::fprintf(stderr, "  at %s:%u, in %s\n",
                 where.file_name(), where.line(), where.function_name());
    std::fflush(stderr);
    std::abort();
}

}  // namespace kero
