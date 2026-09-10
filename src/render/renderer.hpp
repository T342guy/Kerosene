// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "bsp/level.hpp"
#include "math/angles.hpp"
#include "math/mat.hpp"
#include "math/vec.hpp"

#include <expected>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

/// The renderer.
///
/// SDL3's GPU API rather than Vulkan directly. It is a modern explicit API --
/// command buffers, render passes, binary shaders, no hidden state -- targeting
/// Vulkan, D3D12 and Metal from one backend. Source needed a platform layer
/// plus a `shaderapi` DLL per graphics API and a shader compiler for each; this
/// is one dependency and one set of SPIR-V.
///
/// Nothing in the engine links this library. The simulation runs identically
/// without it, which is what makes `--headless` the dedicated server rather
/// than a mode, and the build fails if that ever stops being true.
namespace kero::render {

using math::Angles;
using math::Vec3;

/// What the player did, translated out of SDL so nothing above here includes it.
struct Input {
    f32 forward = 0.0f;
    f32 side = 0.0f;
    bool jump = false;
    bool duck = false;
    bool quit = false;

    /// Mouse movement since the last poll, in degrees.
    f32 look_yaw = 0.0f;
    f32 look_pitch = 0.0f;

    /// Console text typed this frame, and whether the console was toggled.
    bool toggle_console = false;
    std::string typed;
};

/// What the last frame cost, for the overlay and for `r_speeds`.
struct Stats {
    usize leaves_visible = 0;
    usize faces_drawn = 0;
    usize draw_calls = 0;
    usize triangles = 0;
};

class Renderer {
public:
    [[nodiscard]] static std::expected<std::unique_ptr<Renderer>, std::string> create(
        std::string_view title, i32 width, i32 height);

    ~Renderer();
    Renderer(const Renderer&) = delete;
    Renderer& operator=(const Renderer&) = delete;

    /// Uploads a level's geometry. Once per level, not per frame.
    [[nodiscard]] std::expected<void, std::string> set_level(const bsp::Level& level);

    /// Drains SDL's event queue.
    [[nodiscard]] Input poll();

    /// Draws one frame.
    void draw(const bsp::Level& level, const Vec3& eye, const Angles& angles, i32 cluster);

    [[nodiscard]] const Stats& stats() const { return stats_; }

    /// Whether the mouse is captured for looking. Released so the window can be
    /// left without a wrestle -- a game that traps the pointer with no way out
    /// is a game people force-quit.
    void set_mouse_captured(bool captured);
    [[nodiscard]] bool mouse_captured() const { return mouse_captured_; }

private:
    struct Impl;
    explicit Renderer(std::unique_ptr<Impl> impl);

    std::unique_ptr<Impl> impl_;
    Stats stats_;
    bool mouse_captured_ = true;
};

}  // namespace kero::render
