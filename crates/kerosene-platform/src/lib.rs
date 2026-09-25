// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The store a game ships on: achievements, stats, leaderboards, rich
//! presence, cloud files, DLC, the overlay and the Workshop.
//!
//! Steam is the reason this exists, and the only store it speaks to today.
//! But no game code, map, or script names Steam. They name a [`Platform`],
//! which forwards to a [`Backend`]: [`steam::SteamBackend`] when the engine
//! is built with the `steam` feature and the Steam client is running, and
//! [`NullBackend`] otherwise. So a game that awards an achievement on Steam
//! awards it -- in memory -- off Steam too, the same map works in both, and
//! the tests need neither a Steam client nor Valve's SDK.
//!
//! # Why the SDK is optional
//!
//! Kerosene is GPL software. The Steamworks SDK is not free software; the
//! Kerosene Exception lets a game combine the two (the SDK is an Independent
//! Module), but a default build of the engine, and the source tree, carry
//! none of it. The SDK arrives through the `steamworks` crate only when a
//! game turns the `steam` feature on, and `kiln --ship --steam` puts Valve's
//! redistributable beside the binary.
//!
//! # The shape of it
//!
//! Everything a game, a map or a script can ask for is a [`PlatformAction`],
//! and everything that comes back is a [`PlatformEvent`] -- the same
//! queue-and-apply model as script actions and entity requests. That is
//! what lets the entity I/O classes (`logic_achievement`, `logic_stat`,
//! `logic_leaderboard`, ...) and the Rhai `platform` object share one path:
//! each turns into actions, and each reacts to events.
//!
//! [`Platform`] also enforces the one rule a store integration most needs:
//! achievements and stats are *declared* in the project file, and anything
//! undeclared is refused with a message saying so, rather than silently
//! doing nothing on a store that has never heard of it.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

mod null;
pub mod script;
#[cfg(feature = "steam")]
pub mod steam;

pub use null::NullBackend;

/// Whether this build can talk to Steam at all.
pub const STEAM_BUILT_IN: bool = cfg!(feature = "steam");

/// The type a stat holds. Steam stores the two differently, and setting an
/// integer stat with a float fails there; declaring it makes that a mistake
/// caught at the source rather than on the store.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StatKind {
    #[default]
    Int,
    Float,
}

impl StatKind {
    pub fn parse(text: &str) -> StatKind {
        match text.trim().to_ascii_lowercase().as_str() {
            "float" | "f32" | "real" => StatKind::Float,
            _ => StatKind::Int,
        }
    }
}

/// What a game declares about itself: read from the project file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlatformConfig {
    /// The Steam app id. Without one, Steam is not tried.
    pub steam_appid: Option<u32>,
    /// Try Steam at all. The launcher sets this for a windowed run; tests and
    /// headless servers leave it off.
    pub use_steam: bool,
    /// Achievement ids and their display names.
    pub achievements: Vec<(String, String)>,
    /// Stat names and their types.
    pub stats: Vec<(String, StatKind)>,
    /// DLC app ids and their names.
    pub dlc: Vec<(u32, String)>,
    /// Where [`NullBackend`] keeps "cloud" files. `None` keeps them in
    /// memory.
    pub local_cloud: Option<PathBuf>,
}

/// Something asked of the platform.
///
/// Every source -- a script, an entity, a console command, game code --
/// ends up here, and each has a one-line text form ([`PlatformAction::parse`]
/// and `Display`) so an entity's request and a console command are the same
/// string.
#[derive(Clone, PartialEq, Debug)]
pub enum PlatformAction {
    /// Award an achievement.
    Unlock(String),
    /// Take one back. A cheat; for testing.
    Clear(String),
    /// Show "3 / 10" progress toward an achievement, as a toast.
    Progress {
        id: String,
        current: u32,
        max: u32,
    },
    SetStat {
        name: String,
        value: f64,
    },
    AddStat {
        name: String,
        delta: f64,
    },
    /// Push changed stats now rather than on the next batch.
    StoreStats,
    /// Rich presence: what friends see the player doing.
    Presence {
        key: String,
        value: String,
    },
    ClearPresence,
    /// Open the overlay on a dialog: `friends`, `achievements`, `stats`,
    /// `community`, `settings`, ...
    OpenOverlay(String),
    OpenUrl(String),
    /// The store page of an app; `None` is this game's own.
    OpenStore(Option<u32>),
    /// Post a score. The result comes back as [`PlatformEvent::ScoreSubmitted`]
    /// or [`PlatformEvent::ScoreFailed`].
    SubmitScore {
        board: String,
        score: i32,
        /// Lower is better (a time), rather than higher (points).
        ascending: bool,
    },
    /// Ask whether a DLC is installed; answered by
    /// [`PlatformEvent::DlcChecked`].
    CheckDlc(u32),
}

