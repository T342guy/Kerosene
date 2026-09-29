// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! How the engine is configured to start, and what it says when a map is missing.

use super::*;

/// How the engine was asked to start.
///
/// [`launch`](crate::launch::launch) fills this in from the command line and
/// the project; a test or a custom host starts from the default and changes
/// what it needs:
///
/// ```
/// # use kerosene_engine::EngineConfig;
/// let config = EngineConfig::default().with_content("content").with_map("kero_start");
/// assert_eq!(config.map.as_deref(), Some("kero_start"));
/// ```
///
/// Non-exhaustive, so a new setting is never a breaking change.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct EngineConfig {
    /// Directories to mount, searched in order.
    pub content_paths: Vec<PathBuf>,
    /// Archives to mount after them.
    pub archives: Vec<PathBuf>,
    /// Map to load on start.
    pub map: Option<String>,
    /// Console commands to run once everything is up.
    pub startup_commands: Vec<String>,
    /// Whether to open an audio device.
    ///
    /// Off in tests and headless runs: opening a sound card is slow, and a
    /// hundred `Engine`s in one test binary would each try.
    pub audio: bool,
    /// Which renderer to prefer for the window. Vulkan by default; the
    /// display falls back to whatever is there when it is not.
    pub renderer: kerosene_config::Renderer,
    /// Window size in pixels, when there is a window.
    pub window_width: u32,
    pub window_height: u32,
    /// Whether the window syncs to the display's refresh.
    pub vsync: bool,
    /// The global log relay, if one was installed.
    ///
    /// Handed in rather than installed here because installing a logger is a
    /// process-wide act and an `Engine` is constructed in a hundred tests.
    /// Without it the console still works; it just cannot show anything the
    /// rest of the engine logged.
    pub log: Option<std::sync::Arc<kerosene_console::LogRelay>>,
    /// The store: whether to try Steam, and what the game declares. The
    /// default is no store, which is what tests and servers want.
    pub platform: kerosene_platform::PlatformConfig,
    /// The window's title: the game's name.
    pub title: String,
    /// What the desktop knows the game as: the Wayland app id and the X11
    /// `WM_CLASS`, which is how a `.desktop` file's icon finds the window.
    /// Lowercase, no spaces.
    pub app_id: String,
    /// The game's own version, for `version` and the log. Empty for a
    /// test or a tool.
    pub version: String,
    /// Whether to mount the engine's base content beneath everything else.
    /// On by default; a test that wants only what it put there turns it
    /// off. See [`crate::base`].
    pub base_content: bool,
    /// Where the player's own files go -- saves and `config.cfg`, and
    /// anything else the engine or the game writes through the VFS --
    /// searched before everything else and written to first.
    /// `None` writes them into the first content directory instead, which
    /// is what a test and a `--portable` run want. [`crate::launch`] sets it
    /// to the platform's per-user directory for the game.
    pub user_dir: Option<PathBuf>,
    /// The map New Game starts on, from the main menu. `None` takes
    /// [`map`](EngineConfig::map), and failing that the base content's
    /// room. [`crate::launch`] sets it to the project's start map.
    pub new_game_map: Option<String>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            audio: false,
            renderer: kerosene_config::Renderer::default(),
            window_width: kerosene_config::DEFAULT_WIDTH,
            window_height: kerosene_config::DEFAULT_HEIGHT,
            vsync: true,
            log: None,
            content_paths: vec![PathBuf::from("content")],
            archives: Vec::new(),
            map: None,
            startup_commands: Vec::new(),
            platform: kerosene_platform::PlatformConfig::default(),
            title: "Kerosene".to_string(),
            app_id: "kerosene".to_string(),
            version: String::new(),
            base_content: true,
            user_dir: None,
            new_game_map: None,
        }
    }
}

impl EngineConfig {
    /// Mount `dir` as the only content tree.
    pub fn with_content(mut self, dir: impl Into<PathBuf>) -> Self {
        self.content_paths = vec![dir.into()];
        self
    }

    /// Load `map` on start.
    pub fn with_map(mut self, map: impl Into<String>) -> Self {
        self.map = Some(map.into());
        self
    }

    /// Run a console command once everything is up.
    pub fn with_command(mut self, line: impl Into<String>) -> Self {
        self.startup_commands.push(line.into());
        self
    }

    /// Open an audio device, or not.
    pub fn with_audio(mut self, audio: bool) -> Self {
        self.audio = audio;
        self
    }

    /// Relay the process's log into the console.
    pub fn with_log(mut self, relay: std::sync::Arc<kerosene_console::LogRelay>) -> Self {
        self.log = Some(relay);
        self
    }

    /// Mount the engine's base content beneath the game's, or not.
    pub fn with_base_content(mut self, base: bool) -> Self {
        self.base_content = base;
        self
    }

    /// Keep the player's files in `dir`, or with `None`, in the content
    /// tree. See [`EngineConfig::user_dir`].
    pub fn with_user_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.user_dir = dir;
        self
    }

    /// The store and what the game declares to it.
    pub fn with_platform(mut self, platform: kerosene_platform::PlatformConfig) -> Self {
        self.platform = platform;
        self
    }
}

/// Say why a map would not load, in terms someone can act on.
///
/// "not found in any search path" is true and useless. The overwhelmingly
/// common reason a map is missing is that it has never been compiled -- the
/// `.kmap` is right there, and nothing turned it into a `.kbsp`. The
/// next most common is that the content tree being searched is not the one
/// the map lives in. Both are worth saying outright, along with what was
/// searched, because the alternative is reading the source to find out.
pub fn explain_missing_map(vfs: &Vfs, name: &str, why: &VfsError) -> String {
    let mut said = format!("could not load the map '{name}'\n");

    // Anything other than "it is not there" is its own problem -- a permission,
    // a truncated archive -- and guessing "you forgot to compile it" over the
    // top of it would send someone the wrong way.
    if !matches!(why, VfsError::NotFound(_)) {
        return format!("{said}  {why}").trim_end().to_string();
    }

    if vfs.exists(&format!("maps/{name}.kmap")) {
        said.push_str(&format!(
            "  maps/{name}.kmap is there, but has not been compiled.\n"
        ));
        said.push_str("  build it with:  kerosene-tools play  (cargo play, in a game crate)\n");
        said.push_str(&format!(
            "  or on its own:  kerosene-tools cleave maps/{name}.kmap\n"
        ));
    } else {
        let mut maps: Vec<String> = vfs
            .list("maps", Some("kbsp"))
            .iter()
            .filter_map(|p| {
                p.strip_prefix("maps/")
                    .map(|n| n.trim_end_matches(".kbsp").to_string())
            })
            .collect();
        maps.sort();
        maps.dedup();
        if maps.is_empty() {
            said.push_str(
                "  no compiled maps in any search path. Build them with kerosene-tools play\n",
            );
        } else {
            said.push_str(&format!("  compiled maps here: {}\n", maps.join(", ")));
        }
    }

    said.push_str("  searched:\n");
    for layer in vfs.describe() {
        said.push_str(&format!("    {layer}\n"));
    }
    if vfs.path_count() == 0 {
        said.push_str("    (nothing mounted)\n");
    }
    said.trim_end().to_string()
}
