// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Entities that reach the store: Steam when the game has it, nothing
//! otherwise -- and the same map works on both.
//!
//! | Class | What it does |
//! |---|---|
//! | `logic_achievement` | Award, clear, or show progress toward one achievement |
//! | `logic_stat` | Set or add to a stat; fire at a threshold, and optionally award an achievement there |
//! | `logic_leaderboard` | Post a score; hear back the rank |
//! | `logic_richpresence` | What the player's friends see them doing |
//! | `logic_platform` | Whether a store is there; the overlay; DLC |
//!
//! Like every class that reaches past the entity world, none of these calls
//! the store. Each leaves a [`host_requests::PLATFORM`] request holding a
//! platform action in its one-line text form, and the engine applies it.
//! The *results* -- `OnUnlocked`, `OnThreshold`, `OnRankImproved`,
//! `OnOverlayOpened` -- are fired by the engine when the store reports
//! them, on every entity of the class that names the same achievement,
//! stat or board. So an achievement a script awards still fires the
//! `logic_achievement` wired to it, and a result that arrives a second later
//! from Steam's servers lands where the designer expects.

use kerosene_entity::io::InputEvent;
use kerosene_entity::{ClassDef, ClassRegistry, EntityId, EntityWorld, Value, host_requests};

pub fn register(registry: &mut ClassRegistry) {
    registry.register(
        ClassDef::new("logic_achievement")
            .input("Unlock", |w, id, e| {
                ask(w, id, e, |a, _| format!("unlock {a}"))
            })
            .input("Clear", |w, id, e| {
                ask(w, id, e, |a, _| format!("clear {a}"))
            })
            .input("SetProgress", set_progress)
            .output("OnUnlocked"),
    );
    registry.register(
        ClassDef::new("logic_stat")
            .input("Set", |w, id, e| stat(w, id, e, "set_stat", None))
            .input("Add", |w, id, e| stat(w, id, e, "add_stat", Some("1")))
            .input("Increment", |w, id, e| {
                stat(
                    w,
                    id,
                    &InputEvent {
                        parameter: "1".into(),
                        ..e.clone()
                    },
                    "add_stat",
                    None,
                )
            })
            .input("Store", |w, id, e| {
                w.request(host_requests::PLATFORM, "store_stats", id, e.activator);
                true
            })
            .output("OnChanged")
            .output("OnThreshold"),
    );
    registry.register(
        ClassDef::new("logic_leaderboard")
            .input("Submit", submit)
            .output("OnSubmitted")
            .output("OnRankImproved")
            .output("OnFailed"),
    );
    registry.register(
        ClassDef::new("logic_richpresence")
            .on_spawn(|w, id| {
                let status = text(w, id, "status");
                if !status.is_empty() {
                    w.request(
                        host_requests::PLATFORM,
                        format!("presence status {status}"),
                        id,
                        None,
                    );
                }
            })
            .input("SetStatus", |w, id, e| {
                let status = e.parameter.trim();
                let action = if status.is_empty() {
                    "presence status".to_string()
                } else {
                    format!("presence status {status}")
                };
                w.request(host_requests::PLATFORM, action, id, e.activator);
                true
            })
            .input("SetKey", set_presence_key)
            .input("Clear", |w, id, e| {
                w.request(host_requests::PLATFORM, "clear_presence", id, e.activator);
                true
            }),
    );
    registry.register(
        ClassDef::new("logic_platform")
            .on_spawn(|w, id| w.request(host_requests::PLATFORM_STATUS, "", id, None))
            .input("Refresh", |w, id, e| {
                w.request(host_requests::PLATFORM_STATUS, "", id, e.activator);
                true
            })
            .input("OpenOverlay", |w, id, e| {
                let dialog = e.parameter.trim();
                let dialog = if dialog.is_empty() { "friends" } else { dialog };
                w.request(
                    host_requests::PLATFORM,
                    format!("overlay {dialog}"),
                    id,
                    e.activator,
                );
                true
            })
            .input("OpenUrl", |w, id, e| {
                let url = e.parameter.trim();
                if url.is_empty() {
                    log::warn!("logic_platform OpenUrl: needs an address");
                    return false;
                }
                w.request(
                    host_requests::PLATFORM,
                    format!("url {url}"),
                    id,
                    e.activator,
                );
                true
            })
            .input("OpenStore", |w, id, e| {
                let appid = e.parameter.trim();
                w.request(
                    host_requests::PLATFORM,
                    format!("store {appid}").trim().to_string(),
                    id,
                    e.activator,
                );
                true
            })
            .input("CheckDlc", check_dlc)
            .output("OnAvailable")
            .output("OnUnavailable")
            .output("OnOverlayOpened")
            .output("OnOverlayClosed")
            .output("OnDlcOwned")
            .output("OnDlcNotOwned"),
    );
}

