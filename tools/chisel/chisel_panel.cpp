// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "chisel/chisel_panel.hpp"

#include "chisel/document.hpp"
#include "chisel/tools.hpp"
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

/// How close, in pixels, the cursor has to be to take hold of a resize grip.
constexpr f32 kGripPixels = 7.0f;

/// The colours the drag preview is drawn in. Nothing is committed to the map
/// until the button comes up, so what moves under the cursor is an overlay.
constexpr u32 kPreviewColour = 0xFF60D0FFu;
constexpr u32 kGripColour = 0xFFFFC050u;

/// The point-entity classes the Entity tool offers.
///
/// A hard-coded list rather than a scan of the game's classes, because there is
/// no class registry to scan yet -- `kerosene::entity` registers its classes in
/// the engine process. When there is one, this becomes a query.
constexpr std::array<const char*, 5> kEntityClasses{
    "info_player_start", "light", "logic_relay", "func_detail", "prop_static"};
constexpr f32 kFlySpeed = 320.0f;  ///< ku per second, about twice a run.

/// Which tool the mouse is holding.
enum class ToolKind : u8 {
    Select,  ///< Pick, move, and resize by the grips on the selection.
    Block,   ///< Drag out a box.
    Entity,  ///< Place a point entity.
};

[[nodiscard]] const char* name_of(ToolKind tool) {
    switch (tool) {
        case ToolKind::Select: return "Select";
        case ToolKind::Block: return "Block";
        case ToolKind::Entity: return "Entity";
    }
    return "?";
}

/// The twelve edges of a box, for the Block tool's preview.
///
/// Drawn rather than turned into a brush: a brush would want ids from the
/// document's pool, and a drag that allocated ids every frame would burn
/// through them by the time the button came up.
void append_box_edges(std::vector<ViewportRenderer::Line>& lines,
                      const math::Aabbd& bounds, u32 colour) {
    std::array<Vec3, 8> corner{};
    for (usize i = 0; i < corner.size(); ++i) {
        const math::Vec3d point((i & 1u) != 0 ? bounds.maxs.x : bounds.mins.x,
                                (i & 2u) != 0 ? bounds.maxs.y : bounds.mins.y,
                                (i & 4u) != 0 ? bounds.maxs.z : bounds.mins.z);
        corner[i] = Vec3(static_cast<f32>(point.x), static_cast<f32>(point.y),
                         static_cast<f32>(point.z));
    }
    // Every pair of corners differing in exactly one bit is an edge.
    for (usize i = 0; i < corner.size(); ++i) {
        for (const usize bit : {1u, 2u, 4u}) {
            if ((i & bit) == 0) {
                lines.push_back(ViewportRenderer::Line{corner[i], corner[i | bit], colour});
            }
        }
    }
}

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

    ToolKind tool = ToolKind::Select;
    usize entity_class = 0;
    f64 block_depth = 64.0;
    std::string block_material = "dev/grid";

    /// One brush as it was when the drag started, and who owns it.
    struct Held {
        i32 owner = 0;
        map::Solid before;
    };

    /// A drag in progress.
    ///
    /// Nothing reaches the map until the button comes up. A drag that edited as
    /// it went would push a hundred steps onto the undo stack for one gesture,
    /// and rebuild the viewport mesh on every one of them; what moves under the
    /// cursor is an overlay, and the edit is the difference between where the
    /// brushes were and where they were let go.
    struct Drag {
        bool active = false;
        ToolKind tool = ToolKind::Select;
        usize view = 0;
        Vec3d start;
        Vec3d current;
        bool resizing = false;
        Grip grip;
        math::Aabbd original;
        std::vector<Held> held;
        /// Where the held brushes would end up. Drawn, not applied.
        std::vector<map::Solid> preview;
        /// The Block tool's box, which has no brush yet to preview.
        math::Aabbd block;
    };
    Drag drag;
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
    [[nodiscard]] Vec3d world_at(const Viewport& view, Vec2 cursor) const;
    [[nodiscard]] Vec3d drag_delta(const Viewport& view) const;
    void begin_drag(usize index, Vec2 cursor, bool additive);
    void update_drag(usize index, Vec2 cursor);
    void end_drag();
    void update_preview();
    void delete_selection();
    void place_entity(const Viewport& view, Vec2 cursor);
    [[nodiscard]] std::vector<ViewportRenderer::Line> overlay_for(usize index) const;
    void handle_input(Viewport& view, const Pane& pane);
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
// Dragging
// ---------------------------------------------------------------------------

