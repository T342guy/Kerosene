// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

fn config() -> PlatformConfig {
    PlatformConfig {
        achievements: vec![
            ("ACH_DOOR".into(), "Open a door".into()),
            ("ACH_TEN".into(), "Ten of them".into()),
        ],
        stats: vec![
            ("doors".into(), StatKind::Int),
            ("distance".into(), StatKind::Float),
        ],
        dlc: vec![(111, "Soundtrack".into())],
        ..Default::default()
    }
}

fn platform() -> Platform {
    Platform::new(config())
}

#[test]
fn with_no_store_the_null_backend_answers() {
    let p = platform();
    assert_eq!(p.name(), "none");
    assert!(!p.available());
    let view = p.view();
    assert_eq!(view.achievements.len(), 2);
    assert_eq!(view.stats.get("doors"), Some(&0.0));
    assert_eq!(view.dlc.get(&111), Some(&false));
}

#[test]
fn unlocking_is_once_and_reports_itself() {
    let mut p = platform();
    p.apply(&PlatformAction::Unlock("ACH_DOOR".into())).unwrap();
    p.apply(&PlatformAction::Unlock("ACH_DOOR".into())).unwrap();
    assert!(p.is_unlocked("ACH_DOOR"));
    assert_eq!(
        p.take_events(),
        vec![PlatformEvent::AchievementUnlocked("ACH_DOOR".into())]
    );
    p.apply(&PlatformAction::Clear("ACH_DOOR".into())).unwrap();
    assert!(!p.is_unlocked("ACH_DOOR"));
    assert_eq!(
        p.take_events(),
        vec![PlatformEvent::AchievementCleared("ACH_DOOR".into())]
    );
}

#[test]
fn undeclared_achievements_and_stats_are_refused_with_a_reason() {
    let mut p = platform();
    let e = p.apply(&PlatformAction::Unlock("NOPE".into())).unwrap_err();
    assert!(e.contains("not declared"), "{e}");
    let e = p
        .apply(&PlatformAction::AddStat {
            name: "nope".into(),
            delta: 1.0,
        })
        .unwrap_err();
    assert!(e.contains("stats"), "{e}");

    let mut bare = Platform::null();
    let e = bare.apply(&PlatformAction::Unlock("X".into())).unwrap_err();
    assert!(e.contains("declares no achievements"), "{e}");
}

#[test]
fn stats_round_integers_and_report_old_and_new() {
    let mut p = platform();
    p.apply(&PlatformAction::AddStat {
        name: "doors".into(),
        delta: 1.4,
    })
    .unwrap();
    p.apply(&PlatformAction::AddStat {
        name: "distance".into(),
        delta: 2.5,
    })
    .unwrap();
    assert_eq!(p.stat("doors"), Some(1.0));
    assert_eq!(p.stat("distance"), Some(2.5));
    assert_eq!(
        p.take_events()[0],
        PlatformEvent::StatChanged {
            name: "doors".into(),
            old: 0.0,
            new: 1.0
        }
    );
    // Setting a stat to what it already is says nothing.
    p.apply(&PlatformAction::SetStat {
        name: "doors".into(),
        value: 1.0,
    })
    .unwrap();
    assert!(p.take_events().is_empty());
}

#[test]
fn progress_at_the_end_is_an_unlock() {
    let mut p = platform();
    p.apply(&PlatformAction::Progress {
        id: "ACH_TEN".into(),
        current: 3,
        max: 10,
    })
    .unwrap();
    p.apply(&PlatformAction::Progress {
        id: "ACH_TEN".into(),
        current: 10,
        max: 10,
    })
    .unwrap();
    let events = p.take_events();
    assert!(matches!(
        events[0],
        PlatformEvent::AchievementProgress { current: 3, .. }
    ));
    assert_eq!(
        events[1],
        PlatformEvent::AchievementUnlocked("ACH_TEN".into())
    );
}

