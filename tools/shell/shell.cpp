// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "shell/shell.hpp"

#include "core/log.hpp"

#include <SDL3/SDL.h>
#include <imgui.h>
#include <imgui_impl_sdl3.h>
#include <imgui_impl_sdlgpu3.h>

#include <format>

namespace kero::shell {
namespace {

KERO_LOG_CATEGORY(log, "shell");

/// How wide the rail down the left edge is.
constexpr f32 kRailWidth = 92.0f;

/// Where ImGui saves the window layout.
///
/// Not the working directory. ImGui defaults to writing `imgui.ini` wherever it
/// was launched from, which for a tool run out of a build tree means dropping a
/// file into the repository -- and a layout that depends on where you started
/// the program from is not a layout anyone can rely on.
std::string layout_path() {
    char* preferences = SDL_GetPrefPath("Kerosene", "Toolset");
    if (preferences == nullptr) {
        return "kerosene-tools-layout.ini";
    }
    std::string path = std::string(preferences) + "layout.ini";
    SDL_free(preferences);
    return path;
}

}  // namespace

struct Shell::Impl {
    SDL_Window* window = nullptr;
    SDL_GPUDevice* device = nullptr;
    std::string layout;
    bool imgui_ready = false;

    ~Impl() {
        if (imgui_ready) {
            ImGui_ImplSDLGPU3_Shutdown();
            ImGui_ImplSDL3_Shutdown();
            ImGui::DestroyContext();
        }
        if (device != nullptr) {
            SDL_WaitForGPUIdle(device);
            if (window != nullptr) {
                SDL_ReleaseWindowFromGPUDevice(device, window);
            }
            SDL_DestroyGPUDevice(device);
        }
        if (window != nullptr) {
            SDL_DestroyWindow(window);
        }
    }
};

Shell::Shell(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Shell::~Shell() = default;

SDL_GPUDevice* Shell::device() const { return impl_->device; }
SDL_Window* Shell::window() const { return impl_->window; }

u32 Shell::colour_format() const {
    return static_cast<u32>(SDL_GetGPUSwapchainTextureFormat(impl_->device, impl_->window));
}

std::expected<std::unique_ptr<Shell>, std::string> Shell::create(std::string_view title,
                                                                 i32 width, i32 height) {
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        return std::unexpected(std::format("SDL_Init failed: {}", SDL_GetError()));
    }

    auto impl = std::make_unique<Impl>();

    impl->window = SDL_CreateWindow(std::string(title).c_str(), width, height,
                                    SDL_WINDOW_RESIZABLE | SDL_WINDOW_HIDDEN);
    if (impl->window == nullptr) {
        return std::unexpected(std::format("could not open a window: {}", SDL_GetError()));
    }

    // The same device the engine's renderer creates, from the same SPIR-V. One
    // graphics path for the whole project rather than a second one for the
    // tools.
    impl->device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_SPIRV, false, nullptr);
    if (impl->device == nullptr) {
        return std::unexpected(std::format(
            "no usable GPU device: {}. A Vulkan driver is needed on Linux",
            SDL_GetError()));
    }
    if (!SDL_ClaimWindowForGPUDevice(impl->device, impl->window)) {
        return std::unexpected(std::format(
            "could not attach the window to the GPU device: {}", SDL_GetError()));
    }

    KERO_INFO(log, "GPU backend: {}", SDL_GetGPUDeviceDriver(impl->device));

    IMGUI_CHECKVERSION();
    ImGui::CreateContext();
    ImGuiIO& io = ImGui::GetIO();
    io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
    io.ConfigFlags |= ImGuiConfigFlags_DockingEnable;

    impl->layout = layout_path();
    io.IniFilename = impl->layout.c_str();

    ImGui::StyleColorsDark();
    // Squared off rather than rounded. A tool should look like an instrument.
    ImGuiStyle& style = ImGui::GetStyle();
    style.WindowRounding = 0.0f;
    style.FrameRounding = 2.0f;
    style.TabRounding = 0.0f;
    style.WindowBorderSize = 1.0f;

