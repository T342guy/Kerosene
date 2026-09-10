// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "engine/host.hpp"

#include "console/console.hpp"
#include "core/log.hpp"
#include "game/game.hpp"
#include "math/units.hpp"

#include <algorithm>
#include <format>

namespace kero::engine {
namespace {

KERO_LOG_CATEGORY(log, "engine");

using console::ConVar;

ConVar sv_tickrate("sv_tickrate", "66",
                   "Simulation ticks per second. Fixed, and decoupled from the frame "
                   "rate: a variable timestep makes a jump's height depend on how fast "
                   "your machine is and makes a recorded input sequence unreproducible.",
                   console::VarFlags::None, ConVar::Range{10.0f, 256.0f});

ConVar host_maxticks("host_maxticks", "8",
                     "The most simulation ticks one frame may run. A frame that stalls "
                     "must not try to catch up all at once -- that turns a hitch into a "
                     "spiral where each frame has more to do than the last.");

/// How far below a spawn point to look for the floor.
///
/// Generous: a designer's placement is approximate, and a start point a few
/// units too high should not begin the level with a fall. Too far and a start
/// point over a pit would teleport the player to the bottom of it, which is why
/// this is not simply unbounded.
constexpr f32 kSpawnDrop = 64.0f;

/// How far a mover may be from a trigger's leaf and still be considered inside
/// it. The touch test is a leaf-contents query, which is exact enough for a
/// volume built out of brushes.
ConVar sv_showtriggers("sv_showtriggers", "0",
                       "Log every trigger touch. For working out why a wire is not "
                       "firing.", console::VarFlags::Cheat);

}  // namespace

Host::Host() = default;
Host::~Host() = default;
Host::Host(Host&&) noexcept = default;
Host& Host::operator=(Host&&) noexcept = default;

f32 Host::tick_interval() { return 1.0f / std::max(sv_tickrate.number(), 1.0f); }

std::expected<void, std::string> Host::load_map(const std::string& path) {
    auto level = bsp::Level::load(path);
    if (!level) {
        return std::unexpected(level.error());
    }

    level_ = std::make_unique<bsp::Level>(std::move(*level));
    entities_ = entity::World{};
    map_name_ = path;

    std::vector<std::string> unknown;
    entities_.load(level_->entities(), game::factory(), unknown);

    for (const std::string& classname : unknown) {
        // Reported once per class, not once per instance, and as a warning
        // rather than a failure: a map may name an entity this build does not
        // implement, and the level still runs without it.
        KERO_WARN(log, "no entity class for \"{}\"; those entities were left out",
                  classname);
    }

    spawn_player();
    ticks_ = 0;
    accumulator_ = 0.0f;

    KERO_INFO(log, "loaded {} -- {} entities, player at {}", path, entities_.size(),
              player_.origin);
    return {};
}

void Host::spawn_player() {
    const game::SpawnPoint spawn = game::find_spawn(entities_);

    player_ = physics::MoveState{};
    player_.origin = spawn.origin;
    view_angles_ = spawn.angles;

    if (level_ != nullptr) {
        if (any(level_->contents_at(player_.origin) & bsp::Contents::Solid)) {
            KERO_WARN(log,
                      "the spawn point at {} is inside solid geometry; the player "
                      "will be stuck",
                      player_.origin);
        } else {
            // Dropped onto the floor beneath the spawn point.
            //
            // A designer places an info_player_start a little above the floor
            // so it is visibly not embedded in it, and the player should not
            // then begin the level in mid-air. Source does the same fixup, and
            // the alternative -- demanding the entity be placed to the
            // millimetre -- makes every level's first moment a small fall.
            const bsp::Trace drop = level_->trace(
                player_.origin, player_.origin - Vec3(0.0f, 0.0f, kSpawnDrop),
                physics::Mover::hull(false), bsp::Contents::SolidMask);
            if (drop.hit() && !drop.start_solid) {
                player_.origin = drop.end;
            }
        }
        mover_.categorise_position(*level_, player_);
    }
    previous_player_ = player_;
}

void Host::touch_triggers() {
    if (level_ == nullptr) {
        return;
    }

    // A trigger is touched when the player's box overlaps it. Asked as a sweep
    // of zero length, so the same code answers it as answers movement -- one
    // implementation of "what am I inside" rather than two that can disagree.
    const bsp::Trace trace =
        level_->trace(player_.origin, player_.origin, physics::Mover::hull(player_.ducking),
                      bsp::Contents::Trigger);

    const bool inside = trace.start_solid || trace.hit();
    if (!inside) {
        touching_ = Index::kNone;
        return;
    }

    // Which trigger, not merely whether one. The brush records the entity it
    // came from, so the touch goes to that entity and not to every trigger in
    // the level.
    const std::optional<u32> owner = level_->owner_of_brush(trace.brush);
    entity::Entity* trigger =
        owner ? entities_.find_by_source_index(*owner) : nullptr;
    if (trigger == nullptr) {
        return;
    }

    // Only on entry. Standing in a trigger is not sixty-six touches a second,
    // and the input is called OnStartTouch for a reason -- the re-trigger delay
    // in the entity itself is a second line of defence, not the first.
    if (touching_ == trigger->index()) {
        return;
    }
    touching_ = trigger->index();

    if (sv_showtriggers.boolean()) {
        KERO_INFO(log, "touching {} \"{}\"", trigger->classname(), trigger->targetname());
    }
    entities_.queue(trigger->targetname().empty() ? std::string(trigger->classname())
                                                  : std::string(trigger->targetname()),
                    "Touch", {}, 0.0f, Index::kNone, Index::kNone);
}

void Host::tick(const Command& command) {
    if (level_ == nullptr) {
        return;
    }

    previous_player_ = player_;
    view_angles_ = command.move.view;

    physics::MoveInput input = command.move;
    input.view = view_angles_;

    (void)mover_.move(*level_, player_, input, tick_interval());
    touch_triggers();
    entities_.tick(tick_interval());

    ++ticks_;
    if (command.quit) {
        quit_ = true;
    }
}

void Host::run_frame(const Command& command, f32 seconds) {
    // Console commands run between ticks, never inside one. A `map` command
    // that tore the level down mid-tick would leave the mover holding a
    // dangling level.
    console::flush();

    if (level_ == nullptr) {
        if (command.quit) {
            quit_ = true;
        }
        return;
    }

    accumulator_ += std::clamp(seconds, 0.0f, 1.0f);

    const f32 interval = tick_interval();
    const i32 budget = std::max(host_maxticks.integer(), 1);

    i32 ran = 0;
    while (accumulator_ >= interval && ran < budget) {
        tick(command);
        accumulator_ -= interval;
        ++ran;
    }

    if (accumulator_ >= interval) {
        // Behind by more than one frame's budget. Dropping the backlog is the
        // right answer: trying to catch up makes the next frame longer still,
        // and the frame after that longer again.
        KERO_WARN(log, "dropped {:.0f} ms of simulation to avoid a catch-up spiral",
                  static_cast<f64>(accumulator_ * 1000.0f));
        accumulator_ = 0.0f;
    }

    if (command.quit) {
        quit_ = true;
    }
}

ViewState Host::view() const {
    ViewState state;
    const f32 alpha =
        level_ != nullptr ? std::clamp(accumulator_ / tick_interval(), 0.0f, 1.0f) : 0.0f;

    // Interpolated between the last two ticks, so the picture is smooth at any
    // frame rate even though the simulation moves in fixed steps.
    const Vec3 feet = lerp(previous_player_.origin, player_.origin, alpha);
    state.eye = feet + Vec3(0.0f, 0.0f,
                            player_.ducking ? units::kPlayerDuckHeight * 0.85f
                                            : units::kPlayerEyeHeight);
    state.angles = view_angles_;
    state.interpolation = alpha;
    state.cluster = level_ != nullptr ? level_->cluster_at(state.eye) : -1;
    return state;
}

}  // namespace kero::engine
