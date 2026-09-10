// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "chisel/chisel_panel.hpp"

#include "common/project.hpp"
#include "core/log.hpp"
#include "map/map.hpp"

#include <imgui.h>

#include <format>
#include <optional>

namespace kero::chisel {
namespace {

KERO_LOG_CATEGORY(log, "chisel");

}  // namespace

struct ChiselPanel::Impl {
    std::optional<map::Map> document;
    std::string path;
    std::string status;
    std::optional<tools::Project> project;
    bool looked_for_project = false;
};

ChiselPanel::ChiselPanel() : impl_(std::make_unique<Impl>()) {}
ChiselPanel::~ChiselPanel() = default;

void ChiselPanel::open(const std::string& path) {
    Impl& impl = *impl_;

    auto loaded = map::load(path);
    if (!loaded) {
        // The diagnostic already carries a file, line and column; passing it
        // through unchanged is more useful than wrapping it in a sentence.
        impl.status = loaded.error().format();
        KERO_ERROR(log, "{}", impl.status);
        return;
    }

    impl.document = std::move(*loaded);
    impl.path = path;
    impl.status = std::format("{} brushes, {} entities", impl.document->brush_count(),
                              impl.document->entities.size() + 1);
    KERO_INFO(log, "opened {} -- {}", path, impl.status);
}

bool ChiselPanel::can_close() { return true; }

void ChiselPanel::draw_file_menu(shell::Shell&) {
    Impl& impl = *impl_;

    if (!impl.looked_for_project) {
        impl.project = tools::find_project();
        impl.looked_for_project = true;
    }

    if (ImGui::BeginMenu("Open map")) {
        if (impl.project) {
            const std::vector<std::filesystem::path> maps = impl.project->maps();
            if (maps.empty()) {
                ImGui::TextDisabled("no maps in this project");
            }
            for (const std::filesystem::path& path : maps) {
                if (ImGui::MenuItem(path.filename().string().c_str())) {
                    open(path.string());
                }
            }
        } else {
            ImGui::TextDisabled("no content tree found");
        }
        ImGui::EndMenu();
    }
}

void ChiselPanel::draw(shell::Shell& shell) {
    Impl& impl = *impl_;

    if (ImGui::Begin("Chisel")) {
        if (!impl.document) {
            ImGui::TextWrapped("No map open. File -> Open map.");
            if (!impl.status.empty()) {
                ImGui::TextColored(ImVec4(1.0f, 0.42f, 0.38f, 1.0f), "%s",
                                   impl.status.c_str());
            }
        } else {
            ImGui::TextUnformatted(impl.path.c_str());
            ImGui::TextDisabled("%s", impl.status.c_str());
            shell.set_status(impl.path);
        }
    }
    ImGui::End();
}

void ChiselPanel::render(shell::Shell&, SDL_GPUCommandBuffer*) {}

}  // namespace kero::chisel