Vec3d ChiselPanel::Impl::world_at(const Viewport& view, Vec2 cursor) const {
    const Vec3 world = view.world_from_screen(cursor);
    return Vec3d(static_cast<f64>(world.x), static_cast<f64>(world.y),
                 static_cast<f64>(world.z));
}

Vec3d ChiselPanel::Impl::drag_delta(const Viewport& view) const {
    Vec3d delta = drag.current - drag.start;

    // Nothing moves along the axis the view looks down. An orthographic view
    // cannot show that axis, so a drag that changed it would move brushes in a
    // direction the user cannot see -- which is how a wall ends up a hundred
    // units behind the room it belongs to.
    const ViewAxes axes = view.axes();
    const Vec3d forward(static_cast<f64>(axes.forward.x), static_cast<f64>(axes.forward.y),
                        static_cast<f64>(axes.forward.z));
    delta -= forward * dot(delta, forward);

    return snap_to_grid(delta, static_cast<f64>(grid));
}

void ChiselPanel::Impl::begin_drag(usize index, Vec2 cursor, bool additive) {
    Viewport& view = views[index];
    const Vec3d world = world_at(view, cursor);

    drag = Drag{};
    drag.tool = tool;
    drag.view = index;
    drag.start = world;
    drag.current = world;

    if (tool == ToolKind::Entity) {
        place_entity(view, cursor);
        return;
    }

    if (tool == ToolKind::Block) {
        drag.active = true;
        return;
    }

    // Select. A grip on the current selection wins over picking something new:
    // the handles are drawn on top, so they have to be clickable on top.
    const math::Aabbd bounds = selection_bounds(document);
    if (!bounds.empty() && is_orthographic(view.kind)) {
        for (const Grip& grip : grips()) {
            const Vec3d position = grip_position(bounds, view.axes(), grip);
            const Vec2 screen = view.screen_from_world(
                Vec3(static_cast<f32>(position.x), static_cast<f32>(position.y),
                     static_cast<f32>(position.z)));
            if (std::abs(screen.x - cursor.x) <= kGripPixels &&
                std::abs(screen.y - cursor.y) <= kGripPixels) {
                drag.active = true;
                drag.resizing = true;
                drag.grip = grip;
                drag.original = bounds;
                break;
            }
        }
    }

    if (!drag.resizing) {
        const Ray ray = view.ray_from_screen(cursor);
        const Hit hit = pick(document, ray);
        const f64 radius = static_cast<f64>(view.units_per_pixel()) * 8.0;
        const std::optional<i32> entity = pick_entity(document, ray, radius);

        Selection& selection = document.selection();
        if (entity) {
            if (!additive) {
                selection.clear();
            }
            selection.toggle_entity(*entity);
            return;  // Point entities have no brushes to drag.
        }

        if (!hit) {
            if (!additive) {
                selection.clear();
            }
            return;
        }

        // Clicking something already selected starts a drag of the whole
        // selection rather than reducing it to the one thing clicked.
        if (!selection.contains_solid(hit.solid)) {
            if (!additive) {
                selection.clear();
            }
            selection.toggle_solid(hit.solid);
        }
        selection.face = hit.face;

        if (is_orthographic(view.kind)) {
            drag.active = true;
            drag.original = selection_bounds(document);
        }
    }

    for (const i32 id : document.selection().solids) {
        const map::Solid* solid = document.find_solid(id);
        const std::optional<i32> owner = document.owner_of(id);
        if (solid != nullptr && owner) {
            drag.held.push_back(Held{*owner, *solid});
        }
    }
    if (drag.held.empty() && drag.tool == ToolKind::Select) {
        drag.active = false;
    }
}

void ChiselPanel::Impl::update_drag(usize index, Vec2 cursor) {
    if (!drag.active || drag.view != index) {
        return;
    }
    drag.current = world_at(views[index], cursor);
    update_preview();
}