/// A text key, trimmed; empty when unset.
pub fn text(world: &EntityWorld, id: EntityId, key: &str) -> String {
    world
        .get(id)
        .and_then(|e| e.fields.text(key).map(|t| t.trim().to_string()))
        .unwrap_or_default()
}

/// Ask for an action about this entity's achievement, if it names one.
fn ask(
    world: &mut EntityWorld,
    id: EntityId,
    event: &InputEvent,
    action: impl Fn(&str, &str) -> String,
) -> bool {
    let achievement = text(world, id, "achievement");
    if achievement.is_empty() {
        log::warn!("logic_achievement with no achievement id");
        return false;
    }
    world.request(
        host_requests::PLATFORM,
        action(&achievement, event.parameter.trim()),
        id,
        event.activator,
    );
    true
}

/// `SetProgress <current>` against the `progressmax` key, or
/// `SetProgress <current> <max>`.
fn set_progress(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let default_max = world
        .get(id)
        .map(|e| e.fields.i32("progressmax", 0))
        .unwrap_or(0);
    let mut words = event.parameter.split_whitespace();
    let Some(current) = words.next().and_then(|w| w.parse::<f32>().ok()) else {
        log::warn!("logic_achievement SetProgress: needs a number");
        return false;
    };
    let max = words
        .next()
        .and_then(|w| w.parse::<f32>().ok())
        .map_or(default_max, |m| m as i32);
    if max <= 0 {
        log::warn!("logic_achievement SetProgress: set progressmax, or pass `current max`");
        return false;
    }
    ask(world, id, event, |a, _| {
        format!("progress {a} {} {max}", current.max(0.0) as u32)
    })
}

/// Set or add to this entity's stat. The parameter is the amount; with none,
/// `default` if there is one.
fn stat(
    world: &mut EntityWorld,
    id: EntityId,
    event: &InputEvent,
    verb: &str,
    default: Option<&str>,
) -> bool {
    let name = text(world, id, "stat");
    if name.is_empty() {
        log::warn!("logic_stat with no stat name");
        return false;
    }
    let amount = match (event.parameter.trim(), default) {
        ("", Some(d)) => d.to_string(),
        ("", None) => {
            log::warn!("logic_stat {verb}: needs a number");
            return false;
        }
        (p, _) => p.to_string(),
    };
    if amount.parse::<f64>().is_err() {
        log::warn!("logic_stat: `{amount}` is not a number");
        return false;
    }
    world.request(
        host_requests::PLATFORM,
        format!("{verb} {name} {amount}"),
        id,
        event.activator,
    );
    true
}

fn submit(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let board = text(world, id, "leaderboard");
    if board.is_empty() {
        log::warn!("logic_leaderboard with no leaderboard name");
        return false;
    }
    let Ok(score) = event.parameter.trim().parse::<f64>() else {
        log::warn!("logic_leaderboard Submit: needs a score");
        return false;
    };
    let sort = if text(world, id, "sort").eq_ignore_ascii_case("asc") {
        "asc"
    } else {
        "desc"
    };
    world.request(
        host_requests::PLATFORM,
        format!("score {board} {} {sort}", score.round() as i64),
        id,
        event.activator,
    );
    true
}

/// `SetKey key value` (or `key=value`).
fn set_presence_key(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let p = event.parameter.trim();
    let (key, value) = p
        .split_once('=')
        .or_else(|| p.split_once(char::is_whitespace))
        .unwrap_or((p, ""));
    let key = key.trim();
    if key.is_empty() {
        log::warn!("logic_richpresence SetKey: needs a key");
        return false;
    }
    world.request(
        host_requests::PLATFORM,
        format!("presence {key} {}", value.trim()),
        id,
        event.activator,
    );
    true
}

/// Ask about a DLC: the parameter's app id, or the `dlc` key. The answer
/// comes back to this entity (and any other naming the same DLC) as
/// `OnDlcOwned` or `OnDlcNotOwned`.
fn check_dlc(world: &mut EntityWorld, id: EntityId, event: &InputEvent) -> bool {
    let given = event.parameter.trim();
    let appid = if given.is_empty() {
        text(world, id, "dlc")
    } else {
        given.to_string()
    };
    let Ok(appid) = appid.parse::<u32>() else {
        log::warn!("logic_platform CheckDlc: needs an app id, or set the dlc key");
        return false;
    };
    if let Some(e) = world.get_mut(id) {
        e.fields.set("__checking", Value::Int(appid as i32));
    }
    world.request(
        host_requests::PLATFORM,
        format!("dlc {appid}"),
        id,
        event.activator,
    );
    true
}
