// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//
// The engine.
//
// Headless here; the windowed path adds a renderer and an input source on top
// of the same Host. That is not a convenience -- the simulation running without
// a display is what makes a dedicated server and a CI playthrough the same
// code, and it is enforced by the build rather than by intention.

#include "console/console.hpp"
#include "core/log.hpp"
#include "engine/host.hpp"
#include "kv/keyvalues.hpp"

#if KEROSENE_HAS_RENDER
#    include "render/renderer.hpp"
#endif

#include <algorithm>
#include <chrono>
#include <optional>
#include <cstdio>
#include <filesystem>
#include <string>
#include <thread>
#include <vector>

namespace {

KERO_LOG_CATEGORY(host_log, "kerosene");

using kero::console::ConVar;

ConVar host_framerate("host_framerate", "0",
                      "Frames per second to hold to. 0 runs as fast as the display "
                      "allows; headless uses it to avoid spinning a core.");

void print_usage() {
    std::fputs(
        "kerosene -- the Kerosene engine\n"
        "\n"
        "Usage: kerosene [options] [+command ...]\n"
        "\n"
        "  --headless [ticks]   run the simulation with no display, for a\n"
        "                       dedicated server or a test. With a tick count,\n"
        "                       runs that many and exits.\n"
        "  --width, --height    window size\n"
        "  --frames <n>         draw this many frames and exit; a smoke test\n"
        "  --map <name>         load a map from the content tree\n"
        "  -h, --help           this message\n"
        "\n"
        "Anything settable is settable at launch, because the console, a key\n"
        "bind, a .kcfg file and a +argument are all one mechanism:\n"
        "\n"
        "  kerosene +map kero_start +sv_gravity 200\n",
        stdout);
}

/// Finds the content tree.
///
/// A project file names it outright; without one, climb for a directory that
/// looks like a content root. A fresh clone therefore needs no setup, and the
/// project file is how you overrule the guess. Whichever answer is taken is
/// logged, because "which content did it load" is a question that comes up.
std::optional<std::filesystem::path> find_content_root() {
    namespace fs = std::filesystem;
    std::error_code code;

    fs::path directory = fs::current_path(code);
    for (int depth = 0; depth < 8 && !directory.empty(); ++depth) {
        for (const fs::directory_entry& entry :
             fs::directory_iterator(directory, code)) {
            if (entry.path().extension() == ".kproj") {
                auto document = kero::kv::parse_file(entry.path().string());
                if (document) {
                    if (const kero::kv::Block* project = document->first("project")) {
                        const fs::path content =
                            directory / std::string(project->get("content", "content"));
                        if (fs::is_directory(content, code)) {
                            KERO_INFO(host_log, "content tree from {}: {}",
                                      entry.path().string(), content.string());
                            return content;
                        }
                    }
                }
            }
        }

        const fs::path content = directory / "content";
        if (fs::is_directory(content / "maps", code)) {
            KERO_INFO(host_log, "content tree found by climbing: {}", content.string());
            return content;
        }

        if (!directory.has_parent_path() || directory.parent_path() == directory) {
            break;
        }
        directory = directory.parent_path();
    }
    return std::nullopt;
}

std::string resolve_map(const std::string& name) {
    namespace fs = std::filesystem;
    // An explicit path wins over a name looked up in the content tree.
    if (name.ends_with(".kbsp") && fs::exists(name)) {
        return name;
    }
    if (const std::optional<fs::path> root = find_content_root()) {
        const fs::path compiled = *root / "maps" / (name + ".kbsp");
        if (fs::exists(compiled)) {
            return compiled.string();
        }
        const fs::path source = *root / "maps" / (name + ".kmap");
        if (fs::exists(source)) {
            KERO_ERROR(host_log,
                       "{} has never been compiled. Run:\n"
                       "    kerosene-tools cleave {}\n"
                       "    kerosene-tools umbra {}",
                       name, source.string(),
                       (*root / "maps" / (name + ".kbsp")).string());
            return {};
        }
    }
    return name;
}

#if KEROSENE_HAS_RENDER

/// The windowed loop.
///
/// Note how little there is to it: poll, hand the input to the Host, draw what
/// the Host says the view is. The renderer never touches the simulation, and
/// the simulation does not know the renderer exists. That is the arrangement
/// that makes the headless path above the same program rather than a
/// second one.
int run_windowed(kero::engine::Host& host, kero::i32 width, kero::i32 height,
                 kero::i64 frame_limit) {
    auto renderer = kero::render::Renderer::create("Kerosene", width, height);
    if (!renderer) {
        std::fprintf(stderr, "%s\n", renderer.error().c_str());
        return 1;
    }
    if (auto uploaded = (*renderer)->set_level(host.level()); !uploaded) {
        std::fprintf(stderr, "%s\n", uploaded.error().c_str());
        return 1;
    }

    kero::math::Angles view = kero::math::Angles(0.0f, 0.0f, 0.0f);
    auto previous = std::chrono::steady_clock::now();
    kero::i64 frames = 0;

    while (!host.quitting()) {
        if (frame_limit >= 0 && frames >= frame_limit) {
            break;
        }
        ++frames;
        const kero::render::Input input = (*renderer)->poll();
        if (input.quit) {
            break;
        }

        view.yaw = kero::math::normalize_angle(view.yaw + input.look_yaw);
        // Pitch is clamped rather than wrapped: rolling over the top is
        // disorienting and every game that allows it regrets it. Positive is
        // downward, which is the Quake convention this engine keeps.
        view.pitch = std::clamp(view.pitch + input.look_pitch, -89.0f, 89.0f);

        kero::engine::Command command;
        command.move.forward = input.forward;
        command.move.side = input.side;
        command.move.jump = input.jump;
        command.move.duck = input.duck;
        command.move.view = view;

        const auto now = std::chrono::steady_clock::now();
        const auto elapsed = std::chrono::duration<float>(now - previous).count();
        previous = now;

        host.run_frame(command, elapsed);

        const kero::engine::ViewState state = host.view();
        (*renderer)->draw(host.level(), state.eye, state.angles, state.cluster);
    }

    KERO_INFO(host_log, "{} frames, {} ticks", frames, host.tick_count());

    const kero::render::Stats& stats = (*renderer)->stats();
    KERO_INFO(host_log, "last frame: {} leaves, {} faces, {} draws, {} triangles",
              stats.leaves_visible, stats.faces_drawn, stats.draw_calls,
              stats.triangles);
    return 0;
}

#endif  // KEROSENE_HAS_RENDER

}  // namespace

