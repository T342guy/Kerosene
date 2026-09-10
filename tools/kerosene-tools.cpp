// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//
// The toolset.
//
// One application holding every tool, switched with a rail down the left edge:
// the world editor, the build panel, and whatever comes after. None of it is
// the engine -- the toolset and the engine share file formats and nothing else,
// which is the arrangement that made Source's tools worth having, and the build
// enforces it rather than trusting it.

#include "build/build_panel.hpp"
#include "chisel/chisel_panel.hpp"
#include "core/log.hpp"
#include "shell/shell.hpp"

#include <cstdio>
#include <string>
#include <vector>

namespace {

KERO_LOG_CATEGORY(tools_log, "tools");

void print_usage() {
    std::fputs(
        "kerosene-tools -- the Kerosene toolset\n"
        "\n"
        "Usage: kerosene-tools [map.kmap] [options]\n"
        "\n"
        "Opens the toolset window. A map given on the command line is opened in\n"
        "Chisel; without one, Chisel starts empty.\n"
        "\n"
        "  --width, --height <n>  window size\n"
        "  --frames <n>           draw this many frames and exit; a smoke test\n"
        "  --log-level <level>    trace, debug, info, warn, error\n"
        "  -h, --help             this message\n",
        stdout);
}

}  // namespace

int main(int argc, char** argv) {
    std::vector<std::string> arguments(argv + 1, argv + argc);

    kero::i32 width = 1600;
    kero::i32 height = 900;
    kero::i64 frame_limit = -1;
    std::string map;

    for (kero::usize i = 0; i < arguments.size(); ++i) {
        const std::string& argument = arguments[i];
        if (argument == "-h" || argument == "--help") {
            print_usage();
            return 0;
        }
        if (argument == "--width" && i + 1 < arguments.size()) {
            width = std::stoi(arguments[++i]);
            continue;
        }
        if (argument == "--height" && i + 1 < arguments.size()) {
            height = std::stoi(arguments[++i]);
            continue;
        }
        if (argument == "--frames" && i + 1 < arguments.size()) {
            frame_limit = std::stoll(arguments[++i]);
            continue;
        }
        if (argument == "--log-level" && i + 1 < arguments.size()) {
            kero::LogLevel level{};
            if (!kero::parse_log_level(arguments[++i], level)) {
                std::fprintf(stderr, "unknown log level '%s'\n", arguments[i].c_str());
                return 2;
            }
            kero::for_each_log_category(
                [level](kero::LogCategory& category) { category.set_minimum(level); });
            continue;
        }
        if (argument.starts_with('-')) {
            std::fprintf(stderr, "unknown option '%s'\n\n", argument.c_str());
            print_usage();
            return 2;
        }
        map = argument;
    }

    auto shell = kero::shell::Shell::create("Kerosene Toolset", width, height);
    if (!shell) {
        std::fprintf(stderr, "%s\n", shell.error().c_str());
        return 1;
    }

    auto chisel = std::make_unique<kero::chisel::ChiselPanel>();
    if (!map.empty()) {
        chisel->open(map);
    }

    (*shell)->add_panel(std::move(chisel));
    (*shell)->add_panel(std::make_unique<kero::build::BuildPanel>());

    return (*shell)->run(frame_limit);
}
