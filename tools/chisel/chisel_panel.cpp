// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "chisel/chisel_panel.hpp"

#include "chisel/document.hpp"
#include "chisel/viewport.hpp"
#include "chisel/viewport_render.hpp"
#include "common/project.hpp"
#include "core/log.hpp"
#include "map/map.hpp"
#include "math/units.hpp"

#include <imgui.h>
#include <imgui_internal.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <string>
#include <vector>

namespace kero::chisel {
namespace {

KERO_LOG_CATEGORY(log, "chisel");

/// The four panes, in the order they are created and drawn.
constexpr std::array<ViewKind, 4> kViews{ViewKind::Top, ViewKind::Front, ViewKind::Side,
                                         ViewKind::Perspective};

/// Grid spacings offered, in kerosene units. Four ku -- eight inches -- is the
/// default because it is the coarsest grid a doorway still lands on.
constexpr std::array<f32, 7> kGridSteps{1.0f, 2.0f, 4.0f, 8.0f, 16.0f, 32.0f, 64.0f};

constexpr f32 kOrbitDegreesPerPixel = 0.25f;
constexpr f32 kFlySpeed = 320.0f;  ///< ku per second, about twice a run.

/// Whether a viewport's own texture is what ImGui should show.
[[nodiscard]] ImTextureID texture_id(SDL_GPUTexture* texture) {
    return reinterpret_cast<ImTextureID>(texture);
}

}  // namespace

struct ChiselPanel::Impl {
    Document document;
    bool loaded = false;
    std::string status;

    std::optional<tools::Project> project;
    bool looked_for_project = false;

    std::array<Viewport, kViews.size()> views;
    /// Last frame's picture for each pane.
    ///
    /// The shell draws the UI before it records a panel's GPU work, so the
    /// texture handed to `ImGui::Image` is the one filled in the previous
    /// frame. That is one frame of latency on a viewport, which nobody can see,
    /// and the alternative -- rendering before knowing how much room ImGui gave
    /// the pane -- is a viewport that is the wrong size whenever a splitter
    /// moves.
    std::array<SDL_GPUTexture*, kViews.size()> textures{};

    std::unique_ptr<ViewportRenderer> renderer;
    std::string renderer_error;

    f32 grid = 4.0f;
    bool wireframe_grid = true;
    bool laid_out = false;
    bool reported = false;

    Impl() {
        for (usize i = 0; i < kViews.size(); ++i) {
            views[i].kind = kViews[i];
        }
    }

    /// The pane's pixel rectangle, and the cursor within it.
    struct Pane {
        bool hovered = false;
        Vec2 cursor;
        Vec2 delta;
        f32 wheel = 0.0f;
    };

