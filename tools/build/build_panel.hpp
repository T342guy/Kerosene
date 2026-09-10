// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "shell/shell.hpp"

#include <memory>
#include <optional>

/// The build panel: the whole pipeline over a project, with a log.
namespace kero::build {

class BuildPanel : public shell::Panel {
public:
    BuildPanel();
    ~BuildPanel() override;

    [[nodiscard]] std::string_view name() const override { return "Build"; }
    [[nodiscard]] std::string_view summary() const override {
        return "Compile a project's maps. The same stages F9 runs.";
    }

    void draw(shell::Shell& shell) override;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

}  // namespace kero::build
