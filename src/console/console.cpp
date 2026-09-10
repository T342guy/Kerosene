// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#include "console/console.hpp"

#include "core/log.hpp"

#include <algorithm>
#include <cstdio>
#include <charconv>
#include <deque>
#include <filesystem>
#include <format>
#include <fstream>
#include <mutex>
#include <sstream>

namespace kero::console {
namespace {

KERO_LOG_CATEGORY(log, "console");

struct Registry {
    std::mutex mutex;
    std::vector<ConVar*> vars;
    std::vector<const ConCommand*> commands;
    std::vector<std::pair<u32, Printer>> printers;
    u32 next_handle = 1;

    /// Commands wait here until flush(). See enqueue().
    std::deque<std::string> pending;
};

Registry& registry() {
    static Registry instance;
    return instance;
}

/// Whether cheat-flagged vars may be changed. Declared here rather than in the
/// game so that the check lives with the flag it enforces.
ConVar sv_cheats("sv_cheats", "0",
                 "Allow cheat-flagged convars to be changed. Off in a real game.",
                 VarFlags::Notify);

f32 parse_number(std::string_view text) {
    f32 value = 0.0f;
    while (!text.empty() && (text.front() == ' ' || text.front() == '\t')) {
        text.remove_prefix(1);
    }
    if (text.empty()) {
        return 0.0f;
    }
    const auto [stop, code] = std::from_chars(text.data(), text.data() + text.size(), value);
    // A partial parse is deliberate here, unlike in the map formats: "1 fast"
    // as a convar value should read as 1, because a person typed it.
    (void)stop;
    return code == std::errc{} ? value : 0.0f;
}

}  // namespace

// ---------------------------------------------------------------------------
// ConVar
// ---------------------------------------------------------------------------

ConVar::ConVar(std::string_view name, std::string_view default_value, std::string_view help,
               VarFlags flags)
    : name_(name), help_(help), default_(default_value), value_(default_value),
      flags_(flags) {
    reparse();
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    reg.vars.push_back(this);
}

void ConVar::reparse() {
    number_ = parse_number(value_);
    integer_ = static_cast<i32>(number_);
}

ConVar& ConVar::with_range(f32 minimum, f32 maximum) {
    has_range_ = true;
    minimum_ = minimum;
    maximum_ = maximum;
    (void)set(value_);  // Clamp the current value into the new range.
    return *this;
}

ConVar& ConVar::on_change(Callback callback) {
    callback_ = std::move(callback);
    return *this;
}

bool ConVar::set(std::string_view value, std::string* reason) {
    if (any(flags_ & VarFlags::ReadOnly)) {
        if (reason != nullptr) {
            *reason = std::format("{} is read-only; it is fixed at startup", name_);
        }
        return false;
    }
    if (any(flags_ & VarFlags::Cheat) && !sv_cheats.boolean() && &sv_cheats != this) {
        if (reason != nullptr) {
            *reason = std::format("{} is a cheat; set sv_cheats 1 first", name_);
        }
        return false;
    }

    std::string wanted(value);
    if (has_range_) {
        const f32 clamped = std::clamp(parse_number(wanted), minimum_, maximum_);
        // Reformatted only when clamping actually changed it, so a value typed
        // as "1" does not come back as "1.000000".
        if (clamped != parse_number(wanted)) {
            wanted = std::format("{:g}", static_cast<f64>(clamped));
            if (reason != nullptr) {
                *reason = std::format("{} clamped to {} (range {:g} to {:g})", name_,
                                      wanted, static_cast<f64>(minimum_),
                                      static_cast<f64>(maximum_));
            }
        }
    }

    if (wanted == value_) {
        return true;
    }

    const std::string previous = value_;
    value_ = std::move(wanted);
    reparse();

    if (any(flags_ & VarFlags::Notify)) {
        print(std::format("{} changed to {}\n", name_, value_));
    }
    if (callback_) {
        callback_(*this, previous);
    }
    return true;
}

void ConVar::set(f32 value) { (void)set(std::format("{:g}", static_cast<f64>(value))); }
void ConVar::set(i32 value) { (void)set(std::format("{}", value)); }

// ---------------------------------------------------------------------------
// ConCommand
// ---------------------------------------------------------------------------

ConCommand::ConCommand(std::string_view name, std::string_view help, Handler handler)
    : name_(name), help_(help), handler_(std::move(handler)) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    reg.commands.push_back(this);
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

ConVar* find_var(std::string_view name) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    for (ConVar* var : reg.vars) {
        if (var->name() == name) {
            return var;
        }
    }
    return nullptr;
}

