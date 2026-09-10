// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "common/project.hpp"

#include "core/log.hpp"
#include "kv/keyvalues.hpp"

#include <algorithm>
#include <format>

#if defined(_WIN32)
#    include <windows.h>
#elif defined(__APPLE__)
#    include <mach-o/dyld.h>
#else
#    include <unistd.h>
#endif

namespace kero::tools {
namespace {

KERO_LOG_CATEGORY(log, "project");

namespace fs = std::filesystem;

/// How far to climb before giving up. Deep enough for any sane checkout, and
/// finite so a tool started at the filesystem root does not walk the whole
/// machine looking for a level.
constexpr int kMaxDepth = 8;

}  // namespace

std::vector<fs::path> Project::maps() const {
    std::vector<fs::path> found;
    std::error_code code;

    const fs::path directory = content / "maps";
    for (const fs::directory_entry& entry : fs::directory_iterator(directory, code)) {
        if (entry.path().extension() == ".kmap") {
            found.push_back(entry.path());
        }
    }

    // Sorted, so a project builds in the same order on every machine and a
    // build log can be diffed against another one.
    std::ranges::sort(found);
    return found;
}

std::vector<std::string> Project::materials() const {
    std::vector<std::string> found;
    std::error_code code;

    const fs::path directory = content / "materials";
    for (const fs::directory_entry& entry :
         fs::recursive_directory_iterator(directory, code)) {
        if (entry.path().extension() != ".kmat") {
            continue;
        }
        // Stored as the name a map refers to it by -- "dev/wall", not a path --
        // because that is what a `.kmap` holds and what the compiler looks up.
        fs::path relative = fs::relative(entry.path(), directory, code);
        relative.replace_extension();
        std::string material = relative.generic_string();
        if (!material.empty()) {
            found.push_back(std::move(material));
        }
    }

    std::ranges::sort(found);
    return found;
}

std::string Project::provenance() const {
    if (!project_file.empty()) {
        return std::format("{} (from {})", content.string(), project_file.string());
    }
    return std::format("{} (found by climbing; no .kproj)", content.string());
}

std::optional<Project> find_project(const fs::path& start) {
    std::error_code code;
    fs::path directory = fs::absolute(start, code);

    for (int depth = 0; depth < kMaxDepth && !directory.empty(); ++depth) {
        for (const fs::directory_entry& entry : fs::directory_iterator(directory, code)) {
            if (entry.path().extension() != ".kproj") {
                continue;
            }

            auto document = kv::parse_file(entry.path().string());
            if (!document) {
                KERO_WARN(log, "{}", document.error().format());
                continue;
            }
            const kv::Block* block = document->first("project");
            if (block == nullptr) {
                continue;
            }

            Project project;
            project.project_file = entry.path();
            project.root = directory;
            project.name = block->get("name", "Kerosene");
            project.start_map = block->get("startmap");
            project.content = directory / std::string(block->get("content", "content"));

            if (!fs::is_directory(project.content, code)) {
                KERO_WARN(log, "{} names a content directory that is not there: {}",
                          entry.path().string(), project.content.string());
                continue;
            }
            KERO_INFO(log, "project {}: {}", project.name, project.provenance());
            return project;
        }

        // No project file here. A directory with a maps subdirectory in it is a
        // content root, which is the guess that makes a fresh clone work.
        const fs::path content = directory / "content";
        if (fs::is_directory(content / "maps", code)) {
            Project project;
            project.root = directory;
            project.content = content;
            KERO_INFO(log, "project: {}", project.provenance());
            return project;
        }

        if (!directory.has_parent_path() || directory.parent_path() == directory) {
            break;
        }
        directory = directory.parent_path();
    }

    return std::nullopt;
}

fs::path executable_directory() {
    std::error_code code;

#if defined(_WIN32)
    wchar_t buffer[MAX_PATH]{};
    const DWORD length = GetModuleFileNameW(nullptr, buffer, MAX_PATH);
    if (length > 0) {
        return fs::path(std::wstring(buffer, length)).parent_path();
    }
#elif defined(__APPLE__)
    char buffer[4096]{};
    std::uint32_t size = sizeof(buffer);
    if (_NSGetExecutablePath(buffer, &size) == 0) {
        return fs::canonical(fs::path(buffer), code).parent_path();
    }
#else
    const fs::path self = fs::read_symlink("/proc/self/exe", code);
    if (!code) {
        return self.parent_path();
    }
#endif

    return fs::current_path(code);
}

}  // namespace kero::tools
