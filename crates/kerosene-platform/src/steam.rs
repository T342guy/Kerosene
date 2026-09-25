// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Steam, through Valve's Steamworks SDK.
//!
//! Only compiled with the `steam` feature. The SDK arrives through the
//! `steamworks` crate (MIT/Apache bindings over Valve's own `steam_api`
//! library), which links `libsteam_api.so` / `steam_api64.dll` /
//! `libsteam_api.dylib` dynamically. `cargo run` finds that library in the
//! build directory; a shipped game needs it beside the executable, which is
//! what `kiln --ship --steam` does.
//!
//! Callbacks are pumped with `process_callbacks` from the engine's frame, on
//! the main thread. The asynchronous calls -- finding a leaderboard, posting
//! to one -- answer on the same thread through a channel, so nothing here is
//! shared between threads.

use crate::{Backend, PlatformEvent, StatKind};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use steamworks::{
    AppId, CallbackResult, Client, Leaderboard, LeaderboardDisplayType, LeaderboardScoreUploaded,
    LeaderboardSortMethod, OverlayToStoreFlag, UploadScoreMethod,
};

/// What an asynchronous call came back with.
enum Answer {
    Board(String, Option<Leaderboard>),
    Uploaded {
        board: String,
        score: i32,
        result: Option<LeaderboardScoreUploaded>,
    },
}

pub struct SteamBackend {
    client: Client,
    appid: u32,
    overlay: bool,
    boards: HashMap<String, Leaderboard>,
    /// Boards being looked up, so a burst of scores asks once.
    finding: HashSet<String>,
    /// Scores waiting for their board to be found.
    waiting: Vec<(String, i32, bool)>,
    tx: Sender<Answer>,
    rx: Receiver<Answer>,
}

impl SteamBackend {
    /// Connect to the running Steam client as `appid`.
    pub fn init(appid: u32) -> Result<SteamBackend, String> {
        let client = Client::init_app(AppId(appid)).map_err(|e| e.to_string())?;
        let (tx, rx) = channel();
        Ok(SteamBackend {
            client,
            appid,
            overlay: false,
            boards: HashMap::new(),
            finding: HashSet::new(),
            waiting: Vec::new(),
            tx,
            rx,
        })
    }

    /// The client, for what [`Backend`] does not cover.
    pub fn client(&self) -> &Client {
        &self.client
    }

    fn upload(&self, board: &str, score: i32) {
        let Some(handle) = self.boards.get(board) else {
            return;
        };
        let tx = self.tx.clone();
        let name = board.to_string();
        self.client.user_stats().upload_leaderboard_score(
            handle,
            UploadScoreMethod::KeepBest,
            score,
            &[],
            move |result| {
                let _ = tx.send(Answer::Uploaded {
                    board: name,
                    score,
                    result: result.ok().flatten(),
                });
            },
        );
    }
}

