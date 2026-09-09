// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "kv/keyvalues.hpp"

#include <charconv>
#include <filesystem>
#include <format>
#include <fstream>
#include <sstream>

namespace kero::kv {
namespace {

/// How deep blocks may nest.
///
/// Not a limit anyone will meet by accident -- a map is three levels deep -- but
/// the parser recurses, and a file consisting of nothing but open braces would
/// otherwise overflow the stack. A corrupt file must produce a diagnostic, not
/// a crash, because the file may well have arrived from somewhere untrusted.
constexpr u32 kMaxDepth = 64;

class Parser {
public:
    Parser(std::string_view text, std::string_view filename)
        : text_(text), filename_(filename) {}

    std::expected<Document, Error> parse_document() {
        Document document;
        document.filename = std::string(filename_);

        for (;;) {
            skip_trivia();
            if (at_end()) {
                break;
            }
            if (peek() == '}') {
                return std::unexpected(error("unmatched '}' with no block open"));
            }

            auto block = parse_block(0);
            if (!block) {
                return std::unexpected(block.error());
            }
            document.blocks.push_back(std::move(*block));
        }
        return document;
    }

private:
    [[nodiscard]] bool at_end() const { return position_ >= text_.size(); }
    [[nodiscard]] char peek() const { return text_[position_]; }

    char advance() {
        const char c = text_[position_++];
        if (c == '\n') {
            ++line_;
            column_ = 1;
        } else {
            ++column_;
        }
        return c;
    }

    [[nodiscard]] Location here() const { return Location{line_, column_}; }

    [[nodiscard]] Error error(std::string message) const {
        return Error{std::move(message), here(), std::string(filename_)};
    }
    [[nodiscard]] Error error_at(std::string message, Location where) const {
        return Error{std::move(message), where, std::string(filename_)};
    }

    void skip_trivia() {
        for (;;) {
            while (!at_end() && (peek() == ' ' || peek() == '\t' ||
                                 peek() == '\r' || peek() == '\n')) {
                advance();
            }
            // Line comments only. A block comment would need an end delimiter
            // that could appear inside a quoted material path, and the format
            // gains nothing from it.
            if (position_ + 1 < text_.size() && peek() == '/' && text_[position_ + 1] == '/') {
                while (!at_end() && peek() != '\n') {
                    advance();
                }
                continue;
            }
            return;
        }
    }

    std::expected<std::string, Error> parse_quoted() {
        const Location start = here();
        advance();  // The opening quote.

        std::string value;
        while (!at_end() && peek() != '"') {
            char c = advance();
            if (c != '\\' || at_end()) {
                if (c == '\n') {
                    return std::unexpected(error_at("string is missing its closing quote", start));
                }
                value.push_back(c);
                continue;
            }

            const char escaped = advance();
            switch (escaped) {
                case 'n':  value.push_back('\n'); break;
                case 't':  value.push_back('\t'); break;
                case '"':  value.push_back('"');  break;
                case '\\': value.push_back('\\'); break;
                default:
                    // A backslash before anything else is literal, so a Windows
                    // path pasted into a material key survives being read back.
                    value.push_back('\\');
                    value.push_back(escaped);
                    break;
            }
        }

        if (at_end()) {
            return std::unexpected(error_at("string is missing its closing quote", start));
        }
        advance();  // The closing quote.
        return value;
    }

    /// A block name: bare, up to whitespace or a brace.
    std::expected<std::string, Error> parse_name() {
        if (peek() == '"') {
            return parse_quoted();
        }

        std::string name;
        while (!at_end() && peek() != ' ' && peek() != '\t' && peek() != '\r' &&
               peek() != '\n' && peek() != '{' && peek() != '}' && peek() != '"') {
            name.push_back(advance());
        }
        if (name.empty()) {
            return std::unexpected(error(std::format("expected a name, found '{}'", peek())));
        }
        return name;
    }

