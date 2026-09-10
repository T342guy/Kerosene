// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <optional>
#include <string>
#include <string_view>
#include <vector>

/// Argument parsing shared by every stage.
///
/// Deliberately small. Each stage is a subcommand with a handful of flags, and
/// the value in sharing this is consistency -- `--fast` means the same thing
/// everywhere, `-h` works everywhere, and an unknown flag is an error rather
/// than being ignored, which is what stops a typo in a build script from
/// quietly compiling a level differently for a month.
///
/// The subcommand comes first, as in `git` or `cargo`. Everything after it may
/// appear in any order, and `--name value` and `--name=value` are both
/// accepted -- the first because it is what people type, the second because it
/// is unambiguous when the value looks like a flag.
namespace kero::tools {

class Args {
public:
    Args(int argc, char** argv);

    /// The subcommand: the first argument, when it is not a flag.
    [[nodiscard]] std::string_view command() const { return command_; }

    /// Whether `--name` (or `-name`) was given. Consumes it.
    [[nodiscard]] bool flag(std::string_view name);

    /// The value of `--name value` or `--name=value`. Consumes both tokens.
    [[nodiscard]] std::optional<std::string> option(std::string_view name);

    /// As `option`, but parsed as a number. Nothing if absent or unparseable --
    /// the caller distinguishes the two by asking `option` as well when it
    /// matters.
    [[nodiscard]] std::optional<f64> number(std::string_view name);

    /// Arguments that are not flags and were not consumed as an option's value.
    /// Call after the options have been read.
    [[nodiscard]] std::vector<std::string> positional() const;

    /// Flags nobody asked for. Reporting these rather than ignoring them is the
    /// whole point of the class.
    [[nodiscard]] std::vector<std::string> unconsumed() const;

private:
    /// Whether `token` looks like a flag rather than a value. A negative number
    /// is a value: `--split-cost -1` has to work.
    [[nodiscard]] static bool is_flag(std::string_view token);

    /// The index of `--name` or `-name`, or nothing. `inline_value` is set when
    /// the token carried its value after an `=`.
    [[nodiscard]] std::optional<usize> find(std::string_view name,
                                            std::optional<std::string>& inline_value) const;

    std::string command_;
    std::vector<std::string> tokens_;
    std::vector<bool> consumed_;
};

}  // namespace kero::tools
