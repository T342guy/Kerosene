// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "kv/keyvalues.hpp"
#include "math/angles.hpp"
#include "math/vec.hpp"

#include <array>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

/// Entities, and the wiring between them.
///
/// Kerosene has no scripting language, and the reason is that Source showed one
/// is not needed. A button's `OnPressed` fires a door's `Open` after a delay;
/// a trigger's `OnStartTouch` fires a relay; a relay fires three things at
/// once. That is the whole mechanism, and it composes far further than it has
/// any right to -- lifts, timed sequences, branching puzzles -- while staying
/// something a level designer can see and edit as wires in an editor rather
/// than as code in a file.
///
/// What it buys over a script language is that the graph is *data*. It can be
/// drawn, validated at compile time, diffed, and reasoned about without running
/// it. A `logic_branch` covers the case where the answer is "otherwise", and
/// that turns out to be enough.
namespace kero::entity {

using math::Angles;
using math::Vec3;

class World;
class Entity;

/// A value passed along a wire. Most inputs take nothing; some take a number or
/// a name.
using Parameter = std::string;

/// One queued firing, waiting for its delay to elapse.
struct Event {
    std::string target;      ///< targetname to deliver to, or a classname.
    std::string input;
    Parameter parameter;
    f32 fire_at = 0.0f;      ///< Level time, in seconds.
    u32 activator = Index::kNone;  ///< Who set this off, for `!activator`.
    u32 caller = Index::kNone;     ///< Who fired the output.

    /// Ordering within the same timestamp, so a sequence wired to one output
    /// fires in the order the designer wrote it rather than in hash order.
    u64 sequence = 0;
};

/// One wire, as authored.
struct Output {
    std::string name;        ///< The event on this entity, e.g. "OnStartTouch".
    std::string target;
    std::string input;
    Parameter parameter;
    f32 delay = 0.0f;
    i32 times_to_fire = -1;  ///< -1 is unlimited.
    i32 times_fired = 0;
};

/// The base class every entity class derives from.
///
/// Fields are read out of the KeyValues the map carried, so an entity class
/// takes what it understands and the rest stays available. That is what lets
/// the game code gain a feature without every map being recompiled.
class Entity {
public:
    virtual ~Entity() = default;

    [[nodiscard]] u32 index() const { return index_; }

    /// Where this entity sat in the level's entity lump. Not the same as
    /// index(): a classname this build does not implement is skipped, which
    /// shifts everything after it. Brushes refer to entities by this.
    [[nodiscard]] u32 source_index() const { return source_index_; }
    [[nodiscard]] std::string_view classname() const { return classname_; }
    [[nodiscard]] std::string_view targetname() const { return targetname_; }
    [[nodiscard]] const Vec3& origin() const { return origin_; }
    [[nodiscard]] const Angles& angles() const { return angles_; }
    [[nodiscard]] const std::vector<Output>& outputs() const { return outputs_; }

    /// The raw property, for a class that wants a key nothing else knows about.
    [[nodiscard]] std::string_view property(std::string_view key,
                                            std::string_view fallback = {}) const;
    [[nodiscard]] std::optional<f32> property_number(std::string_view key) const;

    /// Called once after every entity in the level has been created, so a class
    /// may look up the entities it refers to.
    virtual void spawn(World&) {}

    /// Called every tick.
    virtual void think(World&, f32 /*dt*/) {}

    /// Handles a fired input. Return false if the name means nothing here, so
    /// the world can report a wire that goes nowhere.
    virtual bool accept(World&, std::string_view input, const Parameter&, u32 activator);

    /// Fires an output by name, queueing everything wired to it.
    void fire(World& world, std::string_view output_name, u32 activator,
              const Parameter& parameter = {});

private:
    friend class World;

    u32 index_ = Index::kNone;
    u32 source_index_ = Index::kNone;
    std::string classname_;
    std::string targetname_;
    Vec3 origin_;
    Angles angles_;
    std::vector<kv::Pair> properties_;
    std::vector<Output> outputs_;
};

/// Creates an entity for a classname. Returns null for one the game does not
/// implement, which is not an error -- an unknown entity is left out and
/// reported, and the level still runs.
using Factory = std::function<std::unique_ptr<Entity>(std::string_view classname)>;

/// Every entity in the level, and the event queue between them.
class World {
public:
    /// Builds the entities from a level's entity lump.
    ///
    /// `unknown` receives every classname the factory declined, so the caller
    /// can report them once rather than per instance.
    void load(std::string_view entity_text, const Factory& factory,
              std::vector<std::string>& unknown);

    [[nodiscard]] usize size() const { return entities_.size(); }
    [[nodiscard]] Entity* at(u32 index);
    [[nodiscard]] const Entity* at(u32 index) const;

    /// Every entity with this targetname. Repeats are ordinary: several doors
    /// sharing a name is how you open them together.
    [[nodiscard]] std::vector<Entity*> find_by_name(std::string_view targetname);
    [[nodiscard]] std::vector<Entity*> find_by_class(std::string_view classname);
    [[nodiscard]] Entity* first_by_class(std::string_view classname);

    /// The entity that sat at `source_index` in the level's entity lump, or
    /// null if it was one this build does not implement.
    [[nodiscard]] Entity* find_by_source_index(u32 source_index);

    /// Queues an input to be delivered after `delay` seconds.
    void queue(std::string_view target, std::string_view input, const Parameter& parameter,
               f32 delay, u32 activator, u32 caller);

    /// Delivers everything due, then runs every entity's think.
    void tick(f32 dt);

    [[nodiscard]] f32 time() const { return time_; }
    [[nodiscard]] usize pending_events() const { return queue_.size(); }

    /// Wires whose target names nothing, found at load. A designer wants to
    /// know at compile time, not when the button turns out to do nothing.
    [[nodiscard]] const std::vector<std::string>& dangling_wires() const {
        return dangling_;
    }

    /// How many inputs have been delivered. Used by the tests, and by
    /// `ent_stats`.
    [[nodiscard]] u64 delivered() const { return delivered_; }

private:
    std::vector<std::unique_ptr<Entity>> entities_;
    std::unordered_map<std::string, std::vector<u32>> by_name_;
    std::vector<Event> queue_;
    std::vector<std::string> dangling_;
    f32 time_ = 0.0f;
    u64 sequence_ = 0;
    u64 delivered_ = 0;
};

}  // namespace kero::entity
