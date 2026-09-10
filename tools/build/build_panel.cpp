// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "build/build_panel.hpp"

#include "build/builder.hpp"
#include "common/project.hpp"

#include <imgui.h>

#include <format>

namespace kero::build {
namespace {

ImVec4 colour_of(Line::Kind kind) {
    switch (kind) {
        case Line::Kind::Error:   return ImVec4(1.00f, 0.42f, 0.38f, 1.0f);
        case Line::Kind::Warning: return ImVec4(1.00f, 0.80f, 0.35f, 1.0f);
        case Line::Kind::Heading: return ImVec4(0.62f, 0.80f, 1.00f, 1.0f);
        case Line::Kind::Info:    break;
    }
    return ImGui::GetStyleColorVec4(ImGuiCol_Text);
}

}  // namespace

struct BuildPanel::Impl {
    Builder builder;
    Builder::Options options;
    std::optional<tools::Project> project;
    bool looked_for_project = false;
    bool follow_tail = true;
};

BuildPanel::BuildPanel() : impl_(std::make_unique<Impl>()) {}
BuildPanel::~BuildPanel() = default;

void BuildPanel::draw(shell::Shell& shell) {
    Impl& impl = *impl_;

    // Looked up once, on first draw rather than in the constructor: the working
    // directory is a property of how the tool was launched, and asking for it
    // before there is a window to report the answer in means the answer goes
    // nowhere.
    if (!impl.looked_for_project) {
        impl.project = tools::find_project();
        impl.looked_for_project = true;
    }

    if (ImGui::Begin("Build")) {
        if (!impl.project) {
            ImGui::TextWrapped(
                "No content tree found. Kerosene looks for a .kproj file, and "
                "failing that for a directory with content/maps in it, climbing "
                "from where the toolset was started.");
            if (ImGui::Button("Look again")) {
                impl.looked_for_project = false;
            }
            ImGui::End();
            return;
        }

        const tools::Project& project = *impl.project;
        ImGui::TextUnformatted(project.name.c_str());
        ImGui::TextDisabled("%s", project.provenance().c_str());
        ImGui::Separator();

        const std::vector<std::filesystem::path> maps = project.maps();
        const bool busy = impl.builder.running();

        ImGui::BeginDisabled(busy);
        if (ImGui::Button(maps.size() == 1 ? "Build map" : "Build all maps")) {
            impl.builder.start(maps, impl.options);
        }
        ImGui::SameLine();
        ImGui::Checkbox("Fast", &impl.options.fast);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip(
                "Skip the expensive passes. For a layout that is still moving -- "
                "the level will compile and play, and the visibility will be "
                "coarser than it should be.");
        }
        ImGui::SameLine();
        ImGui::Checkbox("Geometry only", &impl.options.no_visibility);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip(
                "Stop after Cleave. Enough to see whether the geometry compiles "
                "and whether the level is sealed.");
        }
        ImGui::EndDisabled();

        ImGui::SameLine();
        ImGui::TextDisabled("%zu map%s", maps.size(), maps.size() == 1 ? "" : "s");

        if (busy) {
            ImGui::ProgressBar(impl.builder.progress(), ImVec2(-1.0f, 0.0f));
            shell.set_status("building...");
        }

        ImGui::Separator();

        const std::vector<Line> log = impl.builder.log();
        if (ImGui::BeginChild("##log", ImVec2(0.0f, 0.0f), ImGuiChildFlags_Borders)) {
            for (const Line& line : log) {
                ImGui::PushStyleColor(ImGuiCol_Text, colour_of(line.kind));
                if (line.kind == Line::Kind::Heading) {
                    ImGui::Separator();
                }
                ImGui::TextUnformatted(line.text.c_str());
                ImGui::PopStyleColor();
            }
            // Follows the tail while the log is growing, and stops the moment
            // you scroll up to read something -- which is when you want it to
                // stop.
            if (busy && impl.follow_tail) {
                ImGui::SetScrollHereY(1.0f);
            }
            if (ImGui::IsWindowHovered() && ImGui::GetIO().MouseWheel != 0.0f) {
                impl.follow_tail = ImGui::GetScrollY() >= ImGui::GetScrollMaxY() - 1.0f;
            }
            if (!busy) {
                impl.follow_tail = true;
            }
        }
        ImGui::EndChild();
    }
    ImGui::End();
}

}  // namespace kero::build