impl PlatformAction {
    /// Read the one-line form: `unlock ACH_WIN`, `add_stat kills 1`,
    /// `score best_time 5230 asc`, `presence status In the atrium`.
    pub fn parse(text: &str) -> Result<PlatformAction, String> {
        let text = text.trim();
        let (verb, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        let rest = rest.trim();
        let mut words = rest.split_whitespace();
        let mut word = |what: &str| {
            words
                .next()
                .map(str::to_string)
                .ok_or_else(|| format!("`{verb}` needs {what}"))
        };
        let number = |s: String, what: &str| -> Result<f64, String> {
            s.parse::<f64>()
                .map_err(|_| format!("`{verb}`: {what} `{s}` is not a number"))
        };
        Ok(match verb.to_ascii_lowercase().as_str() {
            "unlock" => PlatformAction::Unlock(word("an achievement id")?),
            "clear" => PlatformAction::Clear(word("an achievement id")?),
            "progress" => {
                let id = word("an achievement id")?;
                let current = number(word("a current value")?, "current")? as u32;
                let max = number(word("a maximum")?, "maximum")? as u32;
                PlatformAction::Progress { id, current, max }
            }
            "set_stat" => {
                let name = word("a stat name")?;
                let value = number(word("a value")?, "value")?;
                PlatformAction::SetStat { name, value }
            }
            "add_stat" => {
                let name = word("a stat name")?;
                let delta = match words.next() {
                    Some(d) => number(d.to_string(), "amount")?,
                    None => 1.0,
                };
                PlatformAction::AddStat { name, delta }
            }
            "store_stats" => PlatformAction::StoreStats,
            "presence" => {
                let (key, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                if key.is_empty() {
                    return Err("`presence` needs a key".into());
                }
                PlatformAction::Presence {
                    key: key.to_string(),
                    value: value.trim().to_string(),
                }
            }
            "clear_presence" => PlatformAction::ClearPresence,
            "overlay" => PlatformAction::OpenOverlay(if rest.is_empty() {
                "friends".to_string()
            } else {
                rest.to_string()
            }),
            "url" => {
                if rest.is_empty() {
                    return Err("`url` needs an address".into());
                }
                PlatformAction::OpenUrl(rest.to_string())
            }
            "store" => PlatformAction::OpenStore(match words.next() {
                Some(id) => Some(number(id.to_string(), "app id")? as u32),
                None => None,
            }),
            "score" => {
                let board = word("a leaderboard name")?;
                let score = number(word("a score")?, "score")? as i32;
                let ascending = matches!(words.next(), Some(s) if s.eq_ignore_ascii_case("asc"));
                PlatformAction::SubmitScore {
                    board,
                    score,
                    ascending,
                }
            }
            "dlc" => PlatformAction::CheckDlc(number(word("an app id")?, "app id")? as u32),
            "" => return Err("empty platform action".into()),
            other => return Err(format!("unknown platform action `{other}`")),
        })
    }
}

impl std::fmt::Display for PlatformAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformAction::Unlock(id) => write!(f, "unlock {id}"),
            PlatformAction::Clear(id) => write!(f, "clear {id}"),
            PlatformAction::Progress { id, current, max } => {
                write!(f, "progress {id} {current} {max}")
            }
            PlatformAction::SetStat { name, value } => write!(f, "set_stat {name} {value}"),
            PlatformAction::AddStat { name, delta } => write!(f, "add_stat {name} {delta}"),
            PlatformAction::StoreStats => write!(f, "store_stats"),
            PlatformAction::Presence { key, value } => write!(f, "presence {key} {value}"),
            PlatformAction::ClearPresence => write!(f, "clear_presence"),
            PlatformAction::OpenOverlay(d) => write!(f, "overlay {d}"),
            PlatformAction::OpenUrl(u) => write!(f, "url {u}"),
            PlatformAction::OpenStore(Some(id)) => write!(f, "store {id}"),
            PlatformAction::OpenStore(None) => write!(f, "store"),
            PlatformAction::SubmitScore {
                board,
                score,
                ascending,
            } => write!(
                f,
                "score {board} {score} {}",
                if *ascending { "asc" } else { "desc" }
            ),
            PlatformAction::CheckDlc(id) => write!(f, "dlc {id}"),
        }
    }
}

