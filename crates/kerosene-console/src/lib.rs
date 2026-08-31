// SPDX-License-Identifier: MPL-2.0
//! The console: convars, concommands, and the command buffer.
//!
//! Source's defining trait is that nearly every knob in the engine is a
//! `ConVar` and nearly every action is a `ConCommand`, so the same string
//! typed at a console, bound to a key, written in a `.cfg`, or sent by the
//! server all take the identical path. Kerosene keeps that, because the
//! tools depend on it: Chisel drives a live engine by pushing console text at
//! it, and the compilers read their tuning from the same convar table.
//!
//! ```
//! # use kerosene_console::{Console, ConVarFlags};
//! let mut con = Console::new();
//! con.register_cvar("sv_gravity", "800", ConVarFlags::REPLICATED, "World gravity in units/s^2.");
//! con.execute("sv_gravity 600");
//! assert_eq!(con.float("sv_gravity"), 600.0);
//! ```
//!
//! Ordering matters and is preserved: text goes into a buffer and is drained
//! once per frame, so a `.cfg` that sets twenty convars applies as one atomic
//! batch rather than bleeding across frames.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

pub mod logging;
pub mod overlay;
mod tokenize;
pub use logging::{LogRelay, install as install_logger};
pub use overlay::ConsoleUi;
pub use tokenize::{split_commands, tokenize};

/// Behavioural flags on a convar. Combine with `|`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ConVarFlags(pub u32);

impl ConVarFlags {
    pub const NONE: ConVarFlags = ConVarFlags(0);
    /// Written to `config.cfg` on shutdown and restored on start.
    pub const ARCHIVE: ConVarFlags = ConVarFlags(1 << 0);
    /// Refuses to change unless `sv_cheats` is on.
    pub const CHEAT: ConVarFlags = ConVarFlags(1 << 1);
    /// Server pushes its value to every client; clients may not set it.
    pub const REPLICATED: ConVarFlags = ConVarFlags(1 << 2);
    /// Changing it announces the change to all players.
    pub const NOTIFY: ConVarFlags = ConVarFlags(1 << 3);
    /// Part of the client's identity, sent to the server on connect.
    pub const USERINFO: ConVarFlags = ConVarFlags(1 << 4);
    /// Hidden from `cvarlist` and autocomplete.
    pub const HIDDEN: ConVarFlags = ConVarFlags(1 << 5);
    /// Only meaningful in a developer build.
    pub const DEVELOPMENT: ConVarFlags = ConVarFlags(1 << 6);
    /// The server is allowed to run this on a client.
    pub const SERVER_CAN_EXECUTE: ConVarFlags = ConVarFlags(1 << 7);

    #[inline]
    pub fn contains(self, other: ConVarFlags) -> bool { self.0 & other.0 == other.0 }

    pub fn describe(self) -> String {
        let mut parts = Vec::new();
        for (flag, name) in [
            (Self::ARCHIVE, "archive"), (Self::CHEAT, "cheat"),
            (Self::REPLICATED, "replicated"), (Self::NOTIFY, "notify"),
            (Self::USERINFO, "userinfo"), (Self::HIDDEN, "hidden"),
            (Self::DEVELOPMENT, "development"), (Self::SERVER_CAN_EXECUTE, "server_can_execute"),
        ] {
            if self.contains(flag) { parts.push(name); }
        }
        parts.join(", ")
    }
}

impl std::ops::BitOr for ConVarFlags {
    type Output = ConVarFlags;
    fn bitor(self, o: ConVarFlags) -> ConVarFlags { ConVarFlags(self.0 | o.0) }
}

/// A single console variable.
///
/// The parsed forms are cached rather than reparsed per read, because convars
/// are read inside hot loops (`sv_gravity` every physics tick, `r_drawworld`
/// every frame) and `str::parse` in those loops is not free.
#[derive(Clone)]
pub struct ConVar {
    pub name: String,
    pub help: String,
    pub flags: ConVarFlags,
    pub default: String,
    value: String,
    float: f32,
    int: i32,
    pub min: Option<f32>,
    pub max: Option<f32>,
}

