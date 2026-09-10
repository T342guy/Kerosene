// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "chisel/document.hpp"
#include "chisel/viewport.hpp"

#include <expected>
#include <memory>
#include <string>
#include <vector>

struct SDL_GPUDevice;
struct SDL_GPUCommandBuffer;
struct SDL_GPUTexture;

namespace kero::chisel {

/// Draws editable geometry.
///
/// Not `kerosene::render`, and deliberately so. The engine renderer draws a
/// compiled `bsp::Level`: static, welded, culled against a PVS computed hours
/// ago. This draws brushes that are changing under the cursor, in wireframe and
/// in solid, with the selected one picked out and drag handles on top. Sharing
/// one renderer between those two jobs would make both worse; what they share
/// is the shader build step and the developer textures, so a surface looks the
/// same in the editor as it will in the game.
class ViewportRenderer {
public:
    [[nodiscard]] static std::expected<std::unique_ptr<ViewportRenderer>, std::string>
    create(SDL_GPUDevice* device);

    ~ViewportRenderer();
    ViewportRenderer(const ViewportRenderer&) = delete;
    ViewportRenderer& operator=(const ViewportRenderer&) = delete;

    /// An overlay line, in world space. Grid, handles, the leak path.
    struct Line {
        Vec3 from;
        Vec3 to;
        u32 colour = 0xFFFFFFFFu;  ///< RGBA, low byte red.
    };

    /// Renders one viewport into its own texture, and returns it for the UI to
    /// show. Null when the size is degenerate.
    ///
    /// `overlay` is drawn on top with no depth test -- a handle you cannot see
    /// because it is inside a wall is a handle you cannot use.
    [[nodiscard]] SDL_GPUTexture* draw(SDL_GPUCommandBuffer* command, usize index,
                                       const Viewport& view, const Document& document,
                                       const std::vector<Line>& overlay);

    /// How many triangles and lines the last frame drew, for the status bar.
    struct Stats {
        usize triangles = 0;
        usize lines = 0;
    };
    [[nodiscard]] const Stats& stats() const { return stats_; }

private:
    struct Impl;
    explicit ViewportRenderer(std::unique_ptr<Impl> impl);

    std::unique_ptr<Impl> impl_;
    Stats stats_;
};

/// The grid lines visible in an orthographic view, at a sensible density.
///
/// Returned rather than drawn so the caller decides what else goes on the
/// overlay, and so this is testable without a GPU.
[[nodiscard]] std::vector<ViewportRenderer::Line> grid_lines(const Viewport& view,
                                                             f32 spacing);

/// The edges of a brush's faces, for a wireframe.
void append_solid_edges(std::vector<ViewportRenderer::Line>& lines, const map::Solid& solid,
                        u32 colour);

}  // namespace kero::chisel