void ChiselPanel::Impl::update_preview() {
    drag.preview.clear();
    if (!drag.active) {
        return;
    }

    const Viewport& view = views[drag.view];
    const Vec3d delta = drag_delta(view);

    if (drag.tool == ToolKind::Block) {
        drag.block = block_bounds(view.axes(), drag.start, drag.current, block_depth,
                                  static_cast<f64>(grid));
        return;
    }

    if (drag.resizing) {
        const math::Aabbd to = snap_bounds(
            drag_grip(drag.original, view.axes(), drag.grip, delta), static_cast<f64>(grid));
        for (const Held& held : drag.held) {
            drag.preview.push_back(resize(held.before, drag.original, to));
        }
        return;
    }

    for (const Held& held : drag.held) {
        drag.preview.push_back(translate(held.before, delta));
    }
}

void ChiselPanel::Impl::end_drag() {
    if (!drag.active) {
        drag = Drag{};
        return;
    }

    update_preview();

    if (drag.tool == ToolKind::Block) {
        {
            const math::Aabbd bounds = drag.block;
            map::Solid box = make_box(bounds, document, block_material);
            if (box.valid() && map::encloses_volume(box)) {
                const i32 id = box.id;
                document.apply(std::make_unique<AddSolid>(document.map().world.id,
                                                          std::move(box), "Draw block"));
                document.selection().clear();
                document.selection().toggle_solid(id);
                status = std::format("drew a {:g} x {:g} x {:g} ku block",
                                     bounds.maxs.x - bounds.mins.x,
                                     bounds.maxs.y - bounds.mins.y,
                                     bounds.maxs.z - bounds.mins.z);
            }
        }
        drag = Drag{};
        return;
    }

    auto compound = std::make_unique<Compound>(drag.resizing ? "Resize" : "Move");
    for (usize i = 0; i < drag.held.size() && i < drag.preview.size(); ++i) {
        const map::Solid& before = drag.held[i].before;
        const map::Solid& after = drag.preview[i];
        if (!map::encloses_volume(after)) {
            // A resize that turned a brush inside out is a resize that does not
            // happen. Refusing the gesture is far kinder than accepting it and
            // letting the compiler explain three stages later.
            drag = Drag{};
            status = "that would have turned a brush inside out";
            return;
        }
        if (after.sides.size() == before.sides.size()) {
            bool moved = false;
            for (usize side = 0; side < after.sides.size() && !moved; ++side) {
                moved = after.sides[side].plane_points != before.sides[side].plane_points;
            }
            if (!moved) {
                continue;  // A click, not a drag.
            }
        }
        compound->add(std::make_unique<ReplaceSolid>(drag.held[i].owner, before, after,
                                                     drag.resizing ? "Resize brush"
                                                                   : "Move brush"));
    }

    if (!compound->empty()) {
        document.apply(std::move(compound));
    }
    drag = Drag{};
}

void ChiselPanel::Impl::delete_selection() {
    const Selection& selection = document.selection();
    if (selection.empty()) {
        return;
    }

    auto compound = std::make_unique<Compound>("Delete");
    for (const i32 id : selection.solids) {
        if (const std::optional<i32> owner = document.owner_of(id)) {
            compound->add(std::make_unique<RemoveSolid>(*owner, id, "Delete brush"));
        }
    }
    for (const i32 id : selection.entities) {
        compound->add(std::make_unique<RemoveEntity>(id, "Delete entity"));
    }

    if (!compound->empty()) {
        document.apply(std::move(compound));
        document.selection().clear();
        document.prune_selection();
    }
}

void ChiselPanel::Impl::place_entity(const Viewport& view, Vec2 cursor) {
    const Vec3d where = snap_to_grid(world_at(view, cursor), static_cast<f64>(grid));

    map::Entity entity;
    entity.id = document.allocate_id();
    entity.classname = kEntityClasses[entity_class];
    entity.set("classname", entity.classname);
    entity.set("origin", std::format("{:g} {:g} {:g}", where.x, where.y, where.z));

    const i32 id = entity.id;
    document.apply(std::make_unique<AddEntity>(std::move(entity),
                                               std::format("Place {}",
                                                           kEntityClasses[entity_class])));
    document.selection().clear();
    document.selection().toggle_entity(id);
    status = std::format("placed {} at {:g} {:g} {:g}", kEntityClasses[entity_class],
                         where.x, where.y, where.z);
}