/// Something the platform reports back.
#[derive(Clone, PartialEq, Debug)]
pub enum PlatformEvent {
    OverlayChanged(bool),
    AchievementUnlocked(String),
    AchievementCleared(String),
    AchievementProgress {
        id: String,
        current: u32,
        max: u32,
    },
    StatChanged {
        name: String,
        old: f64,
        new: f64,
    },
    ScoreSubmitted {
        board: String,
        score: i32,
        /// Global rank after the upload, 1 being first.
        rank: i32,
        /// Whether this beat the player's previous best.
        improved: bool,
    },
    ScoreFailed {
        board: String,
    },
    DlcChecked {
        appid: u32,
        owned: bool,
    },
    /// A Workshop item's content was mounted into the file system.
    WorkshopMounted {
        id: u64,
        path: PathBuf,
    },
}

impl PlatformEvent {
    /// The event's name, as scripts and the UI see it.
    pub fn name(&self) -> &'static str {
        match self {
            PlatformEvent::OverlayChanged(true) => "overlay_opened",
            PlatformEvent::OverlayChanged(false) => "overlay_closed",
            PlatformEvent::AchievementUnlocked(_) => "achievement_unlocked",
            PlatformEvent::AchievementCleared(_) => "achievement_cleared",
            PlatformEvent::AchievementProgress { .. } => "achievement_progress",
            PlatformEvent::StatChanged { .. } => "stat_changed",
            PlatformEvent::ScoreSubmitted { .. } => "score_submitted",
            PlatformEvent::ScoreFailed { .. } => "score_failed",
            PlatformEvent::DlcChecked { .. } => "dlc_checked",
            PlatformEvent::WorkshopMounted { .. } => "workshop_mounted",
        }
    }

    /// The event's data, as one string: the id, or `name=value` pairs.
    pub fn data(&self) -> String {
        match self {
            PlatformEvent::OverlayChanged(_) => String::new(),
            PlatformEvent::AchievementUnlocked(id) | PlatformEvent::AchievementCleared(id) => {
                id.clone()
            }
            PlatformEvent::AchievementProgress { id, current, max } => {
                format!("{id} {current} {max}")
            }
            PlatformEvent::StatChanged { name, new, .. } => format!("{name} {new}"),
            PlatformEvent::ScoreSubmitted {
                board,
                score,
                rank,
                improved,
            } => format!("{board} {score} {rank} {}", *improved as u8),
            PlatformEvent::ScoreFailed { board } => board.clone(),
            PlatformEvent::DlcChecked { appid, owned } => format!("{appid} {}", *owned as u8),
            PlatformEvent::WorkshopMounted { id, .. } => id.to_string(),
        }
    }
}

/// What a script can read of the platform, all at once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlatformView {
    /// A store is connected.
    pub available: bool,
    /// `steam`, or `none`.
    pub name: String,
    pub user: String,
    pub language: String,
    pub overlay: bool,
    /// Every declared achievement, and whether it is unlocked.
    pub achievements: BTreeMap<String, bool>,
    /// Every declared stat's value.
    pub stats: BTreeMap<String, f64>,
    /// Every declared DLC, and whether it is installed.
    pub dlc: BTreeMap<u32, bool>,
}

/// One store's implementation.
///
/// Small on purpose: validation, batching and events are [`Platform`]'s, so
/// a second store (GOG Galaxy, Epic) is only the calls.
pub trait Backend {
    /// `steam`, `none`.
    fn name(&self) -> &'static str;
    /// A store is actually connected.
    fn available(&self) -> bool;
    fn user(&self) -> String;
    fn language(&self) -> String;
    /// Pump the store's callbacks, appending what came of them.
    fn frame(&mut self, events: &mut Vec<PlatformEvent>);
    fn overlay_active(&self) -> bool;