int main(int argc, char** argv) {
    std::vector<std::string> arguments(argv + 1, argv + argc);

    bool headless = false;
    kero::i64 headless_ticks = -1;
    kero::i32 width = 1280;
    kero::i32 height = 720;
    kero::i64 frame_limit = -1;
    std::string map;

    for (kero::usize i = 0; i < arguments.size(); ++i) {
        const std::string& argument = arguments[i];
        if (argument == "-h" || argument == "--help") {
            print_usage();
            return 0;
        }
        if (argument == "--headless") {
            headless = true;
            if (i + 1 < arguments.size() && !arguments[i + 1].starts_with('+') &&
                !arguments[i + 1].starts_with('-')) {
                headless_ticks = std::stoll(arguments[++i]);
            }
            continue;
        }
        if (argument == "--map" && i + 1 < arguments.size()) {
            map = arguments[++i];
            continue;
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
    }

    kero::engine::Host host;

    // `map` is a console command so that the same word works from a launch
    // argument, from a config file and from the console at runtime.
    const kero::console::ConCommand map_command(
        "map", "map <name> -- load a level.",
        [&host](std::span<const std::string> command_arguments) {
            if (command_arguments.empty()) {
                kero::console::print("map <name>\n");
                return;
            }
            const std::string path = resolve_map(command_arguments.front());
            if (path.empty()) {
                return;
            }
            if (auto loaded = host.load_map(path); !loaded) {
                kero::console::print(loaded.error() + "\n");
            }
        });

    const kero::console::ConCommand quit_command(
        "quit", "Leave.", [&host](std::span<const std::string>) { host.request_quit(); });

    kero::console::execute_command_line(arguments);
    if (!map.empty()) {
        kero::console::enqueue("map " + map);
    }
    kero::console::flush();

    if (!host.has_map()) {
        // A startmap in the project file is why the sample level needs no +map.
        if (const std::optional<std::filesystem::path> root = find_content_root()) {
            for (const std::filesystem::directory_entry& entry :
                 std::filesystem::directory_iterator(root->parent_path())) {
                if (entry.path().extension() != ".kproj") {
                    continue;
                }
                auto document = kero::kv::parse_file(entry.path().string());
                if (!document) {
                    continue;
                }
                if (const kero::kv::Block* project = document->first("project")) {
                    if (const std::string_view start = project->get("startmap");
                        !start.empty()) {
                        kero::console::enqueue("map " + std::string(start));
                        kero::console::flush();
                    }
                }
            }
        }
    }

    if (!host.has_map()) {
        std::fputs("no map loaded. Try: kerosene +map kero_start\n", stderr);
        return 1;
    }

    if (!headless) {
#if KEROSENE_HAS_RENDER
        return run_windowed(host, width, height, frame_limit);
#else
        // Accepted and ignored in a build with no renderer, so a command line
        // that works on one machine is not rejected on another.
        (void)width;
        (void)height;
        (void)frame_limit;
        std::fputs(
            "This build has no renderer -- run it with --headless, or build with\n"
            "KEROSENE_BUILD_RENDER=ON. The simulation is the same either way;\n"
            "only the display is missing.\n",
            stderr);
        return 1;
#endif
    }

    KERO_INFO(host_log, "headless: {} ticks at {:.0f} Hz",
              headless_ticks < 0 ? std::string("unbounded")
                                 : std::to_string(headless_ticks),
              static_cast<double>(1.0f / kero::engine::Host::tick_interval()));

    const kero::engine::Command idle;
    const auto interval = std::chrono::duration<double>(
        static_cast<double>(kero::engine::Host::tick_interval()));

    auto next = std::chrono::steady_clock::now();
    while (!host.quitting()) {
        kero::console::flush();
        host.tick(idle);

        if (headless_ticks >= 0 &&
            static_cast<kero::i64>(host.tick_count()) >= headless_ticks) {
            break;
        }

        // A dedicated server should not spin a core. Sleeping to the next tick
        // boundary rather than for a fixed interval keeps the rate honest when
        // a tick runs long.
        next += std::chrono::duration_cast<std::chrono::steady_clock::duration>(interval);
        std::this_thread::sleep_until(next);
    }

    const kero::engine::ViewState view = host.view();
    std::fprintf(stdout,
                 "\n"
                 "  ticks     %llu\n"
                 "  time      %.2f s\n"
                 "  player    (%.1f %.1f %.1f)\n"
                 "  cluster   %d\n"
                 "  entities  %zu\n"
                 "  inputs    %llu delivered\n",
                 static_cast<unsigned long long>(host.tick_count()),
                 static_cast<double>(host.time()),
                 static_cast<double>(host.player().origin.x),
                 static_cast<double>(host.player().origin.y),
                 static_cast<double>(host.player().origin.z), view.cluster,
                 host.entities().size(),
                 static_cast<unsigned long long>(host.entities().delivered()));
    return 0;
}
