// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "common/cli.hpp"

#include <cctype>
#include <charconv>

namespace kero::tools {

Args::Args(int argc, char** argv) {
    int first = 1;
    if (argc > 1 && !is_flag(argv[1])) {
        command_ = argv[1];
        first = 2;
    }
    for (int i = first; i < argc; ++i) {
        tokens_.emplace_back(argv[i]);
    }
    consumed_.assign(tokens_.size(), false);
}

bool Args::is_flag(std::string_view token) {
    if (token.size() < 2 || token[0] != '-') {
        return false;
    }
    // "-1" and "-.5" are values, not flags.
    return !(std::isdigit(static_cast<unsigned char>(token[1])) != 0 || token[1] == '.');
}

std::optional<usize> Args::find(std::string_view name,
                                std::optional<std::string>& inline_value) const {
    inline_value.reset();

    for (usize i = 0; i < tokens_.size(); ++i) {
        if (consumed_[i]) {
            continue;
        }
        std::string_view token = tokens_[i];
        if (!is_flag(token)) {
            continue;
        }
        token.remove_prefix(token.starts_with("--") ? 2 : 1);

        if (token == name) {
            return i;
        }
        if (token.starts_with(name) && token.size() > name.size() &&
            token[name.size()] == '=') {
            inline_value = std::string(token.substr(name.size() + 1));
            return i;
        }
    }
    return std::nullopt;
}

bool Args::flag(std::string_view name) {
    std::optional<std::string> inline_value;
    const std::optional<usize> index = find(name, inline_value);
    if (!index) {
        return false;
    }
    consumed_[*index] = true;
    return true;
}

std::optional<std::string> Args::option(std::string_view name) {
    std::optional<std::string> inline_value;
    const std::optional<usize> index = find(name, inline_value);
    if (!index) {
        return std::nullopt;
    }

    consumed_[*index] = true;
    if (inline_value) {
        return inline_value;
    }
    // Look ahead. A following flag is not this option's value -- that is a
    // missing value, and reporting it as such beats swallowing the next flag.
    const usize next = *index + 1;
    if (next < tokens_.size() && !consumed_[next] && !is_flag(tokens_[next])) {
        consumed_[next] = true;
        return tokens_[next];
    }
    return std::nullopt;
}

std::optional<f64> Args::number(std::string_view name) {
    const std::optional<std::string> text = option(name);
    if (!text) {
        return std::nullopt;
    }
    f64 value{};
    const auto [stop, code] =
        std::from_chars(text->data(), text->data() + text->size(), value);
    if (code != std::errc{} || stop != text->data() + text->size()) {
        return std::nullopt;
    }
    return value;
}

std::vector<std::string> Args::positional() const {
    std::vector<std::string> found;
    for (usize i = 0; i < tokens_.size(); ++i) {
        if (!consumed_[i] && !is_flag(tokens_[i])) {
            found.push_back(tokens_[i]);
        }
    }
    return found;
}

std::vector<std::string> Args::unconsumed() const {
    std::vector<std::string> left;
    for (usize i = 0; i < tokens_.size(); ++i) {
        if (!consumed_[i] && is_flag(tokens_[i])) {
            left.push_back(tokens_[i]);
        }
    }
    return left;
}

}  // namespace kero::tools
