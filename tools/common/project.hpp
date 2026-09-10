// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <filesystem>
#include <optional>
#include <string>
#include <vector>

/// Finding a project and its content tree.
///
/// Every tool and the engine answer this the same way, and say which answer
/// they took. A `.kproj` names the content directory outright; without one the
/// tree is found by climbing for a directory that looks like a content root,
/// which is why a fresh clone needs no setup. The project file is how you
/// overrule the guess.
namespace kero::tools {

struct Project {
    std::string name = "Kerosene";
    std::filesystem::path root;        ///< The directory holding the `.kproj`.
    std::filesystem::path content;     ///< The content tree.
    std::string start_map;             ///< The map the engine loads with no +map.
    std::filesystem::path project_file;  ///< Empty when the tree was guessed.

    /// Every `.kmap` under `content/maps`, sorted, so the build order is the
    /// same on every machine.
    [[nodiscard]] std::vector<std::filesystem::path> maps() const;

    /// Every `.kmat` under `content/materials`, as material names -- the form a
    /// `.kmap` refers to them by, so "dev/wall" rather than a path.
    [[nodiscard]] std::vector<std::string> materials() const;

    /// How the project was found, for the tool to say so.
    [[nodiscard]] std::string provenance() const;
};

/// Searches upward from `start` (the working directory by default).
[[nodiscard]] std::optional<Project> find_project(
    const std::filesystem::path& start = std::filesystem::current_path());

/// The directory the running executable is in, for finding the engine binary
/// beside the toolset.
[[nodiscard]] std::filesystem::path executable_directory();

}  // namespace kero::tools
