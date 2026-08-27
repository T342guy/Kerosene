// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;
use log::Log;

/// A record built by hand, since these tests must not depend on which logger
/// the test binary happens to have installed globally.
fn record(relay: &LogRelay, level: log::Level, target: &str, text: &str) {
    relay.log(&log::Record::builder().level(level).target(target).args(format_args!("{text}")).build());
}

fn relay() -> LogRelay { LogRelay::new(log::LevelFilter::Debug) }

#[test]
fn a_record_from_the_game_reaches_the_console() {
    // The gap this closes: a door reporting a missing target logged to a
    // terminal nobody was watching, while the console showed nothing.
    let relay = relay();
    record(&relay, log::Level::Warn, "void_game", "func_door has no target");

    let (lines, dropped) = relay.take();
    assert_eq!(dropped, 0);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].level, LogLevel::Warning);
    assert_eq!(lines[0].text, "func_door has no target");
}

#[test]
fn taking_twice_does_not_repeat_a_line() {
    let relay = relay();
    record(&relay, log::Level::Info, "void_game", "once");
    assert_eq!(relay.take().0.len(), 1);
    assert!(relay.take().0.is_empty());
}

#[test]
fn the_consoles_own_output_is_not_queued_back_to_it() {
    // `Console::print` forwards to `log`, so without this every console line
    // would come back round and appear twice in the scrollback.
    let relay = relay();
    record(&relay, log::Level::Info, "void_console", "already in the scrollback");
    assert!(relay.take().0.is_empty());
}

#[test]
fn levels_map_onto_the_consoles_own() {
    let relay = relay();
    record(&relay, log::Level::Error, "void_bsp", "e");
    record(&relay, log::Level::Warn, "void_bsp", "w");
    record(&relay, log::Level::Info, "void_bsp", "i");
    record(&relay, log::Level::Debug, "void_bsp", "d");
    let levels: Vec<LogLevel> = relay.take().0.into_iter().map(|l| l.level).collect();
    assert_eq!(
        levels,
        [LogLevel::Error, LogLevel::Warning, LogLevel::Info, LogLevel::Developer]
    );
}

#[test]
fn a_record_below_the_level_is_not_kept() {
    let relay = LogRelay::new(log::LevelFilter::Warn);
    record(&relay, log::Level::Info, "void_game", "chatter");
    record(&relay, log::Level::Error, "void_game", "trouble");
    let lines = relay.take().0;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "trouble");
}

#[test]
fn a_flood_is_bounded_and_reported_rather_than_silently_truncated() {
    // A loop logging every iteration must not be able to exhaust memory
    // before the next frame drains the queue -- but it must also not look
    // like it logged nothing.
    let relay = relay();
    for i in 0..MAX_PENDING + 50 {
        record(&relay, log::Level::Info, "void_game", &format!("line {i}"));
    }
    let (lines, dropped) = relay.take();
    assert_eq!(lines.len(), MAX_PENDING);
    assert_eq!(dropped, 50);
    // The count resets once reported, so the next frame is not told again.
    assert_eq!(relay.take().1, 0);
}

#[test]
fn a_log_file_gets_every_line_including_the_consoles_own() {
    let dir = std::env::temp_dir().join(format!("void-log-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("void.log");

    let relay = relay();
    relay.open_file(&path).expect("the log file opens");
    assert!(relay.has_file());
    record(&relay, log::Level::Info, "void_game", "from the game");
    record(&relay, log::Level::Info, "void_console", "from the console");
    relay.flush();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("from the game"), "{text}");
    assert!(text.contains("from the console"), "the file is the whole record: {text}");

    relay.close_file();
    assert!(!relay.has_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opening_a_log_file_somewhere_impossible_reports_rather_than_logging() {
    // The thing that would report the failure is the thing that just failed.
    let relay = relay();
    let err = relay.open_file(std::path::Path::new("/definitely/not/here/void.log"));
    assert!(err.is_err());
    assert!(!relay.has_file());
}

#[test]
fn the_environment_can_choose_the_level_but_nonsense_does_not() {
    // Only a bare level is understood; anything else keeps the default rather
    // than turning logging off by accident.
    unsafe { std::env::set_var("RUST_LOG", "warn") };
    assert_eq!(level_from_env(log::LevelFilter::Info), log::LevelFilter::Warn);
    unsafe { std::env::set_var("RUST_LOG", "void_bsp=trace") };
    assert_eq!(level_from_env(log::LevelFilter::Info), log::LevelFilter::Info);
    unsafe { std::env::remove_var("RUST_LOG") };
    assert_eq!(level_from_env(log::LevelFilter::Info), log::LevelFilter::Info);
}