    fn achievement(&self, id: &str) -> bool;
    fn set_achievement(&mut self, id: &str, unlocked: bool) -> Result<(), String>;
    /// The achievements the store knows of, when it can say.
    fn achievement_names(&self) -> Vec<String> {
        Vec::new()
    }
    fn indicate_progress(&mut self, id: &str, current: u32, max: u32) -> Result<(), String>;

    fn stat(&self, name: &str, kind: StatKind) -> Option<f64>;
    fn set_stat(&mut self, name: &str, kind: StatKind, value: f64) -> Result<(), String>;
    /// Send changed stats and achievements to the store.
    fn store_stats(&mut self) -> Result<(), String>;

    fn set_presence(&mut self, key: &str, value: &str);
    fn clear_presence(&mut self);

    fn open_overlay(&mut self, dialog: &str);
    fn open_url(&mut self, url: &str);
    fn open_store(&mut self, appid: Option<u32>);

    /// Post a score; the result arrives through [`Backend::frame`].
    fn submit_score(&mut self, board: &str, score: i32, ascending: bool);

    fn owns_dlc(&self, appid: u32) -> bool;

    fn cloud_enabled(&self) -> bool;
    fn cloud_write(&mut self, name: &str, bytes: &[u8]) -> Result<(), String>;
    fn cloud_read(&self, name: &str) -> Option<Vec<u8>>;

    /// Installed Workshop items: their ids and folders.
    fn workshop_items(&self) -> Vec<(u64, PathBuf)>;
}

/// How long changed stats wait before they are sent, in seconds.
///
/// Steam rate-limits `StoreStats`, and a stat bumped every tick would hit
/// the limit in a second; batching also keeps the call off the frame.
pub const STORE_INTERVAL: f64 = 1.0;

/// The store, as the engine holds it.
pub struct Platform {
    backend: Box<dyn Backend>,
    config: PlatformConfig,
    events: Vec<PlatformEvent>,
    /// Changed stats or achievements not yet sent.
    dirty: bool,
    since_store: f64,
    /// Things already warned about, so a trigger fired every tick does not
    /// fill the console.
    warned: HashSet<String>,
    overlay: bool,
}

impl Platform {
    /// The platform for a game: Steam when the build has it, the config asks
    /// for it and the client answers; nothing otherwise.
    pub fn new(config: PlatformConfig) -> Platform {
        #[cfg(feature = "steam")]
        if config.use_steam
            && let Some(appid) = config.steam_appid
        {
            match steam::SteamBackend::init(appid) {
                Ok(backend) => {
                    log::info!("steam: connected as {}", backend.user());
                    return Platform::with_backend(config, Box::new(backend));
                }
                Err(e) => {
                    log::warn!("steam: not available ({e}); achievements and stats stay local")
                }
            }
        }
        #[cfg(not(feature = "steam"))]
        if config.use_steam && config.steam_appid.is_some() {
            log::info!(
                "the project has a steam_appid, but this build has no Steam support \
                 (build with --features steam)"
            );
        }
        let null = NullBackend::new(config.local_cloud.clone());
        Platform::with_backend(config, Box::new(null))
    }

    /// No store: the default for tests and servers.
    pub fn null() -> Platform {
        Platform::new(PlatformConfig::default())
    }

    pub fn with_backend(config: PlatformConfig, backend: Box<dyn Backend>) -> Platform {
        Platform {
            overlay: backend.overlay_active(),
            backend,
            config,
            events: Vec::new(),
            dirty: false,
            since_store: 0.0,
            warned: HashSet::new(),
        }
    }

    pub fn config(&self) -> &PlatformConfig {
        &self.config
    }

