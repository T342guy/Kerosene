// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//
// The toolset. One binary, one subcommand per stage.
//
// The stages stay separate -- each reads and writes files, so you can stop
// after any of them, run them from a Makefile, or parallelise them across a
// build farm. What they share is one executable, because installing eight
// binaries to compile one level is a worse deal than typing a subcommand.
//
// Source's real design achievement was never its renderer; it was that Hammer,
// vbsp, vvis and vrad are separate from the game and share file formats. That
// is the shape being kept here.

#include "cleave/cleave.hpp"
#include "common/cli.hpp"
#include "core/log.hpp"

#include <cstdio>
#include <format>
#include <string>

namespace {

KERO_LOG_CATEGORY(log, "tools");

void print_usage() {
    std::fputs(
        "kerosene-tools -- the Kerosene toolset\n"
        "\n"
        "Usage: kerosene-tools <stage> [options] <file>\n"
        "\n"
        "Stages:\n"
        "  cleave <map.kmap>    CSG, BSP, portals, leak detection  -> .kbsp\n"
        "\n"
        "Common options:\n"
        "  --fast               skip the expensive passes; for a layout still moving\n"
        "  --log-level <level>  trace, debug, info, warn, error\n"
        "  -h, --help           this message\n"
        "\n"
        "cleave options:\n"
        "  -o, --output <file>  where to write; defaults to the input with .kbsp\n"
        "  --allow-leaks        do not report an unsealed level as an error\n"
        "  --no-portals         stop after the tree; skips the leak check\n"
        "  --split-cost <n>     how much a brush split costs the split heuristic\n"
        "  --balance-cost <n>   how much an unbalanced division costs it\n"
        "\n"
        "Not yet implemented: umbra (visibility), radiance (lighting),\n"
        "alchemy (textures), forge (models), timbre (sound), vault (archives),\n"
        "kiln (whole-project builds), chisel (the editor).\n",
        stdout);
}

int run_cleave(kero::tools::Args& args) {
    kero::cleave::Options options;
    options.allow_leaks = args.flag("allow-leaks");
    options.no_portals = args.flag("no-portals");
    options.fast = args.flag("fast");

    if (const auto output = args.option("output")) {
        options.output = *output;
    } else if (const auto shorthand = args.option("o")) {
        options.output = *shorthand;
    }
    if (const auto cost = args.number("split-cost")) {
        options.policy.split_cost = *cost;
    }
    if (const auto cost = args.number("balance-cost")) {
        options.policy.balance_cost = *cost;
    }

    const std::vector<std::string> files = args.positional();
    if (files.empty()) {
        std::fputs("cleave: no map given. Try: kerosene-tools cleave map.kmap\n", stderr);
        return 2;
    }

    // An unknown flag is an error, not something to ignore: a typo in a build
    // script that quietly compiles the level differently is a bug that hides
    // for months.
    if (const std::vector<std::string> left = args.unconsumed(); !left.empty()) {
        for (const std::string& flag : left) {
            std::fprintf(stderr, "cleave: unknown option '%s'\n", flag.c_str());
        }
        return 2;
    }

    const auto stats = kero::cleave::run(files.front(), options);
    if (!stats) {
        KERO_ERROR(log, "{}", stats.error().message);
        return 1;
    }

    std::fprintf(stdout,
                 "\n"
                 "  brushes      %zu (%zu detail)\n"
                 "  planes       %zu\n"
                 "  nodes        %zu\n"
                 "  leaves       %zu (%zu solid, %zu open)\n"
                 "  portals      %zu\n"
                 "  brush splits %zu\n"
                 "  tree depth   %zu\n"
                 "  faces        %zu\n"
                 "  vertices     %zu\n",
                 stats->brushes, stats->detail_brushes, stats->planes, stats->tree.nodes,
                 stats->tree.leaves, stats->tree.solid_leaves, stats->tree.empty_leaves,
                 stats->tree.portals, stats->tree.splits, stats->tree.max_depth,
                 stats->faces, stats->vertices);

    if (!stats->problems.empty()) {
        std::fprintf(stdout, "  %zu brushes could not be compiled\n",
                     stats->problems.size());
    }

    if (stats->leaked) {
        std::fprintf(stderr,
                     "\nLEAK: reached the void from %s.\n"
                     "The level is not sealed. Visibility and lighting will both be\n"
                     "wrong until it is. The path out is in %s -- follow it from the\n"
                     "entity to the hole.\n",
                     stats->leak_entity.c_str(),
                     stats->leak_path_file.empty() ? "(unwritten)"
                                                   : stats->leak_path_file.c_str());
        return options.allow_leaks ? 0 : 1;
    }

    return 0;
}

}  // namespace

int main(int argc, char** argv) {
    kero::tools::Args args(argc, argv);

    if (const auto level = args.option("log-level")) {
        kero::LogLevel parsed{};
        if (!kero::parse_log_level(*level, parsed)) {
            std::fprintf(stderr, "unknown log level '%s'\n", level->c_str());
            return 2;
        }
        kero::for_each_log_category(
            [parsed](kero::LogCategory& category) { category.set_minimum(parsed); });
    }

    if (args.flag("help") || args.flag("h") || args.command().empty()) {
        print_usage();
        return args.command().empty() ? 2 : 0;
    }

    if (args.command() == "cleave") {
        return run_cleave(args);
    }

    std::fprintf(stderr, "unknown stage '%s'\n\n", std::string(args.command()).c_str());
    print_usage();
    return 2;
}
