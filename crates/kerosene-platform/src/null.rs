// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! No store: what a game runs on off Steam, in tests, and on a server.
//!
//! It behaves like a store rather than refusing everything, so a map that
//! awards an achievement, bumps a stat and posts a score does all three here
//! and every output wired to the result fires. Achievements and stats live
//! for the session; "cloud" files go to a local directory when one is given.

use crate::{Backend, PlatformEvent, StatKind};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Default, Debug)]
pub struct NullBackend {
    pub achievements: HashSet<String>,
    pub stats: HashMap<String, f64>,
    /// Best score per leaderboard, for simulated ranks.
    pub boards: HashMap<String, i32>,
    /// DLC to report as installed. None, unless a test says otherwise.
    pub owned_dlc: HashSet<u32>,
    /// Where cloud files go; `None` keeps them in `memory`.
    pub cloud_dir: Option<PathBuf>,
    pub memory: HashMap<String, Vec<u8>>,
    /// What `open_overlay` and friends were asked, newest last.
    pub requests: Vec<String>,
    pending: Vec<PlatformEvent>,
}

impl NullBackend {
    pub fn new(cloud_dir: Option<PathBuf>) -> NullBackend {
        NullBackend {
            cloud_dir,
            ..Default::default()
        }
    }
}

/// A cloud file name that cannot climb out of its directory.
fn safe_name(name: &str) -> Result<&str, String> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') || name.contains("..")
    {
        return Err(format!("`{name}` is not a valid cloud file name"));
    }
    Ok(name)
}

impl Backend for NullBackend {
    fn name(&self) -> &'static str {
        "none"
    }
    fn available(&self) -> bool {
        false
    }
    fn user(&self) -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "Player".to_string())
    }
    fn language(&self) -> String {
        "english".to_string()
    }
    fn frame(&mut self, events: &mut Vec<PlatformEvent>) {
        events.append(&mut self.pending);
    }
    fn overlay_active(&self) -> bool {
        false
    }

    fn achievement(&self, id: &str) -> bool {
        self.achievements.contains(id)
    }
    fn set_achievement(&mut self, id: &str, unlocked: bool) -> Result<(), String> {
        if unlocked {
            self.achievements.insert(id.to_string());
        } else {
            self.achievements.remove(id);
        }
        Ok(())
    }
    fn indicate_progress(&mut self, _: &str, _: u32, _: u32) -> Result<(), String> {
        Ok(())
    }

    fn stat(&self, name: &str, _: StatKind) -> Option<f64> {
        Some(self.stats.get(name).copied().unwrap_or_default())
    }
    fn set_stat(&mut self, name: &str, _: StatKind, value: f64) -> Result<(), String> {
        self.stats.insert(name.to_string(), value);
        Ok(())
    }
    fn store_stats(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn set_presence(&mut self, key: &str, value: &str) {
        self.requests.push(format!("presence {key} {value}"));
    }
    fn clear_presence(&mut self) {
        self.requests.push("clear_presence".to_string());
    }
    fn open_overlay(&mut self, dialog: &str) {
        log::info!("no store overlay to open ({dialog})");
        self.requests.push(format!("overlay {dialog}"));
    }
    fn open_url(&mut self, url: &str) {
        log::info!("no store overlay to open {url} in");
        self.requests.push(format!("url {url}"));
    }
    fn open_store(&mut self, appid: Option<u32>) {
        self.requests.push(format!("store {}", appid.unwrap_or(0)));
    }

    /// Ranks are simulated: one player, so a new best is always rank 1, and
    /// whether it is a new best follows the board's sort order.
    fn submit_score(&mut self, board: &str, score: i32, ascending: bool) {
        let previous = self.boards.get(board).copied();
        let improved = match previous {
            None => true,
            Some(p) if ascending => score < p,
            Some(p) => score > p,
        };
        if improved {
            self.boards.insert(board.to_string(), score);
        }
        self.pending.push(PlatformEvent::ScoreSubmitted {
            board: board.to_string(),
            score,
            rank: 1,
            improved,
        });
    }

    fn owns_dlc(&self, appid: u32) -> bool {
        self.owned_dlc.contains(&appid)
    }

    fn cloud_enabled(&self) -> bool {
        self.cloud_dir.is_some()
    }
    fn cloud_write(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let name = safe_name(name)?;
        match &self.cloud_dir {
            Some(dir) => {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                std::fs::write(dir.join(name), bytes).map_err(|e| e.to_string())
            }
            None => {
                self.memory.insert(name.to_string(), bytes.to_vec());
                Ok(())
            }
        }
    }
    fn cloud_read(&self, name: &str) -> Option<Vec<u8>> {
        let name = safe_name(name).ok()?;
        match &self.cloud_dir {
            Some(dir) => std::fs::read(dir.join(name)).ok(),
            None => self.memory.get(name).cloned(),
        }
    }

    fn workshop_items(&self) -> Vec<(u64, PathBuf)> {
        Vec::new()
    }
}