    pub fn name(&self) -> &'static str {
        self.backend.name()
    }
    pub fn available(&self) -> bool {
        self.backend.available()
    }
    pub fn user(&self) -> String {
        self.backend.user()
    }
    pub fn language(&self) -> String {
        self.backend.language()
    }
    pub fn overlay_active(&self) -> bool {
        self.overlay
    }

    /// The achievements this game has: the declared ones, or, with none
    /// declared, whatever the store lists.
    pub fn achievement_ids(&self) -> Vec<String> {
        if self.config.achievements.is_empty() {
            self.backend.achievement_names()
        } else {
            self.config
                .achievements
                .iter()
                .map(|(id, _)| id.clone())
                .collect()
        }
    }

    pub fn is_unlocked(&self, id: &str) -> bool {
        self.backend.achievement(id)
    }

    pub fn stat_kind(&self, name: &str) -> Option<StatKind> {
        self.config
            .stats
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, k)| *k)
    }

    pub fn stat(&self, name: &str) -> Option<f64> {
        let kind = self.stat_kind(name)?;
        self.backend.stat(name, kind)
    }

    pub fn owns_dlc(&self, appid: u32) -> bool {
        self.backend.owns_dlc(appid)
    }

    /// Everything a script may read, now.
    pub fn view(&self) -> PlatformView {
        PlatformView {
            available: self.available(),
            name: self.name().to_string(),
            user: self.user(),
            language: self.language(),
            overlay: self.overlay,
            achievements: self
                .achievement_ids()
                .into_iter()
                .map(|id| {
                    let on = self.backend.achievement(&id);
                    (id, on)
                })
                .collect(),
            stats: self
                .config
                .stats
                .iter()
                .map(|(name, kind)| {
                    (
                        name.clone(),
                        self.backend.stat(name, *kind).unwrap_or_default(),
                    )
                })
                .collect(),
            dlc: self
                .config
                .dlc
                .iter()
                .map(|(id, _)| (*id, self.backend.owns_dlc(*id)))
                .collect(),
        }
    }

    fn known_achievement(&mut self, id: &str) -> Result<(), String> {
        let ids = self.achievement_ids();
        if ids.iter().any(|a| a == id) {
            return Ok(());
        }
        Err(if ids.is_empty() {
            format!(
                "achievement `{id}`: this project declares no achievements. \
                 List them in an \"achievements\" block in the .keroproj."
            )
        } else {
            format!("achievement `{id}` is not declared in the .keroproj")
        })
    }

    fn known_stat(&self, name: &str) -> Result<StatKind, String> {
        self.stat_kind(name).ok_or_else(|| {
            format!("stat `{name}` is not declared in the .keroproj \"stats\" block")
        })
    }

    /// Do what was asked. Errors are the caller's to report; each says what
    /// to change.
    pub fn apply(&mut self, action: &PlatformAction) -> Result<(), String> {
        match action {
            PlatformAction::Unlock(id) => {
                self.known_achievement(id)?;
                if self.backend.achievement(id) {
                    return Ok(());
                }
                self.backend.set_achievement(id, true)?;
                self.dirty = true;
                self.events
                    .push(PlatformEvent::AchievementUnlocked(id.clone()));
            }
            PlatformAction::Clear(id) => {
                self.known_achievement(id)?;
                if !self.backend.achievement(id) {
                    return Ok(());
                }
                self.backend.set_achievement(id, false)?;
                self.dirty = true;
                self.events
                    .push(PlatformEvent::AchievementCleared(id.clone()));
            }
            PlatformAction::Progress { id, current, max } => {
                self.known_achievement(id)?;
                if *max == 0 || self.backend.achievement(id) {
                    return Ok(());
                }
                // At or past the end is an unlock, which is what a designer
                // wiring progress to a counter means.
                if current >= max {
                    return self.apply(&PlatformAction::Unlock(id.clone()));
                }
                self.backend.indicate_progress(id, *current, *max)?;
                self.events.push(PlatformEvent::AchievementProgress {
                    id: id.clone(),
                    current: *current,
                    max: *max,
                });
            }
            PlatformAction::SetStat { name, value } => {
                let kind = self.known_stat(name)?;
                self.set_stat(name, kind, *value)?;
            }
            PlatformAction::AddStat { name, delta } => {
                let kind = self.known_stat(name)?;
                let old = self.backend.stat(name, kind).unwrap_or_default();
                self.set_stat(name, kind, old + delta)?;
            }
            PlatformAction::StoreStats => {
                self.flush()?;
            }
            PlatformAction::Presence { key, value } => self.backend.set_presence(key, value),
            PlatformAction::ClearPresence => self.backend.clear_presence(),
            PlatformAction::OpenOverlay(dialog) => self.backend.open_overlay(dialog),
            PlatformAction::OpenUrl(url) => self.backend.open_url(url),
            PlatformAction::OpenStore(appid) => self.backend.open_store(*appid),
            PlatformAction::SubmitScore {
                board,
                score,
                ascending,
            } => {
                if board.trim().is_empty() {
                    return Err("a score needs a leaderboard name".into());
                }
                self.backend.submit_score(board, *score, *ascending);
            }
            PlatformAction::CheckDlc(appid) => {
                let owned = self.backend.owns_dlc(*appid);
                self.events.push(PlatformEvent::DlcChecked {
                    appid: *appid,
                    owned,
                });
            }
        }
        Ok(())
    }

    fn set_stat(&mut self, name: &str, kind: StatKind, value: f64) -> Result<(), String> {
        let value = match kind {
            StatKind::Int => value.round(),
            StatKind::Float => value,
        };
        let old = self.backend.stat(name, kind).unwrap_or_default();
        if old == value {
            return Ok(());
        }
        self.backend.set_stat(name, kind, value)?;
        self.dirty = true;
        self.events.push(PlatformEvent::StatChanged {
            name: name.to_string(),
            old,
            new: value,
        });
        Ok(())
    }

    /// Apply, and hand back a refusal only the first time it happens: for
    /// callers -- entities, scripts -- whose only recourse is the console,
    /// and which may be asking every tick.
    pub fn apply_once(&mut self, action: &PlatformAction) -> Option<String> {
        match self.apply(action) {
            Err(e) if self.warned.insert(e.clone()) => Some(e),
            _ => None,
        }
    }

    /// Send anything changed now.
    pub fn flush(&mut self) -> Result<(), String> {
        self.since_store = 0.0;
        if !self.dirty {
            return Ok(());
        }
        self.dirty = false;
        self.backend.store_stats()
    }

    /// Once a frame: pump the store, send batched stats, and note the
    /// overlay.
    pub fn frame(&mut self, dt: f64) {
        self.backend.frame(&mut self.events);
        for event in &self.events {
            if let PlatformEvent::OverlayChanged(on) = event {
                self.overlay = *on;
            }
        }
        self.since_store += dt;
        if self.dirty
            && self.since_store >= STORE_INTERVAL
            && let Err(e) = self.flush()
        {
            log::warn!("storing stats: {e}");
        }
    }

    /// What happened since the last call.
    pub fn take_events(&mut self) -> Vec<PlatformEvent> {
        std::mem::take(&mut self.events)
    }

    /// Report that a Workshop item was mounted. The engine mounts; this only
    /// tells everyone listening.
    pub fn note_workshop_mounted(&mut self, id: u64, path: PathBuf) {
        self.events
            .push(PlatformEvent::WorkshopMounted { id, path });
    }

    pub fn cloud_enabled(&self) -> bool {
        self.backend.cloud_enabled()
    }
    pub fn cloud_write(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.backend.cloud_write(name, bytes)
    }
    pub fn cloud_read(&self, name: &str) -> Option<Vec<u8>> {
        self.backend.cloud_read(name)
    }
    pub fn workshop_items(&self) -> Vec<(u64, PathBuf)> {
        self.backend.workshop_items()
    }

    /// Reach the backend itself, for the rare call no action covers.
    pub fn backend_mut(&mut self) -> &mut dyn Backend {
        self.backend.as_mut()
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        // Stats changed in the last second would otherwise be lost on quit.
        let _ = self.flush();
    }
}

impl std::fmt::Debug for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Platform")
            .field("backend", &self.name())
            .field("available", &self.available())
            .finish()
    }
}

/// Whether to relaunch through Steam, and exit.
///
/// A game started from its folder rather than from Steam has no Steam
/// session; Valve's answer is `SteamAPI_RestartAppIfNecessary`, which starts
/// the game again through the client and asks this copy to quit. Skipped in
/// debug builds and when `steam_appid.txt` sits beside the executable,
/// which is how a developer runs the game outside Steam on purpose.
pub fn restart_through_steam(appid: u32) -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("steam_appid.txt")))
        .is_some_and(|p| p.is_file());
    if beside_exe {
        return false;
    }
    #[cfg(feature = "steam")]
    {
        steamworks::restart_app_if_necessary(steamworks::AppId(appid))
    }
    #[cfg(not(feature = "steam"))]
    {
        let _ = appid;
        false
    }
}

#[cfg(test)]
mod tests;
