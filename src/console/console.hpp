// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
#pragma once

#include "core/types.hpp"

#include <functional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

/// Convars, concommands and the command buffer.
///
/// Everything the engine can be told to do goes through here, and there is
/// deliberately only one path. Console text typed at runtime, a key bind, a
/// line in a `.kcfg` file and a `+argument` on the command line are the same
/// mechanism reached four ways -- which means a setting is discoverable
/// (`cvarlist`, `find`), documented at the point it is declared, and scriptable
/// without anyone having designed a scripting interface for it.
///
/// Source got this right and it is worth copying wholesale. The alternative --
/// a settings struct, a config parser, and a separate debug menu -- ends up
/// with three places to add a knob and two of them forgotten.
namespace kero::console {

enum class VarFlags : u32 {
    None = 0,
    /// Written to the user's config when it changes.
    Archive = 1u << 0,
    /// Refuses to change unless `sv_cheats` is on. For the ones that would
    /// spoil a game rather than configure it.
    Cheat = 1u << 1,
    /// The server's value wins; a client cannot set its own.
    Replicated = 1u << 2,
    /// Cannot be set at all after startup. For things the engine has already
    /// built structures around.
    ReadOnly = 1u << 3,
    /// Announces its change in the console, for values other players' games
    /// depend on.
    Notify = 1u << 4,
};

[[nodiscard]] constexpr VarFlags operator|(VarFlags a, VarFlags b) {
    return static_cast<VarFlags>(static_cast<u32>(a) | static_cast<u32>(b));
}
[[nodiscard]] constexpr VarFlags operator&(VarFlags a, VarFlags b) {
    return static_cast<VarFlags>(static_cast<u32>(a) & static_cast<u32>(b));
}
[[nodiscard]] constexpr bool any(VarFlags value) { return static_cast<u32>(value) != 0; }

/// A named, typed, documented setting.
///
/// Declared at namespace scope next to the code that reads it, so the
/// declaration, the default, the help text and the use are all in one place.
/// The value is cached in its parsed forms because these are read in inner
/// loops -- `sv_gravity` is read every tick for every mover.
class ConVar {
public:
    ConVar(std::string_view name, std::string_view default_value, std::string_view help,
           VarFlags flags = VarFlags::None);

    ConVar(const ConVar&) = delete;
    ConVar& operator=(const ConVar&) = delete;

    [[nodiscard]] std::string_view name() const { return name_; }
    [[nodiscard]] std::string_view help() const { return help_; }
    [[nodiscard]] VarFlags flags() const { return flags_; }
    [[nodiscard]] std::string_view default_value() const { return default_; }

    [[nodiscard]] std::string_view string() const { return value_; }
    [[nodiscard]] f32 number() const { return number_; }
    [[nodiscard]] i32 integer() const { return integer_; }
    [[nodiscard]] bool boolean() const { return integer_ != 0; }

    /// Whether the value differs from the one it was declared with. Used by the
    /// config writer, so a file holds what someone changed rather than a dump
    /// of every default.
    [[nodiscard]] bool modified() const { return value_ != default_; }

    /// Sets from text. Returns false, with a reason, if a flag forbids it.
    bool set(std::string_view value, std::string* reason = nullptr);
    void set(f32 value);
    void set(i32 value);
    void reset() { (void)set(default_); }

    /// Optional bounds, applied on every set. Declared after construction so
    /// the common case stays a one-liner.
    ConVar& with_range(f32 minimum, f32 maximum);

    /// Called after the value changes. The old value is passed because a
    /// callback that has to rebuild something usually needs to know what it is
    /// rebuilding from.
    using Callback = std::function<void(ConVar&, std::string_view previous)>;
    ConVar& on_change(Callback callback);

private:
    void reparse();

    std::string name_;
    std::string help_;
    std::string default_;
    std::string value_;
    VarFlags flags_ = VarFlags::None;

    f32 number_ = 0.0f;
    i32 integer_ = 0;

    bool has_range_ = false;
    f32 minimum_ = 0.0f;
    f32 maximum_ = 0.0f;

    Callback callback_;
};

/// A named action.
class ConCommand {
public:
    /// Arguments, not including the command name itself.
    using Handler = std::function<void(std::span<const std::string> arguments)>;

    ConCommand(std::string_view name, std::string_view help, Handler handler);

    ConCommand(const ConCommand&) = delete;
    ConCommand& operator=(const ConCommand&) = delete;

    [[nodiscard]] std::string_view name() const { return name_; }
    [[nodiscard]] std::string_view help() const { return help_; }
    void run(std::span<const std::string> arguments) const { handler_(arguments); }

private:
    std::string name_;
    std::string help_;
    Handler handler_;
};

[[nodiscard]] ConVar* find_var(std::string_view name);
[[nodiscard]] ConCommand* find_command(std::string_view name);
void for_each_var(const std::function<void(ConVar&)>& fn);
void for_each_command(const std::function<void(const ConCommand&)>& fn);

/// Names beginning with `prefix`, sorted, for completion.
[[nodiscard]] std::vector<std::string> complete(std::string_view prefix);

/// Runs one line: a command, `name value` to set a var, or `name` to print it.
///
/// Returns false only when nothing of that name exists -- which is what lets a
/// caller distinguish "unknown command" from "the command ran and failed".
bool execute_line(std::string_view line);

/// Queues text to run at the next `flush()`.
///
/// The buffer exists because a command that changes what is running must not
/// run inside it. `map kero_start` typed at the console tears down the level
/// the console is being drawn over; deferring it to a known point in the frame
/// is the difference between a level change and a crash.
void enqueue(std::string_view text);

/// Runs everything queued. Commands queued *by* those commands run on the next
/// flush, not this one, so one bad `alias` cannot spin forever.
void flush();

/// Reads a `.kcfg` file and queues every line in it.
[[nodiscard]] bool execute_config_file(const std::string& path, std::string* error = nullptr);

/// Writes the modified archived vars to a `.kcfg` file.
[[nodiscard]] bool write_config_file(const std::string& path);

/// Queues each `+command args` group from a command line, in order.
///
/// `kerosene +map kero_start +sv_gravity 800` -- the same path as the console,
/// which is why anything settable is settable at launch without a flag being
/// added for it.
void execute_command_line(std::span<const std::string> arguments);

/// Splits a line into tokens, honouring double quotes.
[[nodiscard]] std::vector<std::string> tokenise(std::string_view line);

/// Where console output goes. The developer console registers one of these, as
/// does the engine's log bridge.
using Printer = std::function<void(std::string_view)>;
[[nodiscard]] u32 add_printer(Printer printer);
void remove_printer(u32 handle);
void print(std::string_view text);

/// Bridges the log into the console, so engine output appears in it as it
/// happens. Returns a handle to undo it.
[[nodiscard]] u32 attach_log();

}  // namespace kero::console
