// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <expected>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

struct SDL_Window;
struct SDL_GPUDevice;
struct SDL_GPUCommandBuffer;

/// The toolset window: one application holding every tool, switched with a rail
/// down the left edge.
///
/// One window rather than one binary per tool, because installing eight
/// programs to build one level is a worse deal than clicking an icon. The tools
/// themselves stay separate -- each is a Panel that knows nothing about the
/// others -- so the shell is a place to put them rather than something they
/// have to be written against.
namespace kero::shell {

class Shell;

/// One tool on the rail.
///
/// The two halves are deliberately separate. `draw` builds this frame's UI and
/// touches no GPU state; `render` records GPU work and builds no UI. A panel
/// that wants a 3D viewport creates its texture in `draw` -- where it can ask
/// ImGui how much room it has -- and fills it in `render`, which the shell
/// calls before the UI reaches the screen, so the picture is this frame's
/// rather than last frame's.
class Panel {
public:
    virtual ~Panel() = default;

    /// What the rail calls it.
    [[nodiscard]] virtual std::string_view name() const = 0;
    /// One line, shown as a tooltip.
    [[nodiscard]] virtual std::string_view summary() const = 0;

    virtual void draw(Shell& shell) = 0;

    virtual void render(Shell& shell, SDL_GPUCommandBuffer* command) {
        (void)shell;
        (void)command;
    }

    /// Whether the window may close. A panel with unsaved work says no and puts
    /// its own dialog up.
    [[nodiscard]] virtual bool can_close() { return true; }

    /// Menu items this panel contributes to the File menu. Drawn inside an
    /// already-open menu.
    virtual void draw_file_menu(Shell& shell) { (void)shell; }
};

class Shell {
public:
    [[nodiscard]] static std::expected<std::unique_ptr<Shell>, std::string> create(
        std::string_view title, i32 width, i32 height);

    ~Shell();
    Shell(const Shell&) = delete;
    Shell& operator=(const Shell&) = delete;

    void add_panel(std::unique_ptr<Panel> panel);

    /// Runs until the window closes. `frame_limit` of -1 means unbounded; a
    /// positive value draws that many frames and returns, which is how this can
    /// be smoke-tested without hijacking whoever's desktop it is.
    [[nodiscard]] int run(i64 frame_limit);

    [[nodiscard]] SDL_GPUDevice* device() const;
    [[nodiscard]] SDL_Window* window() const;

    /// The swapchain's format, for a panel building its own pipelines.
    [[nodiscard]] u32 colour_format() const;

    void request_quit() { quitting_ = true; }
    [[nodiscard]] bool quitting() const { return quitting_; }

    /// A line for the status bar along the bottom. Cleared each frame, so a
    /// panel simply sets it while drawing.
    void set_status(std::string text) { status_ = std::move(text); }

private:
    struct Impl;
    explicit Shell(std::unique_ptr<Impl> impl);

    void draw_rail();
    void draw_menu_bar();

    std::unique_ptr<Impl> impl_;
    std::vector<std::unique_ptr<Panel>> panels_;
    usize active_ = 0;
    bool quitting_ = false;
    std::string status_;
};

}  // namespace kero::shell