impl ConVar {
    pub fn string(&self) -> &str { &self.value }
    pub fn float(&self) -> f32 { self.float }
    pub fn int(&self) -> i32 { self.int }
    pub fn bool(&self) -> bool { self.int != 0 }
    pub fn is_default(&self) -> bool { self.value == self.default }

    fn apply(&mut self, raw: &str) {
        let mut f: f32 = raw.trim().parse().unwrap_or(0.0);
        let clamped = match (self.min, self.max) {
            (lo, hi) => {
                let mut v = f;
                if let Some(lo) = lo { v = v.max(lo); }
                if let Some(hi) = hi { v = v.min(hi); }
                v
            }
        };
        // Only rewrite the string when clamping actually moved the value, so
        // that a non-numeric convar (a map name, a player name) keeps its text.
        if clamped != f && f.is_finite() {
            f = clamped;
            self.value = format!("{f}");
        } else {
            self.value = raw.to_string();
        }
        self.float = f;
        self.int = f as i32;
    }
}

/// Arguments to a concommand. `argv[0]` is the command name itself.
#[derive(Clone, Debug)]
pub struct Args {
    pub argv: Vec<String>,
    /// Everything after the command name, verbatim -- what `say` wants.
    pub rest: String,
}

impl Args {
    pub fn count(&self) -> usize { self.argv.len() }
    pub fn get(&self, i: usize) -> Option<&str> { self.argv.get(i).map(|s| s.as_str()) }
    pub fn name(&self) -> &str { self.argv.first().map(|s| s.as_str()).unwrap_or("") }
    pub fn float(&self, i: usize) -> Option<f32> { self.get(i)?.parse().ok() }
    pub fn int(&self, i: usize) -> Option<i32> {
        self.get(i)?.parse().ok().or_else(|| self.get(i)?.parse::<f32>().ok().map(|f| f as i32))
    }
}

/// A concommand handler. Takes the console so commands can read convars,
/// print, and enqueue further commands.
pub type CommandFn = Arc<dyn Fn(&mut Console, &Args) + Send + Sync>;

/// Called after a convar's value changes.
pub type ChangeFn = Arc<dyn Fn(&mut Console, &str, &str) + Send + Sync>;

#[derive(Clone)]
pub struct ConCommand {
    pub name: String,
    pub help: String,
    pub flags: ConVarFlags,
    pub func: CommandFn,
}

/// Severity of a console line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel { Echo, Info, Warning, Error, Developer }

#[derive(Clone, Debug)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
}

/// How many lines of scrollback the console keeps.
const MAX_LOG_LINES: usize = 4096;

/// The console itself: convar table, command table, buffer and log.
pub struct Console {
    cvars: HashMap<String, ConVar>,
    change_callbacks: HashMap<String, Vec<ChangeFn>>,
    commands: HashMap<String, ConCommand>,
    aliases: HashMap<String, String>,
    buffer: VecDeque<String>,
    log: VecDeque<LogLine>,
    history: Vec<String>,
    /// Set by the `wait` command; blocks the rest of the buffer until the
    /// next [`Console::run_buffered`].
    waiting: bool,
    /// Reads a config file by name. Wired to the VFS by the engine, left
    /// unset in tools that have no filesystem of their own.
    exec_handler: Option<Arc<dyn Fn(&str) -> Option<String> + Send + Sync>>,
    /// Depth guard: an `exec` that execs itself would otherwise spin forever.
    exec_depth: u32,
    /// Things commands have asked the host to do.
    ///
    /// A command only ever gets `&mut Console` -- deliberately, because that
    /// is what lets the same string work from a key binding, a config file
    /// and the command line. Anything needing more than the console (loading
    /// a map, running a script, quitting) leaves a request here and the host
    /// picks it up once a frame. A queue rather than a slot because two
    /// requests in one config file must both survive.
    requests: VecDeque<(String, String)>,
}

