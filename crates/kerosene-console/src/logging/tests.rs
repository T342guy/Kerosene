// SPDX-License-Identifier: MPL-2.0
use super::*;
use log::Log;

/// A record built by hand, since these tests must not depend on which logger
/// the test binary happens to have installed globally.
fn record(relay: &LogRelay, level: log::Level, target: &str, text: &str) {
    relay.log(&log::Record::builder().level(level).target(target).args(format_args!("{text}")).build());
}

fn relay() -> LogRelay { LogRelay::detached_uniform(log::LevelFilter::Debug) }

#[test]
fn a_record_from_the_game_reaches_the_console() {
    // The gap this closes: a door reporting a missing target logged to a
    // terminal nobody was watching, while the console showed nothing.
    let relay = relay();
    record(&relay, log::Level::Warn, "kerosene_game", "func_door has no target");

    let (lines, dropped) = relay.take();
    assert_eq!(dropped, 0);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].level, LogLevel::Warning);
    assert_eq!(lines[0].text, "func_door has no target");
}

#[test]
fn taking_twice_does_not_repeat_a_line() {
    let relay = relay();
    record(&relay, log::Level::Info, "kerosene_game", "once");
    assert_eq!(relay.take().0.len(), 1);
    assert!(relay.take().0.is_empty());
}

#[test]
fn the_consoles_own_output_is_not_queued_back_to_it() {
    // `Console::print` forwards to `log`, so without this every console line
    // would come back round and appear twice in the scrollback.
    let relay = relay();
    record(&relay, log::Level::Info, "kerosene_console", "already in the scrollback");
    assert!(relay.take().0.is_empty());
}

#[test]
fn levels_map_onto_the_consoles_own() {
    let relay = relay();
    record(&relay, log::Level::Error, "kerosene_bsp", "e");
    record(&relay, log::Level::Warn, "kerosene_bsp", "w");
    record(&relay, log::Level::Info, "kerosene_bsp", "i");
    record(&relay, log::Level::Debug, "kerosene_bsp", "d");
    let levels: Vec<LogLevel> = relay.take().0.into_iter().map(|l| l.level).collect();
    assert_eq!(
        levels,
        [LogLevel::Error, LogLevel::Warning, LogLevel::Info, LogLevel::Developer]
    );
}

#[test]
fn a_record_below_the_level_is_not_kept() {
    let relay = LogRelay::detached_uniform(log::LevelFilter::Warn);
    record(&relay, log::Level::Info, "kerosene_game", "chatter");
    record(&relay, log::Level::Error, "kerosene_game", "trouble");
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
        record(&relay, log::Level::Info, "kerosene_game", &format!("line {i}"));
    }
    let (lines, dropped) = relay.take();
    assert_eq!(lines.len(), MAX_PENDING);
    assert_eq!(dropped, 50);
    // The count resets once reported, so the next frame is not told again.
    assert_eq!(relay.take().1, 0);
}

#[test]
fn a_log_file_gets_every_line_including_the_consoles_own() {
    let dir = std::env::temp_dir().join(format!("kerosene-log-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kerosene.log");

    let relay = relay();
    relay.open_file(&path).expect("the log file opens");
    assert!(relay.has_file());
    record(&relay, log::Level::Info, "kerosene_game", "from the game");
    record(&relay, log::Level::Info, "kerosene_console", "from the console");
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
    let err = relay.open_file(std::path::Path::new("/definitely/not/here/kerosene.log"));
    assert!(err.is_err());
    assert!(!relay.has_file());
}

#[test]
fn the_environment_can_choose_the_level_but_nonsense_does_not() {
    // Only a bare level is understood; anything else keeps the default rather
    // than turning logging off by accident.
    unsafe { std::env::set_var("RUST_LOG", "warn") };
    assert_eq!(level_from_env(log::LevelFilter::Info), log::LevelFilter::Warn);
    unsafe { std::env::set_var("RUST_LOG", "kerosene_bsp=trace") };
    assert_eq!(level_from_env(log::LevelFilter::Info), log::LevelFilter::Info);
    unsafe { std::env::remove_var("RUST_LOG") };
    assert_eq!(level_from_env(log::LevelFilter::Info), log::LevelFilter::Info);
}

// ---- whose chatter belongs in the console ---------------------------------

#[test]
fn our_own_crates_are_recognised_by_their_log_targets() {
    // `log` targets are crate paths with underscores, and a module path is
    // separated with colons -- both have to count.
    for target in ["kerosene", "kerosene_engine", "kerosene_render::mesh", "chisel", "kiln", "cleave"] {
        assert!(is_ours(target), "{target} should be ours");
    }
}

#[test]
fn everything_else_is_not() {
    for target in ["wgpu_hal::vulkan::instance", "naga", "winit", "calloop", "kerobird"] {
        assert!(!is_ours(target), "{target} should not be ours");
    }
}

#[test]
fn a_foreign_crates_information_does_not_reach_the_console() {
    // A console opened to read one line and found full of Vulkan loader
    // chatter is a console nobody opens twice.
    let relay = LogRelay::detached(log::LevelFilter::Info);
    record(&relay, log::Level::Info, "wgpu_hal::vulkan", "Loader Message");
    record(&relay, log::Level::Info, "kerosene_engine::host", "loading maps/x");

    let (lines, _) = relay.take();
    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, vec!["loading maps/x"]);
}

#[test]
fn a_foreign_crates_warning_still_does() {
    // Held to warnings, not silenced: a warning from the graphics backend is
    // the one thing it says that anybody needs to act on.
    let relay = LogRelay::detached(log::LevelFilter::Info);
    record(&relay, log::Level::Warn, "wgpu_hal::vulkan", "device lost");

    let (lines, _) = relay.take();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "device lost");
}

#[test]
fn asking_for_less_holds_our_own_crates_to_it_too() {
    let relay = LogRelay::detached(log::LevelFilter::Error);
    record(&relay, log::Level::Warn, "kerosene_engine", "a warning");
    assert!(relay.take().0.is_empty(), "error-only means error-only");
}
