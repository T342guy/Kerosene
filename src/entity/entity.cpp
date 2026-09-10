// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "entity/entity.hpp"

#include "core/log.hpp"

#include <algorithm>
#include <charconv>
#include <format>

namespace kero::entity {
namespace {

KERO_LOG_CATEGORY(log, "entity");

std::optional<f32> to_number(std::string_view text) {
    f32 value{};
    const auto [stop, code] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (code != std::errc{}) {
        return std::nullopt;
    }
    (void)stop;
    return value;
}

/// "target,input,parameter,delay,times" -- Source's wire format, kept because
/// it is compact and because a comma cannot appear in the fields before the
/// numeric ones.
Output parse_output(const kv::Pair& pair) {
    Output output;
    output.name = pair.key;

    std::vector<std::string_view> fields;
    std::string_view rest = pair.value;
    while (fields.size() < 5) {
        const usize comma = rest.find(',');
        if (comma == std::string_view::npos) {
            fields.push_back(rest);
            break;
        }
        fields.push_back(rest.substr(0, comma));
        rest = rest.substr(comma + 1);
    }
    auto field = [&fields](usize i) {
        return i < fields.size() ? fields[i] : std::string_view{};
    };

    output.target = std::string(field(0));
    output.input = std::string(field(1));
    output.parameter = std::string(field(2));
    if (const std::optional<f32> delay = to_number(field(3))) {
        output.delay = *delay;
    }
    if (const std::optional<f32> times = to_number(field(4))) {
        output.times_to_fire = static_cast<i32>(*times);
    }
    return output;
}

}  // namespace

std::string_view Entity::property(std::string_view key, std::string_view fallback) const {
    for (const kv::Pair& pair : properties_) {
        if (pair.key == key) {
            return pair.value;
        }
    }
    return fallback;
}

std::optional<f32> Entity::property_number(std::string_view key) const {
    const std::string_view text = property(key);
    return text.empty() ? std::nullopt : to_number(text);
}

bool Entity::accept(World&, std::string_view, const Parameter&, u32) { return false; }

void Entity::fire(World& world, std::string_view output_name, u32 activator,
                  const Parameter& parameter) {
    for (Output& output : outputs_) {
        if (output.name != output_name) {
            continue;
        }
        if (output.times_to_fire >= 0 && output.times_fired >= output.times_to_fire) {
            continue;
        }
        ++output.times_fired;

        // A parameter on the wire wins over one supplied by the firing code:
        // the designer wrote it down, and the code did not know about it.
        const Parameter& value = output.parameter.empty() ? parameter : output.parameter;
        world.queue(output.target, output.input, value, output.delay, activator, index_);
    }
}

void World::load(std::string_view entity_text, const Factory& factory,
                 std::vector<std::string>& unknown) {
    auto document = kv::parse(entity_text, "<entity lump>");
    if (!document) {
        KERO_ERROR(log, "the entity lump does not parse: {}", document.error().format());
        return;
    }

    std::vector<std::string> unimplemented;

    u32 source_index = 0;
    for (const kv::Block& block : document->blocks) {
        const u32 here = source_index++;
        const std::string_view classname = block.get("classname");
        if (classname.empty()) {
            continue;
        }

        std::unique_ptr<Entity> made = factory ? factory(classname) : nullptr;
        if (!made) {
            // Not an error. A map may name an entity this build does not
            // implement, and dropping it leaves a level that still runs.
            if (std::ranges::find(unimplemented, classname) == unimplemented.end()) {
                unimplemented.emplace_back(classname);
            }
            continue;
        }

        made->index_ = static_cast<u32>(entities_.size());
        made->source_index_ = here;
        made->classname_ = classname;
        made->targetname_ = block.get("targetname");
        made->properties_ = block.pairs;

        if (const std::optional<std::array<f64, 3>> origin = block.get_vec3("origin")) {
            made->origin_ = Vec3(static_cast<f32>((*origin)[0]),
                                 static_cast<f32>((*origin)[1]),
                                 static_cast<f32>((*origin)[2]));
        }
        if (const std::optional<std::array<f64, 3>> angles = block.get_vec3("angles")) {
            made->angles_ = Angles(static_cast<f32>((*angles)[0]),
                                   static_cast<f32>((*angles)[1]),
                                   static_cast<f32>((*angles)[2]));
        }

        if (const kv::Block* connections = block.first_child("connections")) {
            for (const kv::Pair& pair : connections->pairs) {
                made->outputs_.push_back(parse_output(pair));
            }
        }

        if (!made->targetname_.empty()) {
            by_name_[made->targetname_].push_back(made->index_);
        }
        entities_.push_back(std::move(made));
    }

    // Wires that name nothing. Reported once, at load, because a designer wants
    // to know now rather than when the button turns out to do nothing.
    for (const std::unique_ptr<Entity>& made : entities_) {
        for (const Output& output : made->outputs()) {
            if (output.target.empty() || output.target.starts_with('!')) {
                continue;  // !activator and friends resolve at fire time.
            }
            if (by_name_.contains(output.target)) {
                continue;
            }
            if (!find_by_class(output.target).empty()) {
                continue;
            }
            dangling_.push_back(std::format("{} {} -> \"{}\" (nothing has that name)",
                                            made->classname(), output.name,
                                            output.target));
        }
    }

    for (const std::unique_ptr<Entity>& made : entities_) {
        made->spawn(*this);
    }

    unknown = std::move(unimplemented);
    KERO_INFO(log, "{} entities", entities_.size());
    for (const std::string& wire : dangling_) {
        KERO_WARN(log, "wire goes nowhere: {}", wire);
    }
}

Entity* World::at(u32 index) {
    return index < entities_.size() ? entities_[index].get() : nullptr;
}

const Entity* World::at(u32 index) const {
    return index < entities_.size() ? entities_[index].get() : nullptr;
}

std::vector<Entity*> World::find_by_name(std::string_view targetname) {
    std::vector<Entity*> found;
    const auto entry = by_name_.find(std::string(targetname));
    if (entry == by_name_.end()) {
        return found;
    }
    for (u32 index : entry->second) {
        found.push_back(entities_[index].get());
    }
    return found;
}

std::vector<Entity*> World::find_by_class(std::string_view classname) {
    std::vector<Entity*> found;
    for (const std::unique_ptr<Entity>& made : entities_) {
        if (made->classname() == classname) {
            found.push_back(made.get());
        }
    }
    return found;
}

Entity* World::find_by_source_index(u32 source_index) {
    for (const std::unique_ptr<Entity>& made : entities_) {
        if (made->source_index() == source_index) {
            return made.get();
        }
    }
    return nullptr;
}

Entity* World::first_by_class(std::string_view classname) {
    const std::vector<Entity*> found = find_by_class(classname);
    return found.empty() ? nullptr : found.front();
}

void World::queue(std::string_view target, std::string_view input,
                  const Parameter& parameter, f32 delay, u32 activator, u32 caller) {
    Event event;
    event.target = target;
    event.input = input;
    event.parameter = parameter;
    event.fire_at = time_ + std::max(delay, 0.0f);
    event.activator = activator;
    event.caller = caller;
    event.sequence = sequence_++;
    queue_.push_back(std::move(event));
}

void World::tick(f32 dt) {
    time_ += dt;

    // Everything due now, in the order it was wired. Taken as a batch so that
    // an input which queues another does not deliver it within the same tick --
    // otherwise a relay wired to itself with no delay hangs the frame, and the
    // fix would be an arbitrary iteration limit rather than a rule.
    std::vector<Event> due;
    std::vector<Event> later;
    later.reserve(queue_.size());

    for (Event& event : queue_) {
        if (event.fire_at <= time_) {
            due.push_back(std::move(event));
        } else {
            later.push_back(std::move(event));
        }
    }
    queue_ = std::move(later);

    std::ranges::sort(due, [](const Event& a, const Event& b) {
        return a.fire_at != b.fire_at ? a.fire_at < b.fire_at : a.sequence < b.sequence;
    });

    for (const Event& event : due) {
        std::vector<Entity*> targets = find_by_name(event.target);
        if (targets.empty()) {
            // A classname is accepted as a target too, which is how a level
            // addresses "every light" without naming them all.
            targets = find_by_class(event.target);
        }
        for (Entity* target : targets) {
            if (!target->accept(*this, event.input, event.parameter, event.activator)) {
                KERO_WARN(log, "{} does not understand the input \"{}\"",
                          target->classname(), event.input);
                continue;
            }
            ++delivered_;
        }
    }

    for (const std::unique_ptr<Entity>& made : entities_) {
        made->think(*this, dt);
    }
}

}  // namespace kero::entity
