// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "game/game.hpp"

#include "core/log.hpp"

#include <format>

namespace kero::game {
namespace {

KERO_LOG_CATEGORY(log, "game");

using entity::Entity;
using entity::Parameter;
using entity::World;

/// A class with no behaviour of its own.
///
/// Worth having rather than declining to create: an entity that exists can be
/// found by name, can be the target of a wire, and shows up in `ent_list`. A
/// `light` does nothing at runtime -- Radiance baked it -- but a level that
/// refers to one by name should still find it.
class Inert : public Entity {};

/// Where the player appears.
class PlayerStart : public Entity {};

/// A brush entity that is only geometry. Cleave has already merged it into the
/// leaves; at runtime it is a name and nothing more.
class Detail : public Entity {};

/// Relays an input to its outputs, optionally after a delay.
///
/// The most useful entity in the whole system despite doing nothing: it turns
/// one event into several, gives a sequence a name to hang off, and is where a
/// designer puts the delay so it can be changed in one place.
class LogicRelay : public Entity {
public:
    bool accept(World& world, std::string_view input, const Parameter& parameter,
                u32 activator) override {
        if (input == "Trigger") {
            if (disabled_) {
                return true;  // Understood, and deliberately ignored.
            }
            fire(world, "OnTrigger", activator, parameter);
            if (property("spawnflags") == "1") {
                disabled_ = true;  // Fire once, then latch off.
            }
            return true;
        }
        if (input == "Enable") {
            disabled_ = false;
            return true;
        }
        if (input == "Disable") {
            disabled_ = true;
            return true;
        }
        return false;
    }

private:
    bool disabled_ = false;
};

/// Fires when something enters its volume.
///
/// The touch test lives in the engine, which knows where the player is; this
/// holds the wiring and the re-trigger delay.
class TriggerMultiple : public Entity {
public:
    void spawn(World&) override {
        wait_ = property_number("wait").value_or(1.0f);
    }

    bool accept(World& world, std::string_view input, const Parameter& parameter,
                u32 activator) override {
        if (input == "Touch") {
            // Not re-fired while the wait is still running, which is what stops
            // a player standing in a trigger from firing it sixty-six times a
            // second.
            if (world.time() < next_allowed_) {
                return true;
            }
            next_allowed_ = world.time() + wait_;
            fire(world, "OnStartTouch", activator, parameter);
            return true;
        }
        if (input == "Enable") {
            enabled_ = true;
            return true;
        }
        if (input == "Disable") {
            enabled_ = false;
            return true;
        }
        return false;
    }

    [[nodiscard]] bool enabled() const { return enabled_; }

private:
    f32 wait_ = 1.0f;
    f32 next_allowed_ = 0.0f;
    bool enabled_ = true;
};

/// Chooses between two outputs on a stored boolean. The "otherwise" that stops
/// entity I/O needing a scripting language.
class LogicBranch : public Entity {
public:
    void spawn(World&) override {
        value_ = property("InitialValue") == "1";
    }

    bool accept(World& world, std::string_view input, const Parameter& parameter,
                u32 activator) override {
        if (input == "SetValue") {
            value_ = parameter == "1";
            return true;
        }
        if (input == "Toggle") {
            value_ = !value_;
            return true;
        }
        if (input == "Test") {
            fire(world, value_ ? "OnTrue" : "OnFalse", activator, parameter);
            return true;
        }
        return false;
    }

private:
    bool value_ = false;
};

template <typename T>
std::unique_ptr<Entity> make() {
    return std::make_unique<T>();
}

}  // namespace

entity::Factory factory() {
    return [](std::string_view classname) -> std::unique_ptr<Entity> {
        if (classname == "worldspawn")        return make<Inert>();
        if (classname == "info_player_start") return make<PlayerStart>();
        if (classname == "light")             return make<Inert>();
        if (classname == "light_environment") return make<Inert>();
        if (classname == "func_detail")       return make<Detail>();
        if (classname == "logic_relay")       return make<LogicRelay>();
        if (classname == "logic_branch")      return make<LogicBranch>();
        if (classname == "trigger_multiple")  return make<TriggerMultiple>();
        return nullptr;
    };
}

SpawnPoint find_spawn(World& world) {
    SpawnPoint spawn;

    if (const Entity* start = world.first_by_class("info_player_start")) {
        spawn.origin = start->origin();
        spawn.angles = start->angles();
        spawn.found = true;
        return spawn;
    }

    // A level with no spawn point is a mistake, but refusing to load it makes
    // the mistake harder to see. Start at the origin, say so, and let the
    // designer walk to the problem.
    KERO_WARN(log,
              "no info_player_start; spawning at the origin. Place one in the "
              "level so the player starts somewhere sensible");
    spawn.origin = math::Vec3(0.0f, 0.0f, 16.0f);
    return spawn;
}

}  // namespace kero::game