    if (!ImGui_ImplSDL3_InitForSDLGPU(impl->window)) {
        return std::unexpected("could not initialise the ImGui SDL3 backend");
    }

    ImGui_ImplSDLGPU3_InitInfo init{};
    init.Device = impl->device;
    init.ColorTargetFormat = SDL_GetGPUSwapchainTextureFormat(impl->device, impl->window);
    init.MSAASamples = SDL_GPU_SAMPLECOUNT_1;
    if (!ImGui_ImplSDLGPU3_Init(&init)) {
        return std::unexpected("could not initialise the ImGui SDL_GPU backend");
    }
    impl->imgui_ready = true;

    SDL_ShowWindow(impl->window);
    return std::unique_ptr<Shell>(new Shell(std::move(impl)));
}

void Shell::add_panel(std::unique_ptr<Panel> panel) {
    panels_.push_back(std::move(panel));
}

void Shell::draw_menu_bar() {
    if (!ImGui::BeginMainMenuBar()) {
        return;
    }

    if (ImGui::BeginMenu("File")) {
        if (active_ < panels_.size()) {
            panels_[active_]->draw_file_menu(*this);
            ImGui::Separator();
        }
        if (ImGui::MenuItem("Quit", "Ctrl+Q")) {
            quitting_ = true;
        }
        ImGui::EndMenu();
    }

    if (ImGui::BeginMenu("Tool")) {
        for (usize i = 0; i < panels_.size(); ++i) {
            const bool selected = i == active_;
            if (ImGui::MenuItem(std::string(panels_[i]->name()).c_str(), nullptr,
                                selected)) {
                active_ = i;
            }
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("%s", std::string(panels_[i]->summary()).c_str());
            }
        }
        ImGui::EndMenu();
    }

    // The status line sits at the right of the menu bar rather than in a strip
    // along the bottom: it is where the eye already is when a menu was just
    // used, and it costs no vertical space in a window that is mostly viewport.
    if (!status_.empty()) {
        const f32 width = ImGui::CalcTextSize(status_.c_str()).x;
        ImGui::SameLine(ImGui::GetWindowWidth() - width - 16.0f);
        ImGui::TextUnformatted(status_.c_str());
    }

    ImGui::EndMainMenuBar();
}

void Shell::draw_rail() {
    const ImGuiViewport* viewport = ImGui::GetMainViewport();
    const f32 menu_height = ImGui::GetFrameHeight();

    ImGui::SetNextWindowPos(
        ImVec2(viewport->WorkPos.x, viewport->WorkPos.y));
    ImGui::SetNextWindowSize(ImVec2(kRailWidth, viewport->WorkSize.y));

    constexpr ImGuiWindowFlags flags =
        ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize |
        ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoScrollbar |
        ImGuiWindowFlags_NoSavedSettings | ImGuiWindowFlags_NoBringToFrontOnFocus;

    if (ImGui::Begin("##rail", nullptr, flags)) {
        for (usize i = 0; i < panels_.size(); ++i) {
            const bool selected = i == active_;
            if (selected) {
                ImGui::PushStyleColor(ImGuiCol_Button,
                                      ImGui::GetStyleColorVec4(ImGuiCol_ButtonActive));
            }
            if (ImGui::Button(std::string(panels_[i]->name()).c_str(),
                              ImVec2(-1.0f, 40.0f))) {
                active_ = i;
            }
            if (selected) {
                ImGui::PopStyleColor();
            }
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("%s", std::string(panels_[i]->summary()).c_str());
            }
        }
    }
    ImGui::End();
    (void)menu_height;
}

