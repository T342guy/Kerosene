// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "chisel/document.hpp"

#include "core/assert.hpp"

#include <algorithm>

namespace kero::chisel {
namespace {

/// Finds the entity that owns `owner_id`, or the world.
///
/// The world is an entity too and carries its own id, so "the world" is not a
/// special case here -- which keeps every edit below free of a branch that
/// would otherwise appear in all of them.
map::Entity* entity_by_id(map::Map& map, i32 owner_id) {
    if (map.world.id == owner_id) {
        return &map.world;
    }
    for (map::Entity& entity : map.entities) {
        if (entity.id == owner_id) {
            return &entity;
        }
    }
    return nullptr;
}

}  // namespace

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

bool Selection::contains_solid(i32 id) const {
    return std::ranges::find(solids, id) != solids.end();
}

void Selection::toggle_solid(i32 id) {
    if (const auto found = std::ranges::find(solids, id); found != solids.end()) {
        solids.erase(found);
        face.reset();
        return;
    }
    solids.push_back(id);
}

bool Selection::contains_entity(i32 id) const {
    return std::ranges::find(entities, id) != entities.end();
}

void Selection::toggle_entity(i32 id) {
    if (const auto found = std::ranges::find(entities, id); found != entities.end()) {
        entities.erase(found);
        return;
    }
    entities.push_back(id);
}

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

Document::Document() { reset(); }

void Document::reset() {
    map_ = map::Map{};
    map_.world.id = 1;
    map_.world.classname = "worldspawn";
    map_.world.set("id", "1");
    map_.world.set("classname", "worldspawn");

    path_.clear();
    edits_.clear();
    position_ = 0;
    saved_position_ = 0;
    selection_.clear();
    next_id_ = 2;
    ++revision_;
}

void Document::note_ids() {
    // Ids come from one pool shared by brushes, sides and entities, because
    // that is how the `.kmap` format numbers them. Continuing above the highest
    // one seen means a saved map never has two things claiming the same id.
    i32 highest = map_.world.id;
    const auto note_solid = [&highest](const map::Solid& solid) {
        highest = std::max(highest, solid.id);
        for (const map::Side& side : solid.sides) {
            highest = std::max(highest, side.id);
        }
    };

    for (const map::Solid& solid : map_.world.solids) {
        note_solid(solid);
    }
    for (const map::Entity& entity : map_.entities) {
        highest = std::max(highest, entity.id);
        for (const map::Solid& solid : entity.solids) {
            note_solid(solid);
        }
    }
    next_id_ = highest + 1;
}

bool Document::open(const std::string& path, std::string& error) {
    auto loaded = map::load(path);
    if (!loaded) {
        error = loaded.error().format();
        return false;
    }

    map_ = std::move(*loaded);
    if (map_.world.id == 0) {
        // A hand-written map may not have numbered its world. Everything below
        // addresses brushes by their owner's id, so it needs one.
        map_.world.id = 1;
    }
    path_ = path;
    edits_.clear();
    position_ = 0;
    saved_position_ = 0;
    selection_.clear();
    note_ids();
    ++revision_;
    return true;
}

bool Document::save(std::string& error) {
    if (path_.empty()) {
        error = "the map has never been saved; use Save As";
        return false;
    }
    return save_as(path_, error);
}

bool Document::save_as(const std::string& path, std::string& error) {
    if (auto written = map::save(map_, path); !written) {
        error = written.error().format();
        return false;
    }
    path_ = path;
    // Where the file matches the history. Undoing back past this point makes
    // the document dirty again, which is what a person means by "unchanged".
    saved_position_ = position_;
    return true;
}

void Document::apply(std::unique_ptr<Edit> edit) {
    KERO_ASSERT(edit != nullptr, "an edit must exist to be applied");

    // Anything that was undone is now unreachable: the history is a line, not a
    // tree. If the saved point was in the discarded tail, the file can no longer
    // be reached by undoing, so the document is dirty from here on whatever
    // happens.
    if (position_ < edits_.size()) {
        if (saved_position_ > position_) {
            saved_position_ = static_cast<usize>(-1);
        }
        edits_.erase(edits_.begin() + static_cast<isize>(position_), edits_.end());
    }

    edit->apply(map_);
    edits_.push_back(std::move(edit));
    position_ = edits_.size();
    ++revision_;
    prune_selection();
}

std::string_view Document::undo_name() const {
    return can_undo() ? edits_[position_ - 1]->describe() : std::string_view{};
}

std::string_view Document::redo_name() const {
    return can_redo() ? edits_[position_]->describe() : std::string_view{};
}

bool Document::undo() {
    if (!can_undo()) {
        return false;
    }
    --position_;
    edits_[position_]->revert(map_);
    ++revision_;
    prune_selection();
    return true;
}

bool Document::redo() {
    if (!can_redo()) {
        return false;
    }
    edits_[position_]->apply(map_);
    ++position_;
    ++revision_;
    prune_selection();
    return true;
}

const map::Solid* Document::find_solid(i32 id) const {
    for (const map::Solid& solid : map_.world.solids) {
        if (solid.id == id) {
            return &solid;
        }
    }
    for (const map::Entity& entity : map_.entities) {
        for (const map::Solid& solid : entity.solids) {
            if (solid.id == id) {
                return &solid;
            }
        }
    }
    return nullptr;
}

const map::Entity* Document::find_entity(i32 id) const {
    if (map_.world.id == id) {
        return &map_.world;
    }
    for (const map::Entity& entity : map_.entities) {
        if (entity.id == id) {
            return &entity;
        }
    }
    return nullptr;
}

std::optional<i32> Document::owner_of(i32 solid_id) const {
    for (const map::Solid& solid : map_.world.solids) {
        if (solid.id == solid_id) {
            return map_.world.id;
        }
    }
    for (const map::Entity& entity : map_.entities) {
        for (const map::Solid& solid : entity.solids) {
            if (solid.id == solid_id) {
                return entity.id;
            }
        }
    }
    return std::nullopt;
}

std::vector<Document::SolidRef> Document::all_solids() const {
    std::vector<SolidRef> found;
    found.reserve(map_.brush_count());

    for (const map::Solid& solid : map_.world.solids) {
        found.push_back(SolidRef{map_.world.id, &solid});
    }
    for (const map::Entity& entity : map_.entities) {
        for (const map::Solid& solid : entity.solids) {
            found.push_back(SolidRef{entity.id, &solid});
        }
    }
    return found;
}

i32 Document::allocate_id() { return next_id_++; }

void Document::prune_selection() {
    // Undoing a create leaves the selection naming something that is gone.
    std::erase_if(selection_.solids,
                  [this](i32 id) { return find_solid(id) == nullptr; });
    std::erase_if(selection_.entities,
                  [this](i32 id) { return find_entity(id) == nullptr; });

    if (selection_.face) {
        if (selection_.solids.size() != 1) {
            selection_.face.reset();
        } else if (const map::Solid* solid = find_solid(selection_.solids.front())) {
            if (*selection_.face >= solid->sides.size()) {
                selection_.face.reset();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The edits
// ---------------------------------------------------------------------------

void Compound::add(std::unique_ptr<Edit> edit) {
    if (edit != nullptr) {
        edits_.push_back(std::move(edit));
    }
}

void Compound::apply(map::Map& map) {
    for (const std::unique_ptr<Edit>& edit : edits_) {
        edit->apply(map);
    }
}

void Compound::revert(map::Map& map) {
    // Backwards: a remove-then-add pair only comes apart in the order it went
    // together.
    for (auto it = edits_.rbegin(); it != edits_.rend(); ++it) {
        (*it)->revert(map);
    }
}

AddSolid::AddSolid(i32 owner, map::Solid solid, std::string description)
    : owner_(owner), solid_(std::move(solid)), description_(std::move(description)) {}

void AddSolid::apply(map::Map& map) {
    if (map::Entity* owner = entity_by_id(map, owner_)) {
        owner->solids.push_back(solid_);
    }
}

void AddSolid::revert(map::Map& map) {
    if (map::Entity* owner = entity_by_id(map, owner_)) {
        std::erase_if(owner->solids,
                      [this](const map::Solid& solid) { return solid.id == solid_.id; });
    }
}

RemoveSolid::RemoveSolid(i32 owner, i32 solid_id, std::string description)
    : owner_(owner), solid_id_(solid_id), description_(std::move(description)) {}

void RemoveSolid::apply(map::Map& map) {
    map::Entity* owner = entity_by_id(map, owner_);
    if (owner == nullptr) {
        return;
    }
    for (usize i = 0; i < owner->solids.size(); ++i) {
        if (owner->solids[i].id != solid_id_) {
            continue;
        }
        // Where it was, not just what it was. Redo putting a brush back at the
        // end of the list would reorder the file, and a map that reorders
        // itself when you undo and redo is one nobody can diff.
        index_ = i;
        removed_ = owner->solids[i];
        owner->solids.erase(owner->solids.begin() + static_cast<isize>(i));
        return;
    }
}

void RemoveSolid::revert(map::Map& map) {
    map::Entity* owner = entity_by_id(map, owner_);
    if (owner == nullptr) {
        return;
    }
    const usize where = std::min(index_, owner->solids.size());
    owner->solids.insert(owner->solids.begin() + static_cast<isize>(where), removed_);
}

ReplaceSolid::ReplaceSolid(i32 owner, map::Solid before, map::Solid after,
                           std::string description)
    : owner_(owner), before_(std::move(before)), after_(std::move(after)),
      description_(std::move(description)) {}

void ReplaceSolid::swap_in(map::Map& map, const map::Solid& solid) {
    map::Entity* owner = entity_by_id(map, owner_);
    if (owner == nullptr) {
        return;
    }
    for (map::Solid& existing : owner->solids) {
        if (existing.id == solid.id) {
            existing = solid;
            return;
        }
    }
}

void ReplaceSolid::apply(map::Map& map) { swap_in(map, after_); }
void ReplaceSolid::revert(map::Map& map) { swap_in(map, before_); }

AddEntity::AddEntity(map::Entity entity, std::string description)
    : entity_(std::move(entity)), description_(std::move(description)) {}

void AddEntity::apply(map::Map& map) { map.entities.push_back(entity_); }

void AddEntity::revert(map::Map& map) {
    std::erase_if(map.entities,
                  [this](const map::Entity& entity) { return entity.id == entity_.id; });
}

RemoveEntity::RemoveEntity(i32 entity_id, std::string description)
    : entity_id_(entity_id), description_(std::move(description)) {}

void RemoveEntity::apply(map::Map& map) {
    for (usize i = 0; i < map.entities.size(); ++i) {
        if (map.entities[i].id != entity_id_) {
            continue;
        }
        index_ = i;
        removed_ = map.entities[i];
        map.entities.erase(map.entities.begin() + static_cast<isize>(i));
        return;
    }
}

void RemoveEntity::revert(map::Map& map) {
    const usize where = std::min(index_, map.entities.size());
    map.entities.insert(map.entities.begin() + static_cast<isize>(where), removed_);
}

ReplaceEntity::ReplaceEntity(map::Entity before, map::Entity after,
                             std::string description)
    : before_(std::move(before)), after_(std::move(after)),
      description_(std::move(description)) {}

void ReplaceEntity::swap_in(map::Map& map, const map::Entity& entity) {
    if (map.world.id == entity.id) {
        map.world = entity;
        return;
    }
    for (map::Entity& existing : map.entities) {
        if (existing.id == entity.id) {
            existing = entity;
            return;
        }
    }
}

void ReplaceEntity::apply(map::Map& map) { swap_in(map, after_); }
void ReplaceEntity::revert(map::Map& map) { swap_in(map, before_); }

}  // namespace kero::chisel
