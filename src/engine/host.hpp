// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "bsp/level.hpp"
#include "entity/entity.hpp"
#include "physics/movement.hpp"

#include <expected>
#include <memory>
#include <string>

/// The host: the loop that ties everything together.
///
/// It does not know whether there is a window. That is not a convenience, it is
/// the architecture: the simulation runs the same with or without a display, so
/// `--headless` *is* the dedicated server rather than a testing mode bolted on,
/// and an end-to-end playthrough can run in CI on a machine with no GPU. Every
/// engine that adds this later finds the renderer has grown into the
/// simulation, so it is here from the first commit -- and the build enforces
/// it, because kerosene::engine does not link kerosene::render.
namespace kero::engine {

using math::Angles;
using math::Vec3;

/// What the host was told this frame. The renderer and the terminal both fill
/// one of these in; nothing below here knows which.
struct Command {
    physics::MoveInput move;
    bool quit = false;
};

/// Everything a view needs to draw a frame. Interpolated between ticks, so the
/// picture is smooth even though the simulation is not continuous.
struct ViewState {
    Vec3 eye;
    Angles angles;
    i32 cluster = -1;
    f32 interpolation = 0.0f;
};

class Host {
public:
    Host();
    ~Host();

    Host(const Host&) = delete;
    Host& operator=(const Host&) = delete;

    /// Movable, but not copyable. Two hosts sharing one level would be a
    /// mistake; handing one to a caller is not.
    Host(Host&&) noexcept;
    Host& operator=(Host&&) noexcept;

    /// Loads a compiled level and spawns its entities.
    [[nodiscard]] std::expected<void, std::string> load_map(const std::string& path);

    [[nodiscard]] bool has_map() const { return level_ != nullptr; }
    [[nodiscard]] const bsp::Level& level() const { return *level_; }
    [[nodiscard]] entity::World& entities() { return entities_; }

    /// Advances by `seconds` of wall time, running as many fixed ticks as fit.
    ///
    /// The tick is fixed and the frame is not. A variable timestep makes a
    /// jump's height depend on the frame rate and makes a recorded input
    /// sequence unreproducible -- and reproducibility is the precondition for
    /// client prediction, which is the whole reason to fix it now rather than
    /// when networking demands it.
    void run_frame(const Command& command, f32 seconds);

    /// Runs exactly one tick. For tests and for the headless server, where
    /// there is no frame to be decoupled from.
    void tick(const Command& command);

    [[nodiscard]] ViewState view() const;
    [[nodiscard]] const physics::MoveState& player() const { return player_; }
    [[nodiscard]] physics::MoveState& player() { return player_; }
    [[nodiscard]] u64 tick_count() const { return ticks_; }
    [[nodiscard]] f32 time() const { return entities_.time(); }
    [[nodiscard]] bool quitting() const { return quit_; }
    void request_quit() { quit_ = true; }

    /// The seconds one tick covers.
    [[nodiscard]] static f32 tick_interval();

private:
    void spawn_player();
    void touch_triggers();

    std::unique_ptr<bsp::Level> level_;
    entity::World entities_;
    physics::Mover mover_;
    physics::MoveState player_;
    physics::MoveState previous_player_;
    Angles view_angles_;

    /// The trigger the player is currently standing in, so entering one fires
    /// once rather than every tick.
    u32 touching_ = Index::kNone;

    f32 accumulator_ = 0.0f;
    u64 ticks_ = 0;
    bool quit_ = false;
    std::string map_name_;
};

}  // namespace kero::engine