#[test]
fn scores_come_back_on_the_next_frame_with_a_simulated_rank() {
    let mut p = platform();
    let submit = |p: &mut Platform, score, ascending| {
        p.apply(&PlatformAction::SubmitScore {
            board: "time".into(),
            score,
            ascending,
        })
        .unwrap()
    };
    submit(&mut p, 500, true);
    assert!(p.take_events().is_empty(), "asynchronous, as on Steam");
    p.frame(0.016);
    submit(&mut p, 600, true);
    p.frame(0.016);
    submit(&mut p, 400, true);
    p.frame(0.016);
    let improved: Vec<bool> = p
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            PlatformEvent::ScoreSubmitted { improved, rank, .. } => {
                assert_eq!(rank, 1);
                Some(improved)
            }
            _ => None,
        })
        .collect();
    assert_eq!(improved, vec![true, false, true]);
}

#[test]
fn dlc_checks_answer_at_once() {
    let mut null = NullBackend::default();
    null.owned_dlc.insert(111);
    let mut p = Platform::with_backend(config(), Box::new(null));
    p.apply(&PlatformAction::CheckDlc(111)).unwrap();
    p.apply(&PlatformAction::CheckDlc(222)).unwrap();
    assert_eq!(
        p.take_events(),
        vec![
            PlatformEvent::DlcChecked {
                appid: 111,
                owned: true
            },
            PlatformEvent::DlcChecked {
                appid: 222,
                owned: false
            },
        ]
    );
    assert_eq!(p.view().dlc.get(&111), Some(&true));
}

#[test]
fn the_cloud_round_trips_and_refuses_paths() {
    let dir = std::env::temp_dir().join(format!("kerosene-cloud-{}", std::process::id()));
    let mut p = Platform::new(PlatformConfig {
        local_cloud: Some(dir.clone()),
        ..Default::default()
    });
    assert!(p.cloud_enabled());
    p.cloud_write("save1.kerosave", b"hello").unwrap();
    assert_eq!(
        p.cloud_read("save1.kerosave").as_deref(),
        Some(&b"hello"[..])
    );
    assert!(p.cloud_write("../escape", b"x").is_err());
    assert!(p.cloud_read("missing").is_none());
    let _ = std::fs::remove_dir_all(dir);

    let mut memory = Platform::null();
    memory.cloud_write("a", b"1").unwrap();
    assert_eq!(memory.cloud_read("a").as_deref(), Some(&b"1"[..]));
}

#[test]
fn actions_read_back_from_their_text() {
    for text in [
        "unlock ACH_DOOR",
        "clear ACH_DOOR",
        "progress ACH_TEN 3 10",
        "set_stat distance 2.5",
        "add_stat doors 1",
        "store_stats",
        "presence status In the atrium",
        "clear_presence",
        "overlay achievements",
        "url https://example.com/a b",
        "store 480",
        "store",
        "score time 5230 asc",
        "score points 10 desc",
        "dlc 111",
    ] {
        let action = PlatformAction::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(action.to_string(), text);
    }
    assert_eq!(
        PlatformAction::parse("add_stat doors").unwrap(),
        PlatformAction::AddStat {
            name: "doors".into(),
            delta: 1.0
        }
    );
    assert!(PlatformAction::parse("unlock").is_err());
    assert!(PlatformAction::parse("fly").is_err());
    assert!(PlatformAction::parse("set_stat a b").is_err());
}