impl Backend for SteamBackend {
    fn name(&self) -> &'static str {
        "steam"
    }
    fn available(&self) -> bool {
        true
    }
    fn user(&self) -> String {
        self.client.friends().name()
    }
    fn language(&self) -> String {
        self.client.apps().current_game_language()
    }

    fn frame(&mut self, events: &mut Vec<PlatformEvent>) {
        let mut overlay = None;
        self.client.process_callbacks(|result| {
            if let CallbackResult::GameOverlayActivated(o) = result {
                overlay = Some(o.active);
            }
        });
        if let Some(on) = overlay
            && on != self.overlay
        {
            self.overlay = on;
            events.push(PlatformEvent::OverlayChanged(on));
        }

        while let Ok(answer) = self.rx.try_recv() {
            match answer {
                Answer::Board(name, found) => {
                    self.finding.remove(&name);
                    let waiting: Vec<(String, i32, bool)> = {
                        let (mine, rest) = std::mem::take(&mut self.waiting)
                            .into_iter()
                            .partition(|(b, _, _)| *b == name);
                        self.waiting = rest;
                        mine
                    };
                    match found {
                        Some(handle) => {
                            self.boards.insert(name, handle);
                            for (board, score, _) in waiting {
                                self.upload(&board, score);
                            }
                        }
                        None => {
                            log::warn!("steam: leaderboard `{name}` could not be found or made");
                            for (board, _, _) in waiting {
                                events.push(PlatformEvent::ScoreFailed { board });
                            }
                        }
                    }
                }
                Answer::Uploaded {
                    board,
                    score,
                    result,
                } => events.push(match result {
                    Some(r) => PlatformEvent::ScoreSubmitted {
                        board,
                        score,
                        rank: r.global_rank_new,
                        improved: r.was_changed,
                    },
                    None => PlatformEvent::ScoreFailed { board },
                }),
            }
        }
    }

    fn overlay_active(&self) -> bool {
        self.overlay
    }

    fn achievement(&self, id: &str) -> bool {
        self.client
            .user_stats()
            .achievement(id)
            .get()
            .unwrap_or(false)
    }
    fn set_achievement(&mut self, id: &str, unlocked: bool) -> Result<(), String> {
        let stats = self.client.user_stats();
        let a = stats.achievement(id);
        let done = if unlocked { a.set() } else { a.clear() };
        done.map_err(|_| format!("steam refused achievement `{id}`; is it defined for the app?"))
    }
    fn achievement_names(&self) -> Vec<String> {
        self.client
            .user_stats()
            .get_achievement_names()
            .unwrap_or_default()
    }
    fn indicate_progress(&mut self, id: &str, current: u32, max: u32) -> Result<(), String> {
        let name = std::ffi::CString::new(id).map_err(|e| e.to_string())?;
        // Not wrapped by the `steamworks` crate; the flat API is.
        // SAFETY: the interface pointer comes from the SDK, which is
        // initialised for as long as `self.client` lives, and the name is a
        // valid C string that outlives the call.
        let ok = unsafe {
            let stats = steamworks::sys::SteamAPI_SteamUserStats_v013();
            steamworks::sys::SteamAPI_ISteamUserStats_IndicateAchievementProgress(
                stats,
                name.as_ptr(),
                current,
                max,
            )
        };
        if ok {
            Ok(())
        } else {
            Err(format!("steam refused progress on `{id}`"))
        }
    }

    fn stat(&self, name: &str, kind: StatKind) -> Option<f64> {
        let stats = self.client.user_stats();
        match kind {
            StatKind::Int => stats.get_stat_i32(name).ok().map(f64::from),
            StatKind::Float => stats.get_stat_f32(name).ok().map(f64::from),
        }
    }
    fn set_stat(&mut self, name: &str, kind: StatKind, value: f64) -> Result<(), String> {
        let stats = self.client.user_stats();
        let done = match kind {
            StatKind::Int => stats.set_stat_i32(name, value as i32),
            StatKind::Float => stats.set_stat_f32(name, value as f32),
        };
        done.map_err(|_| format!("steam refused stat `{name}`; is it defined, and of that type?"))
    }
    fn store_stats(&mut self) -> Result<(), String> {
        self.client
            .user_stats()
            .store_stats()
            .map_err(|_| "steam refused to store stats".to_string())
    }

    fn set_presence(&mut self, key: &str, value: &str) {
        let value = (!value.is_empty()).then_some(value);
        if !self.client.friends().set_rich_presence(key, value) {
            log::warn!("steam: rich presence `{key}` was refused");
        }
    }
    fn clear_presence(&mut self) {
        self.client.friends().clear_rich_presence();
    }

    fn open_overlay(&mut self, dialog: &str) {
        self.client.friends().activate_game_overlay(dialog);
    }
    fn open_url(&mut self, url: &str) {
        self.client.friends().activate_game_overlay_to_web_page(url);
    }
    fn open_store(&mut self, appid: Option<u32>) {
        self.client.friends().activate_game_overlay_to_store(
            AppId(appid.unwrap_or(self.appid)),
            OverlayToStoreFlag::None,
        );
    }

    fn submit_score(&mut self, board: &str, score: i32, ascending: bool) {
        if self.boards.contains_key(board) {
            self.upload(board, score);
            return;
        }
        self.waiting.push((board.to_string(), score, ascending));
        if !self.finding.insert(board.to_string()) {
            return;
        }
        let tx = self.tx.clone();
        let name = board.to_string();
        self.client.user_stats().find_or_create_leaderboard(
            board,
            if ascending {
                LeaderboardSortMethod::Ascending
            } else {
                LeaderboardSortMethod::Descending
            },
            LeaderboardDisplayType::Numeric,
            move |found| {
                let _ = tx.send(Answer::Board(name, found.ok().flatten()));
            },
        );
    }

    fn owns_dlc(&self, appid: u32) -> bool {
        self.client.apps().is_dlc_installed(AppId(appid))
    }

    fn cloud_enabled(&self) -> bool {
        let rs = self.client.remote_storage();
        rs.is_cloud_enabled_for_account() && rs.is_cloud_enabled_for_app()
    }
    fn cloud_write(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let mut writer = self.client.remote_storage().file(name).write();
        writer.write_all(bytes).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
    }
    fn cloud_read(&self, name: &str) -> Option<Vec<u8>> {
        let file = self.client.remote_storage().file(name);
        if !file.exists() {
            return None;
        }
        let mut bytes = Vec::new();
        file.read().read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    fn workshop_items(&self) -> Vec<(u64, PathBuf)> {
        let ugc = self.client.ugc();
        ugc.subscribed_items(false)
            .into_iter()
            .filter_map(|id| {
                let info = ugc.item_install_info(id)?;
                Some((id.0, PathBuf::from(info.folder)))
            })
            .collect()
    }
}