    std::expected<Block, Error> parse_block(u32 depth) {
        if (depth >= kMaxDepth) {
            return std::unexpected(error(
                std::format("blocks nested more than {} deep; the file is probably not KeyValues",
                            kMaxDepth)));
        }

        Block block;
        block.where = here();

        auto name = parse_name();
        if (!name) {
            return std::unexpected(name.error());
        }
        block.name = std::move(*name);

        skip_trivia();
        if (at_end() || peek() != '{') {
            return std::unexpected(error(
                std::format("expected '{{' after block name '{}'", block.name)));
        }
        advance();

        for (;;) {
            skip_trivia();
            if (at_end()) {
                return std::unexpected(error_at(
                    std::format("block '{}' is never closed", block.name), block.where));
            }
            if (peek() == '}') {
                advance();
                return block;
            }

            if (peek() == '"') {
                const Location key_where = here();
                auto key = parse_quoted();
                if (!key) {
                    return std::unexpected(key.error());
                }

                skip_trivia();
                if (at_end() || peek() != '"') {
                    return std::unexpected(error(
                        std::format("key '{}' has no value; expected a quoted string", *key)));
                }
                auto value = parse_quoted();
                if (!value) {
                    return std::unexpected(value.error());
                }

                block.pairs.push_back(Pair{std::move(*key), std::move(*value), key_where});
                continue;
            }

            auto child = parse_block(depth + 1);
            if (!child) {
                return std::unexpected(child.error());
            }
            block.children.push_back(std::move(*child));
        }
    }

