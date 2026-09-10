// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "map/map.hpp"

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace kero::chisel {

/// One reversible change to the map.
///
/// A command stack rather than snapshots of the whole map. Snapshots are far
/// easier and are the reason editors develop a memory ceiling on large levels:
/// a hundred steps of undo on a ten-thousand-brush level is a hundred copies of
/// it. Getting this right at twenty brushes costs almost nothing; retrofitting
/// it at twenty thousand costs everything.
class Edit {
public:
    virtual ~Edit() = default;

    virtual void apply(map::Map& map) = 0;
    virtual void revert(map::Map& map) = 0;

    /// What the Undo menu item says. Written as the action -- "Move brush",
    /// not "moved a brush" -- because the menu reads "Undo move brush".
    [[nodiscard]] virtual std::string_view describe() const = 0;
};

/// What the user has selected.
///
/// Held as ids rather than indices. Indices shift the moment anything is added
/// or removed, so a selection stored that way silently comes to mean something
/// else after an undo -- which is exactly when the user is watching.
struct Selection {
    std::vector<i32> solids;
    std::vector<i32> entities;

    /// The face within the single selected solid, when one is picked. Faces are
    /// positional within their brush and a brush's side list never reorders, so
    /// an index is safe here where it is not for the brush itself.
    std::optional<usize> face;

    [[nodiscard]] bool empty() const { return solids.empty() && entities.empty(); }
    [[nodiscard]] usize size() const { return solids.size() + entities.size(); }
    void clear() {
        solids.clear();
        entities.clear();
        face.reset();
    }

    [[nodiscard]] bool contains_solid(i32 id) const;
    void toggle_solid(i32 id);
    [[nodiscard]] bool contains_entity(i32 id) const;
    void toggle_entity(i32 id);
};

/// The map being edited, its undo history, and what is selected.
class Document {
public:
    Document();

    /// Starts an empty map. The world exists from the outset -- a map with no
    /// worldspawn is not a map, and making the editor able to represent one
    /// would mean every later stage having to cope with it.
    void reset();

    [[nodiscard]] bool open(const std::string& path, std::string& error);
    [[nodiscard]] bool save(std::string& error);
    [[nodiscard]] bool save_as(const std::string& path, std::string& error);

    [[nodiscard]] const map::Map& map() const { return map_; }
    [[nodiscard]] const std::string& path() const { return path_; }
    [[nodiscard]] bool has_path() const { return !path_.empty(); }

    /// Whether there is unsaved work. Undoing back to the point the file was
    /// written clears it, which is what a person means by "unchanged".
    [[nodiscard]] bool dirty() const { return position_ != saved_position_; }

    /// Applies an edit and pushes it onto the undo stack. The only way the map
    /// is ever changed -- there is no accessor that hands out a mutable map,
    /// which is what keeps undo honest.
    void apply(std::unique_ptr<Edit> edit);

    [[nodiscard]] bool can_undo() const { return position_ > 0; }
    [[nodiscard]] bool can_redo() const { return position_ < edits_.size(); }
    [[nodiscard]] std::string_view undo_name() const;
    [[nodiscard]] std::string_view redo_name() const;
    bool undo();
    bool redo();

    [[nodiscard]] usize history_size() const { return edits_.size(); }

    /// Bumped whenever the map changes, by an edit or by undo, redo or open.
    ///
    /// The viewport rebuilds its buffers when this moves. Comparing a counter
    /// beats every alternative: a dirty flag has to be cleared by someone, and
    /// diffing the map to find out what changed costs more than redrawing it.
    [[nodiscard]] u64 revision() const { return revision_; }

    // --- Lookup ------------------------------------------------------------

    [[nodiscard]] const map::Solid* find_solid(i32 id) const;
    [[nodiscard]] const map::Entity* find_entity(i32 id) const;

    /// Which entity owns a brush. The world's own id for a world brush.
    [[nodiscard]] std::optional<i32> owner_of(i32 solid_id) const;

    /// Every solid in the map, with the entity that owns it.
    struct SolidRef {
        i32 owner = 0;
        const map::Solid* solid = nullptr;
    };
    [[nodiscard]] std::vector<SolidRef> all_solids() const;

    /// The next unused id. Ids are unique across brushes, sides and entities
    /// because the `.kmap` format numbers them from one pool, and the editor
    /// must not be the thing that breaks that.
    [[nodiscard]] i32 allocate_id();

    [[nodiscard]] Selection& selection() { return selection_; }
    [[nodiscard]] const Selection& selection() const { return selection_; }

    /// Drops anything selected that is no longer in the map -- after an undo of
    /// a create, say.
    void prune_selection();

private:
    void note_ids();

    map::Map map_;
    std::string path_;

    std::vector<std::unique_ptr<Edit>> edits_;
    /// How many of `edits_` are currently applied. Redo is everything past it,
    /// which is why an edit made after an undo truncates the tail.
    usize position_ = 0;
    usize saved_position_ = 0;

    Selection selection_;
    i32 next_id_ = 1;
    u64 revision_ = 1;
};

// --- The edits ------------------------------------------------------------

/// Adds a brush to an entity, or to the world.
class AddSolid : public Edit {
public:
    AddSolid(i32 owner, map::Solid solid, std::string description);

    void apply(map::Map& map) override;
    void revert(map::Map& map) override;
    [[nodiscard]] std::string_view describe() const override { return description_; }

private:
    i32 owner_;
    map::Solid solid_;
    std::string description_;
};

/// Removes a brush, remembering where it was so redo puts it back in place.
class RemoveSolid : public Edit {
public:
    RemoveSolid(i32 owner, i32 solid_id, std::string description);

    void apply(map::Map& map) override;
    void revert(map::Map& map) override;
    [[nodiscard]] std::string_view describe() const override { return description_; }

private:
    i32 owner_;
    i32 solid_id_;
    usize index_ = 0;
    map::Solid removed_;
    std::string description_;
};

/// Replaces a brush wholesale.
///
/// Moving, resizing and retexturing are all this. Storing the before and after
/// of one brush is a kilobyte or so -- per *brush*, not per map -- which buys a
/// great deal of simplicity over encoding each kind of change as its own delta.
/// A drag produces exactly one of these, captured when the drag starts and
/// pushed when it ends, so holding the mouse down does not fill the history.
class ReplaceSolid : public Edit {
public:
    ReplaceSolid(i32 owner, map::Solid before, map::Solid after, std::string description);

    void apply(map::Map& map) override;
    void revert(map::Map& map) override;
    [[nodiscard]] std::string_view describe() const override { return description_; }

private:
    void swap_in(map::Map& map, const map::Solid& solid);

    i32 owner_;
    map::Solid before_;
    map::Solid after_;
    std::string description_;
};

class AddEntity : public Edit {
public:
    AddEntity(map::Entity entity, std::string description);

    void apply(map::Map& map) override;
    void revert(map::Map& map) override;
    [[nodiscard]] std::string_view describe() const override { return description_; }

private:
    map::Entity entity_;
    std::string description_;
};

class RemoveEntity : public Edit {
public:
    RemoveEntity(i32 entity_id, std::string description);

    void apply(map::Map& map) override;
    void revert(map::Map& map) override;
    [[nodiscard]] std::string_view describe() const override { return description_; }

private:
    i32 entity_id_;
    usize index_ = 0;
    map::Entity removed_;
    std::string description_;
};

/// Replaces an entity wholesale: properties, wiring, position, brushes.
class ReplaceEntity : public Edit {
public:
    ReplaceEntity(map::Entity before, map::Entity after, std::string description);

    void apply(map::Map& map) override;
    void revert(map::Map& map) override;
    [[nodiscard]] std::string_view describe() const override { return description_; }

private:
    void swap_in(map::Map& map, const map::Entity& entity);

    map::Entity before_;
    map::Entity after_;
    std::string description_;
};

}  // namespace kero::chisel
