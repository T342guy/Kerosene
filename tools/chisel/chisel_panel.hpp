// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "shell/shell.hpp"

#include <memory>
#include <string>

/// Chisel -- the world editor.
namespace kero::chisel {

class ChiselPanel : public shell::Panel {
public:
    ChiselPanel();
    ~ChiselPanel() override;

    [[nodiscard]] std::string_view name() const override { return "Chisel"; }
    [[nodiscard]] std::string_view summary() const override {
        return "The world editor. Draw brushes, place entities, compile and run.";
    }

    /// Opens a `.kmap`. Reports through the panel's own status line rather than
    /// returning, because by the time this is called there is a window to say
    /// it in.
    void open(const std::string& path);

    void draw(shell::Shell& shell) override;
    void render(shell::Shell& shell, SDL_GPUCommandBuffer* command) override;
    [[nodiscard]] bool can_close() override;
    void draw_file_menu(shell::Shell& shell) override;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

}  // namespace kero::chisel