    std::string_view text_;
    std::string_view filename_;
    usize position_ = 0;
    u32 line_ = 1;
    u32 column_ = 1;
};

void write_escaped(std::ostringstream& out, std::string_view value) {
    out << '"';
    for (char c : value) {
        switch (c) {
            case '"':  out << "\\\""; break;
            case '\\': out << "\\\\"; break;
            case '\n': out << "\\n";  break;
            case '\t': out << "\\t";  break;
            default:   out << c;      break;
        }
    }
    out << '"';
}

void write_block(std::ostringstream& out, const Block& block, u32 depth) {
    const std::string indent(depth, '\t');
    out << indent << block.name << '\n' << indent << "{\n";

    // Values are aligned to the longest key in this block, which is what makes
    // a hand-edited map readable. Aligned per block rather than per file: one
    // long material path should not indent every other block in the map.
    usize widest = 0;
    for (const Pair& pair : block.pairs) {
        widest = std::max(widest, pair.key.size());
    }

    for (const Pair& pair : block.pairs) {
        out << indent << '\t';
        write_escaped(out, pair.key);
        out << std::string(widest - pair.key.size() + 1, ' ');
        write_escaped(out, pair.value);
        out << '\n';
    }

    for (const Block& child : block.children) {
        write_block(out, child, depth + 1);
    }

    out << indent << "}\n";
}

}  // namespace

std::string Error::format() const {
    return std::format("{}:{}:{}: {}", filename, where.line, where.column, message);
}

const std::string* Block::find(std::string_view key) const {
    for (const Pair& pair : pairs) {
        if (pair.key == key) {
            return &pair.value;
        }
    }
    return nullptr;
}

std::string_view Block::get(std::string_view key, std::string_view fallback) const {
    const std::string* value = find(key);
    return value != nullptr ? std::string_view(*value) : fallback;
}

std::optional<Location> Block::location_of(std::string_view key) const {
    for (const Pair& pair : pairs) {
        if (pair.key == key) {
            return pair.where;
        }
    }
    return std::nullopt;
}

namespace {

/// from_chars rather than stof: no locale, no exceptions, and it rejects
/// trailing junk instead of silently taking a prefix. "12abc" is a mistake in a
/// map file and should be reported as one, not read as 12.
template <typename T>
std::optional<T> parse_number(std::string_view text) {
    while (!text.empty() && (text.front() == ' ' || text.front() == '\t')) {
        text.remove_prefix(1);
    }
    while (!text.empty() && (text.back() == ' ' || text.back() == '\t')) {
        text.remove_suffix(1);
    }
    if (text.empty()) {
        return std::nullopt;
    }

    T value{};
    const char* begin = text.data();
    const char* end = text.data() + text.size();
    const auto [stop, code] = std::from_chars(begin, end, value);
    if (code != std::errc{} || stop != end) {
        return std::nullopt;
    }
    return value;
}

}  // namespace

std::optional<f32> Block::get_f32(std::string_view key) const {
    const std::string* value = find(key);
    return value != nullptr ? parse_number<f32>(*value) : std::nullopt;
}

std::optional<f64> Block::get_f64(std::string_view key) const {
    const std::string* value = find(key);
    return value != nullptr ? parse_number<f64>(*value) : std::nullopt;
}

std::optional<i32> Block::get_i32(std::string_view key) const {
    const std::string* value = find(key);
    return value != nullptr ? parse_number<i32>(*value) : std::nullopt;
}

std::optional<bool> Block::get_bool(std::string_view key) const {
    const std::string* value = find(key);
    if (value == nullptr) {
        return std::nullopt;
    }
    if (*value == "1" || *value == "true")  return true;
    if (*value == "0" || *value == "false") return false;
    return std::nullopt;
}

std::optional<std::array<f64, 3>> Block::get_vec3(std::string_view key) const {
    const std::string* value = find(key);
    if (value == nullptr) {
        return std::nullopt;
    }

    std::array<f64, 3> result{};
    usize index = 0;
    usize position = 0;
    const std::string_view text = *value;

    while (position < text.size() && index < 3) {
        // Brackets and parentheses are separators. The .kmap plane and texture
        // axis keys wrap their numbers in them, and a reader should not have to
        // know which of the two a given key uses.
        while (position < text.size() &&
               (text[position] == ' ' || text[position] == '\t' || text[position] == '(' ||
                text[position] == ')' || text[position] == '[' || text[position] == ']' ||
                text[position] == ',')) {
            ++position;
        }
        const usize start = position;
        while (position < text.size() && text[position] != ' ' && text[position] != '\t' &&
               text[position] != '(' && text[position] != ')' && text[position] != '[' &&
               text[position] != ']' && text[position] != ',') {
            ++position;
        }
        if (start == position) {
            break;
        }

        const std::optional<f64> number = parse_number<f64>(text.substr(start, position - start));
        if (!number) {
            return std::nullopt;
        }
        result[index++] = *number;
    }

    return index == 3 ? std::optional(result) : std::nullopt;
}

void Block::set(std::string_view key, std::string_view value) {
    for (Pair& pair : pairs) {
        if (pair.key == key) {
            pair.value = std::string(value);
            return;
        }
    }
    pairs.push_back(Pair{std::string(key), std::string(value), Location{}});
}

const Block* Block::first_child(std::string_view child_name) const {
    for (const Block& child : children) {
        if (child.name == child_name) {
            return &child;
        }
    }
    return nullptr;
}

std::vector<const Block*> Block::children_named(std::string_view child_name) const {
    std::vector<const Block*> found;
    for (const Block& child : children) {
        if (child.name == child_name) {
            found.push_back(&child);
        }
    }
    return found;
}

usize Block::count_children(std::string_view child_name) const {
    usize count = 0;
    for (const Block& child : children) {
        if (child.name == child_name) {
            ++count;
        }
    }
    return count;
}

const Block* Document::first(std::string_view block_name) const {
    for (const Block& block : blocks) {
        if (block.name == block_name) {
            return &block;
        }
    }
    return nullptr;
}

std::vector<const Block*> Document::all(std::string_view block_name) const {
    std::vector<const Block*> found;
    for (const Block& block : blocks) {
        if (block.name == block_name) {
            found.push_back(&block);
        }
    }
    return found;
}

std::string Document::to_string() const {
    std::ostringstream out;
    for (const Block& block : blocks) {
        write_block(out, block, 0);
    }
    return out.str();
}

std::expected<Document, Error> parse(std::string_view text, std::string_view filename) {
    Parser parser(text, filename);
    return parser.parse_document();
}

std::expected<Document, Error> parse_file(const std::string& path) {
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::unexpected(Error{"cannot open file", Location{}, path});
    }

    std::ostringstream buffer;
    buffer << file.rdbuf();
    const std::string text = buffer.str();

    auto document = parse(text, path);
    if (document) {
        document->filename = path;
    }
    return document;
}

std::expected<void, Error> write_file(const Document& document, const std::string& path) {
    std::error_code code;
    const std::filesystem::path parent = std::filesystem::path(path).parent_path();
    if (!parent.empty()) {
        std::filesystem::create_directories(parent, code);
    }

    std::ofstream file(path, std::ios::binary | std::ios::trunc);
    if (!file) {
        return std::unexpected(Error{"cannot open file for writing", Location{}, path});
    }

    const std::string text = document.to_string();
    file.write(text.data(), static_cast<std::streamsize>(text.size()));
    if (!file) {
        return std::unexpected(Error{"write failed", Location{}, path});
    }
    return {};
}

}  // namespace kero::kv