std::vector<ViewportRenderer::Line> ChiselPanel::Impl::overlay_for(usize index) const {
    const Viewport& view = views[index];

    std::vector<ViewportRenderer::Line> overlay;
    if (wireframe_grid) {
        overlay = grid_lines(view, grid);
    }

    for (const map::Solid& solid : drag.preview) {
        append_solid_edges(overlay, solid, kPreviewColour);
    }
    if (drag.active && drag.tool == ToolKind::Block && !drag.block.empty()) {
        append_box_edges(overlay, drag.block, kPreviewColour);
    }

    // Grips, on the selection, in the orthographic views only. There is no
    // honest place to put a 2D handle in a perspective view.
    if (tool == ToolKind::Select && is_orthographic(view.kind) && !drag.active) {
        const math::Aabbd bounds = selection_bounds(document);
        if (!bounds.empty()) {
            const f32 size = view.units_per_pixel() * kGripPixels;
            const ViewAxes axes = view.axes();
            for (const Grip& grip : grips()) {
                const Vec3d at = grip_position(bounds, axes, grip);
                const Vec3 centre(static_cast<f32>(at.x), static_cast<f32>(at.y),
                                  static_cast<f32>(at.z));
                const Vec3 across = axes.right * size;
                const Vec3 down = axes.up * size;
                const Vec3 a = centre - across - down;
                const Vec3 b = centre + across - down;
                const Vec3 c = centre + across + down;
                const Vec3 d = centre - across + down;
                overlay.push_back(ViewportRenderer::Line{a, b, kGripColour});
                overlay.push_back(ViewportRenderer::Line{b, c, kGripColour});
                overlay.push_back(ViewportRenderer::Line{c, d, kGripColour});
                overlay.push_back(ViewportRenderer::Line{d, a, kGripColour});
            }
        }
    }

    return overlay;
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

        for (const ToolKind choice :
             {ToolKind::Select, ToolKind::Block, ToolKind::Entity}) {
            if (choice != ToolKind::Select) {
                ImGui::SameLine();
            }
            if (ImGui::RadioButton(name_of(choice), impl.tool == choice)) {
                impl.tool = choice;
            }
        }

        if (impl.tool == ToolKind::Block) {
            f32 depth = static_cast<f32>(impl.block_depth);
            if (ImGui::DragFloat("Depth", &depth, 1.0f, 1.0f, 1024.0f, "%.0f ku")) {
                impl.block_depth = static_cast<f64>(std::max(depth, 1.0f));
            }
        }
        if (impl.tool == ToolKind::Entity) {
            if (ImGui::BeginCombo("Class", kEntityClasses[impl.entity_class])) {
                for (usize i = 0; i < kEntityClasses.size(); ++i) {
                    if (ImGui::Selectable(kEntityClasses[i], impl.entity_class == i)) {
                        impl.entity_class = i;
                    }
                }
                ImGui::EndCombo();
            }
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
        ImGui::BeginDisabled(!impl.document.can_undo());
        if (ImGui::Button("Undo")) {
            (void)impl.document.undo();
            impl.document.prune_selection();
        }
        ImGui::EndDisabled();
        ImGui::SameLine();
        ImGui::BeginDisabled(!impl.document.can_redo());
        if (ImGui::Button("Redo")) {
            (void)impl.document.redo();
            impl.document.prune_selection();
        }
        ImGui::EndDisabled();
        if (impl.document.can_undo()) {
            ImGui::SameLine();
            ImGui::TextDisabled("%s", std::string(impl.document.undo_name()).c_str());
        }

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
            impl.begin_drag(i, pane.cursor, io.KeyCtrl || io.KeyShift);
        }
        if (impl.drag.active && impl.drag.view == i) {
            impl.update_drag(i, pane.cursor);
        }
        if (ImGui::IsMouseReleased(ImGuiMouseButton_Left) && impl.drag.view == i) {
            impl.end_drag();
        }
        impl.handle_input(view, pane);

        ImGui::End();
    }

    // Shortcuts, once per frame rather than once per pane, and only when no
    // text field wants the keys.
    if (!ImGui::GetIO().WantTextInput) {
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Z)) {
            (void)impl.document.undo();
            impl.document.prune_selection();
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_Z) ||
            ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Y)) {
            (void)impl.document.redo();
            impl.document.prune_selection();
        }
        if (ImGui::IsKeyPressed(ImGuiKey_Delete)) {
            impl.delete_selection();
        }
        if (ImGui::IsKeyPressed(ImGuiKey_Escape)) {
            impl.document.selection().clear();
        }
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

    for (usize i = 0; i < kViews.size(); ++i) {
        const std::vector<ViewportRenderer::Line> overlay = impl.overlay_for(i);
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