ConCommand* find_command(std::string_view name) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    for (const ConCommand* command : reg.commands) {
        if (command->name() == name) {
            return const_cast<ConCommand*>(command);
        }
    }
    return nullptr;
}

void for_each_var(const std::function<void(ConVar&)>& fn) {
    Registry& reg = registry();
    std::vector<ConVar*> snapshot;
    {
        std::lock_guard lock(reg.mutex);
        snapshot = reg.vars;
    }
    // Called outside the lock: a callback that prints, or that sets another
    // var, must not deadlock on the registry.
    for (ConVar* var : snapshot) {
        fn(*var);
    }
}

void for_each_command(const std::function<void(const ConCommand&)>& fn) {
    Registry& reg = registry();
    std::vector<const ConCommand*> snapshot;
    {
        std::lock_guard lock(reg.mutex);
        snapshot = reg.commands;
    }
    for (const ConCommand* command : snapshot) {
        fn(*command);
    }
}

std::vector<std::string> complete(std::string_view prefix) {
    std::vector<std::string> names;
    for_each_var([&](ConVar& var) {
        if (var.name().starts_with(prefix)) {
            names.emplace_back(var.name());
        }
    });
    for_each_command([&](const ConCommand& command) {
        if (command.name().starts_with(prefix)) {
            names.emplace_back(command.name());
        }
    });
    std::ranges::sort(names);
    names.erase(std::ranges::unique(names).begin(), names.end());
    return names;
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

u32 add_printer(Printer printer) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    const u32 handle = reg.next_handle++;
    reg.printers.emplace_back(handle, std::move(printer));
    return handle;
}

void remove_printer(u32 handle) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    std::erase_if(reg.printers, [handle](const auto& entry) { return entry.first == handle; });
}

void print(std::string_view text) {
    Registry& reg = registry();
    std::vector<std::pair<u32, Printer>> printers;
    {
        std::lock_guard lock(reg.mutex);
        printers = reg.printers;
    }
    if (printers.empty()) {
        // Nothing has registered yet -- during startup, or in a test. Output
        // that goes nowhere is worse than output in the wrong place.
        std::fputs(std::string(text).c_str(), stdout);
        return;
    }
    for (const auto& [handle, printer] : printers) {
        (void)handle;
        printer(text);
    }
}

u32 attach_log() {
    return add_log_sink([](const LogRecord& record) {
        print(std::format("[{}] {}\n", record.category, record.message));
    });
}

// ---------------------------------------------------------------------------
// Parsing and execution
// ---------------------------------------------------------------------------

std::vector<std::string> tokenise(std::string_view line) {
    std::vector<std::string> tokens;
    usize i = 0;

    while (i < line.size()) {
        while (i < line.size() && (line[i] == ' ' || line[i] == '\t')) {
            ++i;
        }
        if (i >= line.size()) {
            break;
        }
        // A comment runs to the end of the line, so a .kcfg can be annotated.
        if (line[i] == '/' && i + 1 < line.size() && line[i + 1] == '/') {
            break;
        }

        std::string token;
        if (line[i] == '"') {
            // Quoted, so a value can contain spaces: bind f "say hello there".
            ++i;
            while (i < line.size() && line[i] != '"') {
                token.push_back(line[i++]);
            }
            if (i < line.size()) {
                ++i;
            }
        } else {
            while (i < line.size() && line[i] != ' ' && line[i] != '\t') {
                token.push_back(line[i++]);
            }
        }
        tokens.push_back(std::move(token));
    }

    return tokens;
}