/// A request a command left for the host.
pub mod requests {
    /// Load a map. The payload is its name.
    pub const MAP: &str = "map";
    /// Exit.
    pub const QUIT: &str = "quit";
    /// Evaluate script source. The payload is the source.
    pub const SCRIPT: &str = "script";
    /// Load a script file by name.
    pub const SCRIPT_FILE: &str = "script_file";
    /// Forget every loaded script and load them again.
    pub const SCRIPT_RELOAD: &str = "script_reload";
    /// Play a sound by name.
    pub const PLAY_SOUND: &str = "play_sound";
    /// Stop every sound.
    pub const STOP_SOUND: &str = "stop_sound";
    /// Reopen the audio device and forget every loaded sound.
    pub const SOUND_RESTART: &str = "sound_restart";
    /// Open or close the console. Owned by the host, not the engine: a
    /// dedicated server has a console and no window to draw one in.
    pub const TOGGLE_CONSOLE: &str = "toggle_console";
}

const MAX_EXEC_DEPTH: u32 = 16;

impl Default for Console {
    fn default() -> Self { Self::new() }
}

impl Console {
    pub fn new() -> Self {
        let mut con = Console {
            cvars: HashMap::new(),
            change_callbacks: HashMap::new(),
            commands: HashMap::new(),
            aliases: HashMap::new(),
            buffer: VecDeque::new(),
            log: VecDeque::new(),
            history: Vec::new(),
            waiting: false,
            exec_handler: None,
            exec_depth: 0,
            requests: VecDeque::new(),
        };
        con.register_builtins();
        con
    }

    // ---- registration ----------------------------------------------------

    pub fn register_cvar(
        &mut self,
        name: &str,
        default: &str,
        flags: ConVarFlags,
        help: &str,
    ) -> &mut Self {
        let mut cv = ConVar {
            name: name.to_string(),
            help: help.to_string(),
            flags,
            default: default.to_string(),
            value: String::new(),
            float: 0.0,
            int: 0,
            min: None,
            max: None,
        };
        cv.apply(default);
        self.cvars.insert(name.to_string(), cv);
        self
    }

    /// Register a convar clamped to `[min, max]`.
    pub fn register_cvar_ranged(
        &mut self,
        name: &str,
        default: &str,
        min: Option<f32>,
        max: Option<f32>,
        flags: ConVarFlags,
        help: &str,
    ) -> &mut Self {
        self.register_cvar(name, default, flags, help);
        if let Some(cv) = self.cvars.get_mut(name) {
            cv.min = min;
            cv.max = max;
            let raw = cv.value.clone();
            cv.apply(&raw);
        }
        self
    }

    pub fn register_command(
        &mut self,
        name: &str,
        flags: ConVarFlags,
        help: &str,
        func: impl Fn(&mut Console, &Args) + Send + Sync + 'static,
    ) -> &mut Self {
        self.commands.insert(
            name.to_string(),
            ConCommand {
                name: name.to_string(),
                help: help.to_string(),
                flags,
                func: Arc::new(func),
            },
        );
        self
    }

    pub fn on_change(&mut self, name: &str, f: impl Fn(&mut Console, &str, &str) + Send + Sync + 'static) {
        self.change_callbacks.entry(name.to_string()).or_default().push(Arc::new(f));
    }

    pub fn set_exec_handler(&mut self, f: impl Fn(&str) -> Option<String> + Send + Sync + 'static) {
        self.exec_handler = Some(Arc::new(f));
    }

    // ---- reading ---------------------------------------------------------

    pub fn cvar(&self, name: &str) -> Option<&ConVar> { self.cvars.get(name) }
    pub fn has_cvar(&self, name: &str) -> bool { self.cvars.contains_key(name) }
    pub fn has_command(&self, name: &str) -> bool { self.commands.contains_key(name) }

    /// Convar value as a float, or `0.0` if it does not exist.
    ///
    /// Reads deliberately do not panic on a missing convar: subsystems query
    /// convars owned by other subsystems that may not have registered yet.
    pub fn float(&self, name: &str) -> f32 { self.cvars.get(name).map_or(0.0, |c| c.float) }
    pub fn int(&self, name: &str) -> i32 { self.cvars.get(name).map_or(0, |c| c.int) }
    pub fn bool(&self, name: &str) -> bool { self.cvars.get(name).is_some_and(|c| c.int != 0) }
    pub fn string(&self, name: &str) -> &str {
        self.cvars.get(name).map_or("", |c| c.value.as_str())
    }