int Shell::run(i64 frame_limit) {
    if (panels_.empty()) {
        return 0;
    }

    i64 frames = 0;

    while (!quitting_) {
        if (frame_limit >= 0 && frames >= frame_limit) {
            break;
        }
        ++frames;

        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            ImGui_ImplSDL3_ProcessEvent(&event);
            if (event.type == SDL_EVENT_QUIT) {
                quitting_ = true;
            }
            if (event.type == SDL_EVENT_WINDOW_CLOSE_REQUESTED &&
                event.window.windowID == SDL_GetWindowID(impl_->window)) {
                quitting_ = true;
            }
        }

        // A panel with unsaved work gets to refuse, and to put its own dialog
        // up while it does.
        if (quitting_) {
            for (const std::unique_ptr<Panel>& panel : panels_) {
                if (!panel->can_close()) {
                    quitting_ = false;
                    break;
                }
            }
        }

        if ((SDL_GetWindowFlags(impl_->window) & SDL_WINDOW_MINIMIZED) != 0) {
            SDL_Delay(10);
            continue;
        }

        ImGui_ImplSDLGPU3_NewFrame();
        ImGui_ImplSDL3_NewFrame();
        ImGui::NewFrame();

        status_.clear();

        // A full-window dockspace behind everything, so a panel's windows dock
        // into the shell rather than floating over an empty background.
        const ImGuiViewport* viewport = ImGui::GetMainViewport();
        ImGui::SetNextWindowPos(ImVec2(viewport->WorkPos.x + kRailWidth,
                                       viewport->WorkPos.y));
        ImGui::SetNextWindowSize(ImVec2(viewport->WorkSize.x - kRailWidth,
                                        viewport->WorkSize.y));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.0f, 0.0f));
        ImGui::Begin("##dockhost", nullptr,
                     ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize |
                         ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoBringToFrontOnFocus |
                         ImGuiWindowFlags_NoNavFocus | ImGuiWindowFlags_NoSavedSettings |
                         ImGuiWindowFlags_NoDocking);
        ImGui::DockSpace(ImGui::GetID("kerosene_dockspace"), ImVec2(0.0f, 0.0f),
                         ImGuiDockNodeFlags_PassthruCentralNode);
        ImGui::End();
        ImGui::PopStyleVar();

        draw_menu_bar();
        draw_rail();

        if (active_ < panels_.size()) {
            panels_[active_]->draw(*this);
        }

        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Q)) {
            quitting_ = true;
        }

        ImGui::Render();
        ImDrawData* draw_data = ImGui::GetDrawData();

        SDL_GPUCommandBuffer* command = SDL_AcquireGPUCommandBuffer(impl_->device);
        if (command == nullptr) {
            continue;
        }

        // Panels record their own passes first -- a viewport rendering into its
        // texture -- so what the UI then samples is this frame's picture rather
        // than the last one's.
        if (active_ < panels_.size()) {
            panels_[active_]->render(*this, command);
        }

        SDL_GPUTexture* swapchain = nullptr;
        u32 width = 0;
        u32 height = 0;
        if (!SDL_WaitAndAcquireGPUSwapchainTexture(command, impl_->window, &swapchain,
                                                   &width, &height) ||
            swapchain == nullptr) {
            // Being resized. The command buffer still has to go somewhere.
            SDL_SubmitGPUCommandBuffer(command);
            continue;
        }

        ImGui_ImplSDLGPU3_PrepareDrawData(draw_data, command);

        SDL_GPUColorTargetInfo target{};
        target.texture = swapchain;
        target.clear_color = SDL_FColor{0.09f, 0.09f, 0.11f, 1.0f};
        target.load_op = SDL_GPU_LOADOP_CLEAR;
        target.store_op = SDL_GPU_STOREOP_STORE;

        SDL_GPURenderPass* pass = SDL_BeginGPURenderPass(command, &target, 1, nullptr);
        ImGui_ImplSDLGPU3_RenderDrawData(draw_data, command, pass);
        SDL_EndGPURenderPass(pass);

        SDL_SubmitGPUCommandBuffer(command);
    }

    // Nothing may be destroyed while the GPU is still reading it.
    SDL_WaitForGPUIdle(impl_->device);
    KERO_INFO(log, "{} frames", frames);
    return 0;
}

}  // namespace kero::shell