    void lay_out(u32 dockspace);
    void handle_input(Viewport& view, const Pane& pane);
    void pick_at(Viewport& view, Vec2 cursor, bool additive);
    void frame_all();
};

ChiselPanel::ChiselPanel() : impl_(std::make_unique<Impl>()) { impl_->frame_all(); }
ChiselPanel::~ChiselPanel() = default;

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

void ChiselPanel::Impl::lay_out(u32 dockspace) {
    laid_out = true;
    if (ImGui::DockBuilderGetNode(dockspace) != nullptr &&
        ImGui::DockBuilderGetNode(dockspace)->IsSplitNode()) {
        return;  // The user's own arrangement, restored from the ini file.
    }

    ImGui::DockBuilderRemoveNode(dockspace);
    ImGui::DockBuilderAddNode(dockspace, ImGuiDockNodeFlags_DockSpace);
    ImGui::DockBuilderSetNodeSize(dockspace, ImGui::GetMainViewport()->WorkSize);

    // A column of controls on the left, and the classic four panes filling the
    // rest: top and 3D above, front and side below.
    ImGuiID rest = dockspace;
    const ImGuiID controls = ImGui::DockBuilderSplitNode(rest, ImGuiDir_Left, 0.22f,
                                                         nullptr, &rest);
    ImGuiID lower = 0;
    const ImGuiID upper = ImGui::DockBuilderSplitNode(rest, ImGuiDir_Up, 0.5f, nullptr,
                                                      &lower);
    ImGuiID upper_right = 0;
    const ImGuiID upper_left = ImGui::DockBuilderSplitNode(upper, ImGuiDir_Left, 0.5f,
                                                           nullptr, &upper_right);
    ImGuiID lower_right = 0;
    const ImGuiID lower_left = ImGui::DockBuilderSplitNode(lower, ImGuiDir_Left, 0.5f,
                                                           nullptr, &lower_right);

    ImGui::DockBuilderDockWindow("Map", controls);
    ImGui::DockBuilderDockWindow(std::string(name_of(ViewKind::Top)).c_str(), upper_left);
    ImGui::DockBuilderDockWindow(std::string(name_of(ViewKind::Perspective)).c_str(),
                                 upper_right);
    ImGui::DockBuilderDockWindow(std::string(name_of(ViewKind::Front)).c_str(), lower_left);
    ImGui::DockBuilderDockWindow(std::string(name_of(ViewKind::Side)).c_str(), lower_right);
    ImGui::DockBuilderFinish(dockspace);
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

void ChiselPanel::Impl::handle_input(Viewport& view, const Pane& pane) {
    if (!pane.hovered) {
        return;
    }

    const ImGuiIO& io = ImGui::GetIO();
    const bool panning = ImGui::IsMouseDown(ImGuiMouseButton_Middle);

    if (is_orthographic(view.kind)) {
        if (pane.wheel != 0.0f) {
            view.zoom_at(pane.cursor, std::pow(1.15f, pane.wheel));
        }
        if (panning || ImGui::IsMouseDown(ImGuiMouseButton_Right)) {
            view.pan(pane.delta);
        }
        return;
    }

    // The 3D view: right-drag looks, the wheel dollies, and WASD flies while
    // the pointer is over the pane. Holding a button to look is what keeps the
    // cursor usable for everything else.
    if (ImGui::IsMouseDown(ImGuiMouseButton_Right)) {
        view.angles.yaw -= pane.delta.x * kOrbitDegreesPerPixel;
        view.angles.pitch += pane.delta.y * kOrbitDegreesPerPixel;
        view.angles.pitch = std::clamp(view.angles.pitch, -89.0f, 89.0f);
        view.angles.yaw = math::normalize_angle(view.angles.yaw);
    }

    Vec3 forward;
    Vec3 right;
    Vec3 up;
    math::angle_vectors(view.angles, &forward, &right, &up);

    Vec3 wish;
    if (ImGui::IsKeyDown(ImGuiKey_W)) {
        wish += forward;
    }
    if (ImGui::IsKeyDown(ImGuiKey_S)) {
        wish -= forward;
    }
    if (ImGui::IsKeyDown(ImGuiKey_D)) {
        wish += right;
    }
    if (ImGui::IsKeyDown(ImGuiKey_A)) {
        wish -= right;
    }
    if (ImGui::IsKeyDown(ImGuiKey_E)) {
        wish += Vec3{0.0f, 0.0f, 1.0f};
    }
    if (ImGui::IsKeyDown(ImGuiKey_Q)) {
        wish -= Vec3{0.0f, 0.0f, 1.0f};
    }

    const f32 speed = kFlySpeed * (ImGui::IsKeyDown(ImGuiKey_LeftShift) ? 3.0f : 1.0f);
    if (!wish.is_zero()) {
        view.eye += wish.normalized() * (speed * io.DeltaTime);
    }
    if (pane.wheel != 0.0f) {
        view.eye += forward * (pane.wheel * 32.0f);
    }
}

void ChiselPanel::Impl::pick_at(Viewport& view, Vec2 cursor, bool additive) {
    const Ray ray = view.ray_from_screen(cursor);
    Selection& selection = document.selection();

    // Entities first: a point entity sitting inside a brush is one you would
    // otherwise never be able to click.
    const f64 radius = static_cast<f64>(view.units_per_pixel()) * 8.0;
    if (const std::optional<i32> entity = pick_entity(document, ray, radius)) {
        if (!additive) {
            selection.clear();
        }
        selection.toggle_entity(*entity);
        return;
    }

    const Hit hit = pick(document, ray);
    if (!hit) {
        if (!additive) {
            selection.clear();
        }
        return;
    }

    if (!additive) {
        selection.clear();
    }
    selection.toggle_solid(hit.solid);
    selection.face = hit.face;
}

void ChiselPanel::Impl::frame_all() {
    math::Aabb bounds;
    for (const Document::SolidRef& ref : document.all_solids()) {
        const math::Aabbd solid = map::bounds_of(*ref.solid);
        if (solid.empty()) {
            continue;
        }
        bounds.add(Vec3(static_cast<f32>(solid.mins.x), static_cast<f32>(solid.mins.y),
                        static_cast<f32>(solid.mins.z)));
        bounds.add(Vec3(static_cast<f32>(solid.maxs.x), static_cast<f32>(solid.maxs.y),
                        static_cast<f32>(solid.maxs.z)));
    }
    if (bounds.empty()) {
        // An empty document still has to look at somewhere. A box around the
        // origin is the same view a new map opens on.
        bounds = math::Aabb(Vec3(-256.0f, -256.0f, -256.0f), Vec3(256.0f, 256.0f, 256.0f));
    }
    for (Viewport& view : views) {
        view.frame(bounds);
    }
}

// ---------------------------------------------------------------------------
// The panel
// ---------------------------------------------------------------------------

void ChiselPanel::open(const std::string& path) {
    Impl& impl = *impl_;

    std::string error;
    if (!impl.document.open(path, error)) {
        // The diagnostic already carries a file, line and column; passing it
        // through unchanged is more useful than wrapping it in a sentence.
        impl.status = error;
        KERO_ERROR(log, "{}", impl.status);
        return;
    }

    impl.loaded = true;
    impl.status = std::format("{} brushes, {} entities", impl.document.map().brush_count(),
                              impl.document.map().entities.size() + 1);
    impl.frame_all();
    KERO_INFO(log, "opened {} -- {}", path, impl.status);
}

bool ChiselPanel::can_close() { return !impl_->document.dirty(); }

void ChiselPanel::draw_file_menu(shell::Shell&) {
    Impl& impl = *impl_;

    if (!impl.looked_for_project) {
        impl.project = tools::find_project();
        impl.looked_for_project = true;
    }

    if (ImGui::BeginMenu("Open map")) {
        if (impl.project) {
            const std::vector<std::filesystem::path> maps = impl.project->maps();
            if (maps.empty()) {
                ImGui::TextDisabled("no maps in this project");
            }
            for (const std::filesystem::path& path : maps) {
                if (ImGui::MenuItem(path.filename().string().c_str())) {
                    open(path.string());
                }
            }
        } else {
            ImGui::TextDisabled("no content tree found");
        }
        ImGui::EndMenu();
    }

    ImGui::Separator();
    if (ImGui::MenuItem("Save", "Ctrl+S", false, impl.document.has_path())) {
        std::string error;
        if (!impl.document.save(error)) {
            impl.status = error;
            KERO_ERROR(log, "{}", error);
        } else {
            impl.status = std::format("saved {}", impl.document.path());
        }
    }
}

void ChiselPanel::draw(shell::Shell& shell) {
    Impl& impl = *impl_;

    if (!impl.laid_out) {
        impl.lay_out(shell.dockspace());
    }

    if (ImGui::Begin("Map")) {
        if (!impl.loaded) {
            ImGui::TextWrapped("No map open. File -> Open map.");
        } else {
            ImGui::TextUnformatted(impl.document.path().c_str());
        }
        if (!impl.status.empty()) {
            ImGui::TextDisabled("%s", impl.status.c_str());
        }
        if (!impl.renderer_error.empty()) {
            ImGui::TextColored(ImVec4(1.0f, 0.42f, 0.38f, 1.0f), "%s",
                               impl.renderer_error.c_str());
        }

        ImGui::Separator();

        // The grid is shown in both units, because a level is authored in ku
        // and argued about in inches.
        if (ImGui::BeginCombo("Grid", std::format("{:g} ku", impl.grid).c_str())) {
            for (const f32 step : kGridSteps) {
                const bool chosen = std::abs(step - impl.grid) < 0.001f;
                if (ImGui::Selectable(
                            std::format("{:g} ku ({:g}\")", step, step / units::kPerInch)
                            .c_str(),
                        chosen)) {
                    impl.grid = step;
                }
            }
            ImGui::EndCombo();
        }
        if (ImGui::Button("Frame all")) {
            impl.frame_all();
        }

        ImGui::Separator();
        ImGui::Text("selection: %zu", impl.document.selection().size());
        if (impl.renderer) {
            const ViewportRenderer::Stats& stats = impl.renderer->stats();
            ImGui::TextDisabled("%zu triangles, %zu lines", stats.triangles, stats.lines);
        }
        ImGui::TextDisabled("undo: %zu", impl.document.history_size());
    }
    ImGui::End();

    for (usize i = 0; i < kViews.size(); ++i) {
        Viewport& view = impl.views[i];
        if (!ImGui::Begin(std::string(name_of(view.kind)).c_str())) {
            ImGui::End();
            continue;
        }

        const ImVec2 available = ImGui::GetContentRegionAvail();
        view.width = std::max(available.x, 1.0f);
        view.height = std::max(available.y, 1.0f);

        const ImVec2 origin = ImGui::GetCursorScreenPos();
        if (impl.textures[i] != nullptr) {
            ImGui::Image(texture_id(impl.textures[i]), ImVec2(view.width, view.height));
        } else {
            ImGui::Dummy(ImVec2(view.width, view.height));
        }

        Impl::Pane pane;
        pane.hovered = ImGui::IsItemHovered();
        const ImGuiIO& io = ImGui::GetIO();
        pane.cursor = Vec2{io.MousePos.x - origin.x, io.MousePos.y - origin.y};
        pane.delta = Vec2{io.MouseDelta.x, io.MouseDelta.y};
        pane.wheel = io.MouseWheel;

        if (pane.hovered && ImGui::IsMouseClicked(ImGuiMouseButton_Left)) {
            ImGui::SetKeyboardFocusHere(-1);
            impl.pick_at(view, pane.cursor, io.KeyCtrl);
        }
        impl.handle_input(view, pane);

        ImGui::End();
    }

    if (impl.loaded) {
        shell.set_status(std::format("{}{} -- {}", impl.document.path(),
                                     impl.document.dirty() ? " *" : "", impl.status));
    }
}

void ChiselPanel::render(shell::Shell& shell, SDL_GPUCommandBuffer* command) {
    Impl& impl = *impl_;

    if (impl.renderer == nullptr) {
        if (!impl.renderer_error.empty()) {
            return;  // Reported once; retrying every frame would only spam.
        }
        auto created = ViewportRenderer::create(shell.device());
        if (!created) {
            impl.renderer_error = created.error();
            KERO_ERROR(log, "viewport renderer: {}", impl.renderer_error);
            return;
        }
        impl.renderer = std::move(*created);
    }

    std::vector<ViewportRenderer::Line> overlay;
    for (usize i = 0; i < kViews.size(); ++i) {
        overlay.clear();
        if (impl.wireframe_grid) {
            overlay = grid_lines(impl.views[i], impl.grid);
        }
        impl.textures[i] =
            impl.renderer->draw(command, i, impl.views[i], impl.document, overlay);
    }

    // Said once, when there is finally something to say. This is what makes
    // `--frames` a check rather than a screensaver: a run that draws nothing
    // has a log that says so.
    if (!impl.reported && impl.renderer->stats().lines > 0) {
        impl.reported = true;
        KERO_INFO(log, "viewports drawing {} triangles, {} lines",
                  impl.renderer->stats().triangles, impl.renderer->stats().lines);
    }
}

}  // namespace kero::chisel
