// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The `platform` object scripts see -- also reachable as `steam`.
//!
//! Registered in both script VMs, a map's (`kerosene-script`) and a UI
//! document's (`kerosene-ui`), with the same methods, so a line copied from
//! one works in the other:
//!
//! ```text
//! platform.unlock("ACH_FIRST_DOOR");
//! steam.add_stat("doors_opened", 1);
//! if platform.is_unlocked("ACH_FIRST_DOOR") { ... }
//! platform.submit_score("atrium_time", 5230, true);   // lower is better
//! platform.presence("status", "Exploring the atrium");
//! if platform.owns_dlc(1234560) { ... }
//! print(platform.user);
//! ```
//!
//! | Read | |
//! |---|---|
//! | `.available` `.name` `.user` `.language` `.overlay` | The store, the player |
//! | `.achievements` `.stats` | Maps of every declared one |
//! | `.is_unlocked(id)` `.stat(name)` `.owns_dlc(appid)` | One of them |
//!
//! | Do | |
//! |---|---|
//! | `.unlock(id)` `.clear(id)` `.progress(id, current, max)` | Achievements |
//! | `.set_stat(name, v)` `.add_stat(name, d)` `.store_stats()` | Stats |
//! | `.presence(key, value)` `.clear_presence()` | Rich presence |
//! | `.open_overlay(dialog)` `.open_url(url)` `.open_store()` `.open_store(appid)` | The overlay |
//! | `.submit_score(board, score)` `.submit_score(board, score, ascending)` | Leaderboards |
//! | `.check_dlc(appid)` | Ask, and hear back in `on_platform_event` |
//!
//! Reads come from a snapshot taken when the script began, like every other
//! read a script makes; writes become [`PlatformAction`]s the engine applies
//! afterwards. Results come back as events: `on_platform_event(name, data)`
//! in a map script, `on_event` / `on:achievement_unlocked=` in a layout.
//!
//! The object is resolved by name wherever it is used, including inside a
//! script's own functions, which cannot otherwise see anything defined at
//! the top of the file. The price is that `platform` and `steam` cannot be
//! used as variable names.

use crate::{PlatformAction, PlatformView};
use rhai::{Dynamic, Engine, FLOAT, INT, Map};
use std::rc::Rc;

/// The value `platform` and `steam` evaluate to. Carries nothing: every
/// method reads through the view and writes through the sink it was
/// registered with.
#[derive(Clone, Copy, Debug)]
pub struct ScriptPlatform;

/// The names the object answers to.
pub const NAMES: [&str; 2] = ["platform", "steam"];

type View = Rc<dyn Fn() -> PlatformView>;
type Sink = Rc<dyn Fn(PlatformAction)>;

/// Add the `platform` object to a script VM.
///
/// `view` reads the snapshot the running script sees; `sink` queues what it
/// asks for. This sets the engine's variable resolver, so a VM can have only
/// one caller of it.
pub fn register(
    engine: &mut Engine,
    view: impl Fn() -> PlatformView + 'static,
    sink: impl Fn(PlatformAction) + 'static,
) {
    let view: View = Rc::new(view);
    let sink: Sink = Rc::new(sink);

    engine.register_type_with_name::<ScriptPlatform>("Platform");
    #[allow(deprecated)]
    engine.on_var(|name, _, _| Ok(NAMES.contains(&name).then(|| Dynamic::from(ScriptPlatform))));

    macro_rules! get {
        ($name:literal, |$v:ident| $body:expr) => {{
            let view = view.clone();
            engine.register_get($name, move |_: &mut ScriptPlatform| {
                let $v = view();
                $body
            });
        }};
    }
    get!("available", |v| v.available);
    get!("name", |v| v.name);
    get!("user", |v| v.user);
    get!("language", |v| v.language);
    get!("overlay", |v| v.overlay);
    get!("achievements", |v| {
        v.achievements
            .into_iter()
            .map(|(k, on)| (k.into(), Dynamic::from(on)))
            .collect::<Map>()
    });
    get!("stats", |v| {
        v.stats
            .into_iter()
            .map(|(k, n)| (k.into(), Dynamic::from(n as FLOAT)))
            .collect::<Map>()
    });

    let v = view.clone();
    engine.register_fn("is_unlocked", move |_: &mut ScriptPlatform, id: &str| {
        v().achievements.get(id).copied().unwrap_or(false)
    });
    let v = view.clone();
    engine.register_fn(
        "stat",
        move |_: &mut ScriptPlatform, name: &str| -> Dynamic {
            v().stats
                .get(name)
                .map_or(Dynamic::UNIT, |n| Dynamic::from(*n as FLOAT))
        },
    );
    let v = view.clone();
    engine.register_fn("owns_dlc", move |_: &mut ScriptPlatform, appid: INT| {
        v().dlc.get(&(appid as u32)).copied().unwrap_or(false)
    });

    macro_rules! act {
        // rustfmt writes an empty `| |` as `||`, which is one token.
        ($name:literal, || $action:expr) => {
            act!($name, | | $action)
        };
        ($name:literal, |$($arg:ident : $ty:ty),*| $action:expr) => {{
            let sink = sink.clone();
            engine.register_fn($name, move |_: &mut ScriptPlatform $(, $arg: $ty)*| {
                sink($action)
            });
        }};
    }
    act!("unlock", |id: &str| PlatformAction::Unlock(id.to_string()));
    act!("clear", |id: &str| PlatformAction::Clear(id.to_string()));
    act!("progress", |id: &str, current: INT, max: INT| {
        PlatformAction::Progress {
            id: id.to_string(),
            current: current.max(0) as u32,
            max: max.max(0) as u32,
        }
    });
    act!("set_stat", |name: &str, value: INT| {
        PlatformAction::SetStat {
            name: name.to_string(),
            value: value as f64,
        }
    });
    act!("set_stat", |name: &str, value: FLOAT| {
        PlatformAction::SetStat {
            name: name.to_string(),
            value,
        }
    });
    act!("add_stat", |name: &str| PlatformAction::AddStat {
        name: name.to_string(),
        delta: 1.0,
    });
    act!("add_stat", |name: &str, delta: INT| {
        PlatformAction::AddStat {
            name: name.to_string(),
            delta: delta as f64,
        }
    });
    act!("add_stat", |name: &str, delta: FLOAT| {
        PlatformAction::AddStat {
            name: name.to_string(),
            delta,
        }
    });
    act!("store_stats", || PlatformAction::StoreStats);
    act!("presence", |key: &str, value: Dynamic| {
        PlatformAction::Presence {
            key: key.to_string(),
            value: value.to_string(),
        }
    });
    act!("clear_presence", || PlatformAction::ClearPresence);
    act!("open_overlay", |dialog: &str| PlatformAction::OpenOverlay(
        dialog.to_string()
    ));
    act!("open_url", |url: &str| PlatformAction::OpenUrl(
        url.to_string()
    ));
    act!("open_store", || PlatformAction::OpenStore(None));
    act!("open_store", |appid: INT| PlatformAction::OpenStore(Some(
        appid as u32
    )));
    act!("submit_score", |board: &str, score: INT| {
        PlatformAction::SubmitScore {
            board: board.to_string(),
            score: score as i32,
            ascending: false,
        }
    });
    act!("submit_score", |board: &str,
                          score: INT,
                          ascending: bool| {
        PlatformAction::SubmitScore {
            board: board.to_string(),
            score: score as i32,
            ascending,
        }
    });
    act!("check_dlc", |appid: INT| PlatformAction::CheckDlc(
        appid as u32
    ));

    engine.register_fn("to_string", |_: &mut ScriptPlatform| "platform".to_string());
}