bool execute_line(std::string_view line) {
    const std::vector<std::string> tokens = tokenise(line);
    if (tokens.empty()) {
        return true;  // A blank line or a comment is not an error.
    }

    const std::string& name = tokens.front();
    const std::span<const std::string> arguments(tokens.begin() + 1, tokens.end());

    if (const ConCommand* command = find_command(name)) {
        command->run(arguments);
        return true;
    }

    if (ConVar* var = find_var(name)) {
        if (arguments.empty()) {
            // Bare name prints the value, its default and what it is for. The
            // help text is the documentation, so it has to be reachable.
            print(std::format("{} = \"{}\" (default \"{}\")\n{}\n", var->name(),
                              var->string(), var->default_value(), var->help()));
            return true;
        }
        std::string reason;
        if (!var->set(arguments.front(), &reason)) {
            print(std::format("{}\n", reason));
        } else if (!reason.empty()) {
            print(std::format("{}\n", reason));
        }
        return true;
    }

    print(std::format("unknown command: {}\n", name));
    return false;
}

void enqueue(std::string_view text) {
    Registry& reg = registry();
    std::lock_guard lock(reg.mutex);
    std::istringstream stream{std::string(text)};
    std::string line;
    while (std::getline(stream, line)) {
        // Semicolons separate commands on one line, so a bind can hold several.
        usize start = 0;
        for (usize i = 0; i <= line.size(); ++i) {
            if (i == line.size() || line[i] == ';') {
                std::string_view part(line.data() + start, i - start);
                if (!part.empty()) {
                    reg.pending.emplace_back(part);
                }
                start = i + 1;
            }
        }
    }
}

void flush() {
    Registry& reg = registry();

    // Taken all at once. A command that queues more -- `map` queueing the
    // level's own config, an alias expanding -- runs on the next flush rather
    // than this one, so no single line can spin the buffer forever.
    std::deque<std::string> batch;
    {
        std::lock_guard lock(reg.mutex);
        batch.swap(reg.pending);
    }
    for (const std::string& line : batch) {
        (void)execute_line(line);
    }
}

bool execute_config_file(const std::string& path, std::string* error) {
    std::ifstream file(path);
    if (!file) {
        if (error != nullptr) {
            *error = std::format("cannot open {}", path);
        }
        return false;
    }

    std::ostringstream buffer;
    buffer << file.rdbuf();
    enqueue(buffer.str());
    return true;
}

bool write_config_file(const std::string& path) {
    std::error_code code;
    const std::filesystem::path parent = std::filesystem::path(path).parent_path();
    if (!parent.empty()) {
        std::filesystem::create_directories(parent, code);
    }

    std::ofstream file(path, std::ios::trunc);
    if (!file) {
        return false;
    }

    file << "// Written by Kerosene. Values you changed, not a dump of the defaults.\n";
    for_each_var([&file](ConVar& var) {
        if (any(var.flags() & VarFlags::Archive) && var.modified()) {
            file << std::format("{} \"{}\"\n", var.name(), var.string());
        }
    });
    return static_cast<bool>(file);
}

void execute_command_line(std::span<const std::string> arguments) {
    // Each `+name` starts a group and takes the arguments up to the next one,
    // so `+map kero_start +sv_gravity 800` is two commands and needs no
    // quoting.
    std::string current;
    for (const std::string& argument : arguments) {
        if (argument.starts_with('+')) {
            if (!current.empty()) {
                enqueue(current);
            }
            current = argument.substr(1);
            continue;
        }
        if (current.empty()) {
            continue;  // Before any +command: not ours.
        }
        // Quoted on the way in, so an argument containing a space survives.
        current += argument.find(' ') != std::string::npos
                       ? std::format(" \"{}\"", argument)
                       : std::format(" {}", argument);
    }
    if (!current.empty()) {
        enqueue(current);
    }
}

// ---------------------------------------------------------------------------
// The built-in commands
// ---------------------------------------------------------------------------