    pub fn cvars(&self) -> impl Iterator<Item = &ConVar> { self.cvars.values() }
    pub fn commands(&self) -> impl Iterator<Item = &ConCommand> { self.commands.values() }

    /// Convars whose value differs from their default and that are marked
    /// [`ConVarFlags::ARCHIVE`] -- exactly what belongs in `config.cfg`.
    pub fn archived(&self) -> Vec<(&str, &str)> {
        let mut out: Vec<_> = self
            .cvars
            .values()
            .filter(|c| c.flags.contains(ConVarFlags::ARCHIVE) && !c.is_default())
            .map(|c| (c.name.as_str(), c.value.as_str()))
            .collect();
        out.sort_by_key(|(n, _)| *n);
        out
    }

    /// How many names there are to complete against.
    ///
    /// Worth stating out loud when the console opens: the difference between
    /// "this thing accepts nothing" and "this thing accepts two hundred
    /// things, here is how to search them" is the whole of whether anyone
    /// uses it.
    pub fn name_count(&self) -> usize {
        self.complete("").len()
    }

    /// Names matching a prefix, for tab completion. Commands and convars
    /// together, sorted, hidden ones omitted.
    pub fn complete(&self, prefix: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .cvars
            .values()
            .filter(|c| !c.flags.contains(ConVarFlags::HIDDEN))
            .map(|c| c.name.clone())
            .chain(self.commands.values().map(|c| c.name.clone()))
            .chain(self.aliases.keys().cloned())
            .filter(|n| n.starts_with(prefix))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    // ---- writing ---------------------------------------------------------

    /// Set a convar, running its change callbacks. Bypasses cheat protection;
    /// this is the path engine code uses, not the path user input takes.
    pub fn set(&mut self, name: &str, value: &str) {
        let old = match self.cvars.get_mut(name) {
            Some(cv) => {
                let old = cv.value.clone();
                cv.apply(value);
                if cv.value == old { return; }
                old
            }
            None => {
                self.warn(format!("set: unknown convar '{name}'"));
                return;
            }
        };
        // Callbacks are cloned out first: they take `&mut Console`, and the
        // map they live in belongs to that same Console.
        if let Some(cbs) = self.change_callbacks.get(name).cloned() {
            for cb in cbs { cb(self, name, &old); }
        }
    }

    pub fn set_float(&mut self, name: &str, v: f32) { self.set(name, &v.to_string()); }
    pub fn set_bool(&mut self, name: &str, v: bool) { self.set(name, if v { "1" } else { "0" }); }

    // ---- logging ---------------------------------------------------------

    pub fn print(&mut self, text: impl Into<String>) { self.log_line(LogLevel::Info, text.into()); }
    pub fn echo(&mut self, text: impl Into<String>) { self.log_line(LogLevel::Echo, text.into()); }
    pub fn warn(&mut self, text: impl Into<String>) { self.log_line(LogLevel::Warning, text.into()); }
    pub fn error(&mut self, text: impl Into<String>) { self.log_line(LogLevel::Error, text.into()); }

    /// Developer-only output; suppressed unless `developer` is non-zero.
    pub fn developer(&mut self, text: impl Into<String>) {
        if self.int("developer") > 0 { self.log_line(LogLevel::Developer, text.into()); }
    }

    fn log_line(&mut self, level: LogLevel, text: String) {
        match level {
            LogLevel::Error => log::error!("{text}"),
            LogLevel::Warning => log::warn!("{text}"),
            LogLevel::Developer => log::debug!("{text}"),
            _ => log::info!("{text}"),
        }
        self.log.push_back(LogLine { level, text });
        while self.log.len() > MAX_LOG_LINES { self.log.pop_front(); }
    }

    /// Take everything the global logger has queued and put it in the
    /// scrollback. Called once a frame by the engine.
    ///
    /// This is what makes the rest of the engine visible from inside the
    /// game: without it, anything logged through the `log` crate went only to
    /// a terminal, and the console -- the one place anyone would look --
    /// showed nothing.
    pub fn drain_log_relay(&mut self, relay: &LogRelay) {
        let (lines, dropped) = relay.take();
        for line in lines {
            self.log.push_back(line);
            while self.log.len() > MAX_LOG_LINES { self.log.pop_front(); }
        }
        if dropped > 0 {
            self.log_line(
                LogLevel::Warning,
                format!("{dropped} log lines dropped: something is logging faster than a frame"),
            );
        }
    }

    // ---- host requests ---------------------------------------------------

    /// Ask the host to do something the console cannot do itself.
    pub fn request(&mut self, kind: &str, payload: impl Into<String>) {
        self.requests.push_back((kind.to_string(), payload.into()));
    }

    /// Take everything commands have asked the host for.
    pub fn take_requests(&mut self) -> Vec<(String, String)> {
        self.requests.drain(..).collect()
    }

    pub fn pending_requests(&self) -> usize { self.requests.len() }

    pub fn log(&self) -> impl Iterator<Item = &LogLine> { self.log.iter() }
    pub fn log_len(&self) -> usize { self.log.len() }
    pub fn clear_log(&mut self) { self.log.clear(); }
    pub fn history(&self) -> &[String] { &self.history }

    // ---- execution -------------------------------------------------------

    /// Queue text for execution on the next [`Console::run_buffered`].
    pub fn enqueue(&mut self, text: impl Into<String>) {
        for cmd in split_commands(&text.into()) { self.buffer.push_back(cmd); }
    }

    /// Queue text at the *front* of the buffer.
    ///
    /// `exec` uses this: the contents of a config file must run before
    /// whatever was already queued behind the `exec` line itself.
    pub fn enqueue_front(&mut self, text: impl Into<String>) {
        for cmd in split_commands(&text.into()).into_iter().rev() {
            self.buffer.push_front(cmd);
        }
    }

    /// Run everything currently buffered, stopping early at a `wait`.
    pub fn run_buffered(&mut self) {
        self.waiting = false;
        while !self.waiting {
            let Some(cmd) = self.buffer.pop_front() else { break };
            self.execute_single(&cmd);
        }
    }

    /// Execute text immediately, in full.
    pub fn execute(&mut self, text: &str) {
        for cmd in split_commands(text) { self.execute_single(&cmd); }
    }

    /// Execute a line the way user input arrives: recorded in history, and
    /// with cheat protection enforced.
    pub fn execute_user(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() { return; }
        if self.history.last().map(String::as_str) != Some(trimmed) {
            self.history.push(trimmed.to_string());
        }
        self.echo(format!("] {trimmed}"));
        self.execute(trimmed);
    }

    fn execute_single(&mut self, line: &str) {
        let argv = tokenize(line);
        if argv.is_empty() { return; }
        let name = argv[0].clone();
        let rest = line[line.find(&name).map_or(0, |i| i + name.len())..].trim().to_string();
        let args = Args { argv, rest };

        if let Some(cmd) = self.commands.get(&name).cloned() {
            if cmd.flags.contains(ConVarFlags::CHEAT) && !self.cheats_enabled() {
                self.warn(format!("{name} is cheat-protected; set sv_cheats 1 to use it"));
                return;
            }
            (cmd.func)(self, &args);
            return;
        }

        if let Some(expansion) = self.aliases.get(&name).cloned() {
            self.enqueue_front(expansion);
            return;
        }

        if self.cvars.contains_key(&name) {
            if args.count() == 1 {
                let cv = &self.cvars[&name];
                let (value, default, help) =
                    (cv.value.clone(), cv.default.clone(), cv.help.clone());
                self.print(format!("\"{name}\" = \"{value}\" (default \"{default}\")"));
                if !help.is_empty() { self.print(format!(" - {help}")); }
                return;
            }
            let cv = &self.cvars[&name];
            if cv.flags.contains(ConVarFlags::CHEAT) && !self.cheats_enabled() {
                self.warn(format!("{name} is cheat-protected; set sv_cheats 1 to change it"));
                return;
            }
            // Everything after the name, so `name "two words"` sets both words.
            let value = if args.count() == 2 { args.argv[1].clone() } else { args.rest.clone() };
            self.set(&name, &value);
            return;
        }

        self.warn(format!("unknown command '{name}'"));
    }

    /// Whether cheat-protected convars and commands are currently usable.
    pub fn cheats_enabled(&self) -> bool {
        // A missing sv_cheats means nobody registered the game rules yet --
        // in a tool, not a running server. Tools are not cheating.
        !self.has_cvar("sv_cheats") || self.bool("sv_cheats")
    }

    fn register_builtins(&mut self) {
        self.register_cvar("developer", "0", ConVarFlags::NONE, "Verbosity of developer output.");

        self.register_command("echo", ConVarFlags::NONE, "Print text to the console.", |con, args| {
            let text = args.argv[1..].join(" ");
            con.echo(text);
        });

        self.register_command("wait", ConVarFlags::NONE, "Defer the rest of the command buffer to the next frame.", |con, _| {
            con.waiting = true;
        });

        self.register_command("clear", ConVarFlags::NONE, "Clear the console scrollback.", |con, _| {
            con.clear_log();
        });

        self.register_command("alias", ConVarFlags::NONE, "Define or list command aliases.", |con, args| {
            if args.count() < 2 {
                let list: Vec<String> = con.aliases.iter().map(|(k, v)| format!("{k} : {v}")).collect();
                for line in list { con.print(line); }
                return;
            }
            let name = args.argv[1].clone();
            if args.count() == 2 { con.aliases.remove(&name); return; }
            let body = args.argv[2..].join(" ");
            con.aliases.insert(name, body);
        });

        self.register_command("toggle", ConVarFlags::NONE, "Flip a convar between 0 and non-zero.", |con, args| {
            let Some(name) = args.get(1).map(str::to_string) else {
                con.warn("usage: toggle <convar>");
                return;
            };
            let v = con.bool(&name);
            con.set_bool(&name, !v);
        });

        self.register_command("incrementvar", ConVarFlags::NONE, "incrementvar <convar> <min> <max> <delta>", |con, args| {
            let (Some(name), Some(min), Some(max), Some(delta)) =
                (args.get(1).map(str::to_string), args.float(2), args.float(3), args.float(4))
            else {
                con.warn("usage: incrementvar <convar> <min> <max> <delta>");
                return;
            };
            let mut v = con.float(&name) + delta;
            // Wrap rather than clamp, so a key bound to this cycles.
            if v > max { v = min; }
            if v < min { v = max; }
            con.set_float(&name, v);
        });

        self.register_command("exec", ConVarFlags::NONE, "Run a config file.", |con, args| {
            let Some(name) = args.get(1).map(str::to_string) else {
                con.warn("usage: exec <file.cfg>");
                return;
            };
            if con.exec_depth >= MAX_EXEC_DEPTH {
                con.error(format!("exec: refusing to nest deeper than {MAX_EXEC_DEPTH} ({name})"));
                return;
            }
            let Some(handler) = con.exec_handler.clone() else {
                con.warn("exec: no filesystem is attached to this console");
                return;
            };
            match handler(&name) {
                Some(text) => {
                    con.exec_depth += 1;
                    con.execute(&text);
                    con.exec_depth -= 1;
                }
                None => con.warn(format!("exec: '{name}' not found")),
            }
        });

        self.register_command("find", ConVarFlags::NONE, "Search convars and commands by substring.", |con, args| {
            let Some(needle) = args.get(1).map(str::to_lowercase) else {
                con.warn("usage: find <substring>");
                return;
            };
            let mut lines: Vec<String> = Vec::new();
            for cv in con.cvars.values() {
                if cv.flags.contains(ConVarFlags::HIDDEN) { continue; }
                if cv.name.to_lowercase().contains(&needle) || cv.help.to_lowercase().contains(&needle) {
                    lines.push(format!("{} = \"{}\" - {}", cv.name, cv.value, cv.help));
                }
            }
            for c in con.commands.values() {
                if c.name.to_lowercase().contains(&needle) || c.help.to_lowercase().contains(&needle) {
                    lines.push(format!("{} (command) - {}", c.name, c.help));
                }
            }
            lines.sort();
            if lines.is_empty() { con.print(format!("no matches for '{needle}'")); }
            for l in lines { con.print(l); }
        });

        self.register_command("cvarlist", ConVarFlags::NONE, "List every convar.", |con, _| {
            let mut lines: Vec<String> = con
                .cvars
                .values()
                .filter(|c| !c.flags.contains(ConVarFlags::HIDDEN))
                .map(|c| {
                    let flags = c.flags.describe();
                    let suffix = if flags.is_empty() { String::new() } else { format!(" [{flags}]") };
                    format!("{} = \"{}\"{suffix} - {}", c.name, c.value, c.help)
                })
                .collect();
            lines.sort();
            let n = lines.len();
            for l in lines { con.print(l); }
            con.print(format!("{n} convars"));
        });

        self.register_command("help", ConVarFlags::NONE, "Show help for a convar or command.", |con, args| {
            let Some(name) = args.get(1).map(str::to_string) else {
                con.print("usage: help <name>. Try 'find <substring>' or 'cvarlist'.");
                return;
            };
            if let Some(cv) = con.cvars.get(&name) {
                let msg = format!("{} = \"{}\" (default \"{}\")\n  {}", cv.name, cv.value, cv.default, cv.help);
                con.print(msg);
            } else if let Some(c) = con.commands.get(&name) {
                let msg = format!("{} (command)\n  {}", c.name, c.help);
                con.print(msg);
            } else {
                con.warn(format!("no convar or command named '{name}'"));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con() -> Console {
        let mut c = Console::new();
        c.register_cvar("sv_cheats", "0", ConVarFlags::NOTIFY, "Allow cheat commands.");
        c.register_cvar("sv_gravity", "800", ConVarFlags::REPLICATED, "Gravity.");
        c
    }

    #[test]
    fn setting_a_convar_from_text() {
        let mut c = con();
        c.execute("sv_gravity 600");
        assert_eq!(c.float("sv_gravity"), 600.0);
        assert_eq!(c.int("sv_gravity"), 600);
        assert_eq!(c.string("sv_gravity"), "600");
    }

    #[test]
    fn multiple_commands_split_on_semicolons_and_newlines() {
        let mut c = con();
        c.execute("sv_gravity 100; sv_cheats 1\nsv_gravity 200");
        assert_eq!(c.float("sv_gravity"), 200.0);
        assert!(c.bool("sv_cheats"));
    }

    #[test]
    fn semicolons_inside_quotes_do_not_split() {
        let mut c = con();
        c.register_cvar("hostname", "server", ConVarFlags::NONE, "");
        c.execute(r#"hostname "a; b""#);
        assert_eq!(c.string("hostname"), "a; b");
    }

    #[test]
    fn cheat_protection_blocks_then_allows() {
        let mut c = con();
        c.register_cvar("sv_noclip", "0", ConVarFlags::CHEAT, "");
        c.execute("sv_noclip 1");
        assert!(!c.bool("sv_noclip"), "cheat convar must not change with sv_cheats off");
        c.execute("sv_cheats 1; sv_noclip 1");
        assert!(c.bool("sv_noclip"));
    }

    #[test]
    fn engine_set_bypasses_cheat_protection() {
        // Engine code is trusted; only the text path is gated.
        let mut c = con();
        c.register_cvar("sv_noclip", "0", ConVarFlags::CHEAT, "");
        c.set("sv_noclip", "1");
        assert!(c.bool("sv_noclip"));
    }

    #[test]
    fn ranges_clamp() {
        let mut c = Console::new();
        c.register_cvar_ranged("volume", "0.5", Some(0.0), Some(1.0), ConVarFlags::ARCHIVE, "");
        c.execute("volume 5");
        assert_eq!(c.float("volume"), 1.0);
        c.execute("volume -3");
        assert_eq!(c.float("volume"), 0.0);
    }

    #[test]
    fn non_numeric_convars_keep_their_text() {
        let mut c = Console::new();
        c.register_cvar("map", "kero_start", ConVarFlags::NONE, "");
        c.execute("map kero_arena");
        assert_eq!(c.string("map"), "kero_arena");
    }

    #[test]
    fn wait_defers_the_rest_of_the_buffer() {
        let mut c = con();
        c.enqueue("sv_gravity 100; wait; sv_gravity 200");
        c.run_buffered();
        assert_eq!(c.float("sv_gravity"), 100.0, "the post-wait command must not have run yet");
        c.run_buffered();
        assert_eq!(c.float("sv_gravity"), 200.0);
    }

    #[test]
    fn change_callbacks_fire_once_per_real_change() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let hits = Arc::new(AtomicU32::new(0));
        let h = hits.clone();
        let mut c = con();
        c.on_change("sv_gravity", move |_, _, _| { h.fetch_add(1, Ordering::SeqCst); });
        c.execute("sv_gravity 600");
        c.execute("sv_gravity 600"); // no-op, must not fire
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn change_callback_sees_the_old_value() {
        use std::sync::{Arc, Mutex};
        let seen = Arc::new(Mutex::new(String::new()));
        let s = seen.clone();
        let mut c = con();
        c.on_change("sv_gravity", move |_, _, old| { *s.lock().unwrap() = old.to_string(); });
        c.execute("sv_gravity 600");
        assert_eq!(&*seen.lock().unwrap(), "800");
    }

    #[test]
    fn aliases_expand() {
        let mut c = con();
        c.execute("alias lowgrav sv_gravity 200");
        c.enqueue("lowgrav");
        c.run_buffered();
        assert_eq!(c.float("sv_gravity"), 200.0);
    }

    #[test]
    fn exec_runs_config_text_before_queued_commands() {
        let mut c = con();
        c.set_exec_handler(|name| (name == "autoexec.cfg").then(|| "sv_gravity 42".to_string()));
        c.execute("exec autoexec.cfg");
        assert_eq!(c.float("sv_gravity"), 42.0);
    }

    #[test]
    fn self_referential_exec_terminates() {
        let mut c = con();
        c.set_exec_handler(|_| Some("exec loop.cfg".to_string()));
        c.execute("exec loop.cfg"); // must return rather than hang
        assert!(c.log().any(|l| l.level == LogLevel::Error));
    }

    #[test]
    fn unknown_command_warns_rather_than_panicking() {
        let mut c = con();
        c.execute("thiscommanddoesnotexist 1 2 3");
        assert!(c.log().any(|l| l.level == LogLevel::Warning));
    }

    #[test]
    fn archived_lists_only_changed_archive_convars() {
        let mut c = Console::new();
        c.register_cvar("volume", "0.5", ConVarFlags::ARCHIVE, "");
        c.register_cvar("name", "player", ConVarFlags::ARCHIVE, "");
        c.register_cvar("temp", "1", ConVarFlags::NONE, "");
        c.execute("volume 0.8; temp 9");
        assert_eq!(c.archived(), vec![("volume", "0.8")]);
    }

    #[test]
    fn completion_is_sorted_and_hides_hidden() {
        let mut c = Console::new();
        c.register_cvar("r_draw", "1", ConVarFlags::NONE, "");
        c.register_cvar("r_drawworld", "1", ConVarFlags::NONE, "");
        c.register_cvar("r_secret", "1", ConVarFlags::HIDDEN, "");
        let got = c.complete("r_");
        assert_eq!(got, vec!["r_draw", "r_drawworld"]);
    }

    #[test]
    fn commands_can_queue_more_commands() {
        let mut c = con();
        c.register_command("chain", ConVarFlags::NONE, "", |con, _| {
            con.enqueue_front("sv_gravity 333");
        });
        c.enqueue("chain");
        c.run_buffered();
        assert_eq!(c.float("sv_gravity"), 333.0);
    }
}
