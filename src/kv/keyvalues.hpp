// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <array>
#include <expected>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

/// KeyValues -- the text format maps, materials, projects and configuration are
/// written in.
///
/// ```
/// world
/// {
///     "classname" "worldspawn"
///     solid
///     {
///         side
///         {
///             "plane"    "(512 -8 0) (-8 -8 0) (-8 264 0)"
///             "material" "dev/grid"
///         }
///     }
/// }
/// ```
///
/// Blocks are named and nested; leaves are quoted key/value pairs. Repeated
/// names are ordinary -- a solid has six `side` blocks -- so this is a list of
/// entries in file order, not a map. That ordering is load-bearing: brush sides
/// are referred to by position, and a parser that silently reordered or merged
/// them would corrupt maps rather than reject them.
///
/// Everything is a string at this level. The typed accessors convert on demand
/// and report where a bad value was written, because the reader that knows what
/// a key means is the one that can say what was wrong with it.
namespace kero::kv {

/// Where something is in the source text. One-based, because it is shown to a
/// person and pasted into an editor.
struct Location {
    u32 line = 1;
    u32 column = 1;
};

struct Error {
    std::string message;
    Location where;
    std::string filename;

    /// "kero_start.kmap:12:5: expected '{' after block name 'solid'"
    ///
    /// The shape every compiler has used since cc, so an editor's error-list
    /// parser and a person's eye both already know how to read it.
    [[nodiscard]] std::string format() const;
};

class Block;

/// One quoted `"key" "value"` line.
struct Pair {
    std::string key;
    std::string value;
    Location where;
};

/// A named block: its pairs and its child blocks, both in file order.
class Block {
public:
    std::string name;
    Location where;
    std::vector<Pair> pairs;
    std::vector<Block> children;

    /// The first value for `key`, or null. First rather than last: a duplicated
    /// key in a hand-edited file is a mistake, and taking the first makes the
    /// behaviour stable rather than dependent on how far the file was read.
    [[nodiscard]] const std::string* find(std::string_view key) const;
    [[nodiscard]] bool has(std::string_view key) const { return find(key) != nullptr; }

    [[nodiscard]] std::string_view get(std::string_view key,
                                       std::string_view fallback = {}) const;

    /// Typed reads. Each returns nothing if the key is absent *or* unparseable,
    /// and `location_of` gives the caller what it needs to say which.
    [[nodiscard]] std::optional<f32> get_f32(std::string_view key) const;
    [[nodiscard]] std::optional<f64> get_f64(std::string_view key) const;
    [[nodiscard]] std::optional<i32> get_i32(std::string_view key) const;
    [[nodiscard]] std::optional<bool> get_bool(std::string_view key) const;

    /// Parses "128 -64 32" or "(128 -64 32)" into three numbers.
    [[nodiscard]] std::optional<std::array<f64, 3>> get_vec3(std::string_view key) const;

    [[nodiscard]] std::optional<Location> location_of(std::string_view key) const;

    [[nodiscard]] const Block* first_child(std::string_view child_name) const;
    [[nodiscard]] std::vector<const Block*> children_named(std::string_view child_name) const;
    [[nodiscard]] usize count_children(std::string_view child_name) const;

    void set(std::string_view key, std::string_view value);
};

/// A parsed file: its top-level blocks, in order.
class Document {
public:
    std::vector<Block> blocks;
    std::string filename;

    [[nodiscard]] const Block* first(std::string_view block_name) const;
    [[nodiscard]] std::vector<const Block*> all(std::string_view block_name) const;

    /// Serialises back to text: tabs for indentation, values aligned, in the
    /// order the blocks are held. Round-tripping a file it parsed produces text
    /// a diff tool can compare against the original.
    [[nodiscard]] std::string to_string() const;
};

/// Parses `text`. `filename` is used only for diagnostics.
[[nodiscard]] std::expected<Document, Error> parse(std::string_view text,
                                                   std::string_view filename = "<memory>");

/// Reads and parses a file. A file that will not open is reported the same way
/// a file that will not parse is.
[[nodiscard]] std::expected<Document, Error> parse_file(const std::string& path);

/// Writes a document, creating parent directories as needed.
[[nodiscard]] std::expected<void, Error> write_file(const Document& document,
                                                    const std::string& path);

}  // namespace kero::kv