namespace {

const ConCommand help_command(
    "help", "help <name> -- what a convar or command is for.",
    [](std::span<const std::string> arguments) {
        if (arguments.empty()) {
            print("help <name>. Try `cvarlist`, `cmdlist`, or `find <text>`.\n");
            return;
        }
        if (const ConVar* var = find_var(arguments.front())) {
            print(std::format("{} = \"{}\" (default \"{}\")\n{}\n", var->name(),
                              var->string(), var->default_value(), var->help()));
            return;
        }
        if (const ConCommand* command = find_command(arguments.front())) {
            print(std::format("{}\n{}\n", command->name(), command->help()));
            return;
        }
        print(std::format("no convar or command called {}\n", arguments.front()));
    });

const ConCommand cvarlist_command(
    "cvarlist", "Every convar, with its value.",
    [](std::span<const std::string>) {
        std::vector<std::string> lines;
        for_each_var([&lines](ConVar& var) {
            lines.push_back(std::format("  {:<24} \"{}\"  {}", var.name(), var.string(),
                                        var.help()));
        });
        std::ranges::sort(lines);
        for (const std::string& line : lines) {
            print(line + "\n");
        }
        print(std::format("{} convars\n", lines.size()));
    });

const ConCommand cmdlist_command(
    "cmdlist", "Every command.",
    [](std::span<const std::string>) {
        std::vector<std::string> lines;
        for_each_command([&lines](const ConCommand& command) {
            lines.push_back(std::format("  {:<24} {}", command.name(), command.help()));
        });
        std::ranges::sort(lines);
        for (const std::string& line : lines) {
            print(line + "\n");
        }
        print(std::format("{} commands\n", lines.size()));
    });

const ConCommand find_command_command(
    "find", "find <text> -- convars and commands whose name or help mentions it.",
    [](std::span<const std::string> arguments) {
        if (arguments.empty()) {
            print("find <text>\n");
            return;
        }
        const std::string& needle = arguments.front();
        auto mentions = [&needle](std::string_view name, std::string_view help) {
            return name.find(needle) != std::string_view::npos ||
                   help.find(needle) != std::string_view::npos;
        };

        std::vector<std::string> lines;
        for_each_var([&](ConVar& var) {
            if (mentions(var.name(), var.help())) {
                lines.push_back(std::format("  {:<24} \"{}\"  {}", var.name(),
                                            var.string(), var.help()));
            }
        });
        for_each_command([&](const ConCommand& command) {
            if (mentions(command.name(), command.help())) {
                lines.push_back(std::format("  {:<24} {}", command.name(), command.help()));
            }
        });
        std::ranges::sort(lines);
        for (const std::string& line : lines) {
            print(line + "\n");
        }
        if (lines.empty()) {
            print(std::format("nothing mentions \"{}\"\n", needle));
        }
    });

const ConCommand exec_command(
    "exec", "exec <file.kcfg> -- run a config file.",
    [](std::span<const std::string> arguments) {
        if (arguments.empty()) {
            print("exec <file.kcfg>\n");
            return;
        }
        std::string error;
        if (!execute_config_file(arguments.front(), &error)) {
            print(error + "\n");
        }
    });

const ConCommand reset_command(
    "reset", "reset <convar> -- put it back to its default.",
    [](std::span<const std::string> arguments) {
        if (arguments.empty()) {
            print("reset <convar>\n");
            return;
        }
        if (ConVar* var = find_var(arguments.front())) {
            var->reset();
            print(std::format("{} = \"{}\"\n", var->name(), var->string()));
            return;
        }
        print(std::format("no convar called {}\n", arguments.front()));
    });

const ConCommand echo_command("echo", "echo <text> -- print it.",
                              [](std::span<const std::string> arguments) {
                                  std::string line;
                                  for (const std::string& argument : arguments) {
                                      if (!line.empty()) {
                                          line += ' ';
                                      }
                                      line += argument;
                                  }
                                  print(line + "\n");
                              });

}  // namespace

}  // namespace kero::console