#[test]
fn stats_are_stored_in_batches() {
    #[derive(Default)]
    struct Counting {
        inner: NullBackend,
        stores: Rc<RefCell<u32>>,
    }
    impl Backend for Counting {
        fn name(&self) -> &'static str {
            "counting"
        }
        fn available(&self) -> bool {
            true
        }
        fn user(&self) -> String {
            String::new()
        }
        fn language(&self) -> String {
            String::new()
        }
        fn frame(&mut self, e: &mut Vec<PlatformEvent>) {
            self.inner.frame(e)
        }
        fn overlay_active(&self) -> bool {
            false
        }
        fn achievement(&self, id: &str) -> bool {
            self.inner.achievement(id)
        }
        fn set_achievement(&mut self, id: &str, on: bool) -> Result<(), String> {
            self.inner.set_achievement(id, on)
        }
        fn indicate_progress(&mut self, _: &str, _: u32, _: u32) -> Result<(), String> {
            Ok(())
        }
        fn stat(&self, n: &str, k: StatKind) -> Option<f64> {
            self.inner.stat(n, k)
        }
        fn set_stat(&mut self, n: &str, k: StatKind, v: f64) -> Result<(), String> {
            self.inner.set_stat(n, k, v)
        }
        fn store_stats(&mut self) -> Result<(), String> {
            *self.stores.borrow_mut() += 1;
            Ok(())
        }
        fn set_presence(&mut self, _: &str, _: &str) {}
        fn clear_presence(&mut self) {}
        fn open_overlay(&mut self, _: &str) {}
        fn open_url(&mut self, _: &str) {}
        fn open_store(&mut self, _: Option<u32>) {}
        fn submit_score(&mut self, _: &str, _: i32, _: bool) {}
        fn owns_dlc(&self, _: u32) -> bool {
            false
        }
        fn cloud_enabled(&self) -> bool {
            false
        }
        fn cloud_write(&mut self, _: &str, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn cloud_read(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn workshop_items(&self) -> Vec<(u64, PathBuf)> {
            Vec::new()
        }
    }

    let stores = Rc::new(RefCell::new(0));
    let backend = Counting {
        stores: stores.clone(),
        ..Default::default()
    };
    let mut p = Platform::with_backend(config(), Box::new(backend));
    for _ in 0..30 {
        p.apply(&PlatformAction::AddStat {
            name: "doors".into(),
            delta: 1.0,
        })
        .unwrap();
        p.frame(0.1);
    }
    // Three seconds of changes every frame: about three stores, not thirty.
    assert!((2..=4).contains(&*stores.borrow()), "{}", stores.borrow());
    drop(p);
}

#[test]
fn the_script_object_reads_the_view_and_queues_actions() {
    let mut engine = rhai::Engine::new();
    let queued = Rc::new(RefCell::new(Vec::new()));
    let q = queued.clone();
    let mut view = PlatformView {
        name: "none".into(),
        user: "tester".into(),
        ..Default::default()
    };
    view.achievements.insert("ACH_DOOR".into(), true);
    view.stats.insert("doors".into(), 4.0);
    view.dlc.insert(111, true);
    script::register(
        &mut engine,
        move || view.clone(),
        move |a| q.borrow_mut().push(a),
    );

    let got: String = engine
        .eval(
            r#"
            fn award() { steam.unlock("ACH_TEN"); }
            award();
            platform.add_stat("doors");
            platform.add_stat("distance", 2.5);
            platform.submit_score("time", 99, true);
            platform.presence("status", 3);
            platform.check_dlc(111);
            `${platform.user} ${platform.is_unlocked("ACH_DOOR")} ${platform.stat("doors")} ${platform.owns_dlc(111)} ${platform.stat("nope") == ()} ${platform.achievements.len()}`
            "#,
        )
        .unwrap();
    assert_eq!(got, "tester true 4.0 true true 1");
    assert_eq!(
        *queued.borrow(),
        vec![
            PlatformAction::Unlock("ACH_TEN".into()),
            PlatformAction::AddStat {
                name: "doors".into(),
                delta: 1.0
            },
            PlatformAction::AddStat {
                name: "distance".into(),
                delta: 2.5
            },
            PlatformAction::SubmitScore {
                board: "time".into(),
                score: 99,
                ascending: true
            },
            PlatformAction::Presence {
                key: "status".into(),
                value: "3".into()
            },
            PlatformAction::CheckDlc(111),
        ]
    );
}
