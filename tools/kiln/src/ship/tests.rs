// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
use super::*;
use crate::Stage;
use kerosene_vfs::project::Project;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kiln-ship-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A project with content, an archive, and a `kerosene` binary standing in for the
/// engine runtime, arranged the way a real build leaves them.
struct Fixture {
    root: PathBuf,
    settings: Settings,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root = scratch(name);
        let content = root.join("content");
        std::fs::create_dir_all(content.join("maps")).unwrap();
        std::fs::write(content.join("maps/a.kerobsp"), b"map").unwrap();

        let path = root.join("game.keroproj");
        std::fs::write(
            &path,
            "project { \"name\" \"Test Game\" \"content\" \"content\" \"startmap\" \"tg_intro\" }",
        )
        .unwrap();
        let project = Project::read(&path).unwrap();

        let settings = Settings {
            content: content.clone(),
            project: Some(project),
            stages: vec![Stage::Ship],
            ..Settings::default()
        };
        // The archive is written last in a real build, so it is the newest
        // thing in the tree. Several tests turn on that.
        std::fs::write(settings.archive(), b"vault").unwrap();
        Fixture { root, settings }
    }

    /// Ship into `dist`, standing in a binary for the one cargo would build.
    fn ship_with_binary(&self) -> Result<Shipped> {
        let bin_dir = self.root.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let binary = bin_dir.join("kerosene");
        std::fs::write(&binary, b"ELF").unwrap();
        ship_from(&self.settings, &self.root.join("dist"), &binary)
    }

    fn dist(&self) -> PathBuf { self.root.join("dist") }
}

// ---- what lands in the distribution -----------------------------------

#[test]
fn the_game_is_named_after_the_project() {
    let f = Fixture::new("named");
    let shipped = f.ship_with_binary().unwrap();

    let exe = if cfg!(windows) { "test_game.exe" } else { "test_game" };
    assert_eq!(shipped.binary, f.dist().join(exe));
    assert!(shipped.binary.is_file(), "the binary must actually be copied");
}

#[test]
fn the_archive_lands_under_content_where_the_game_looks_for_it() {
    let f = Fixture::new("archive");
    let shipped = f.ship_with_binary().unwrap();

    assert_eq!(shipped.archive, f.dist().join("content/test_game.vault"));
    assert_eq!(std::fs::read(&shipped.archive).unwrap(), b"vault");
}

#[test]
fn a_project_file_is_written_pointing_at_the_shipped_content() {
    let f = Fixture::new("project");
    f.ship_with_binary().unwrap();

    let written = std::fs::read_to_string(f.dist().join("test_game.keroproj")).unwrap();
    let project = Project::parse(&written, &f.dist().join("test_game.keroproj")).unwrap();

    assert_eq!(project.content, f.dist().join("content"), "content must be relative to the game");
    assert_eq!(project.start_map.as_deref(), Some("tg_intro"), "the start map has to survive");
    assert_eq!(
        project.game, None,
        "a player's copy is not built from source, so naming a cargo package would be a lie"
    );
}

#[test]
fn the_licence_texts_are_written_in_full() {
    let f = Fixture::new("licences");
    f.ship_with_binary().unwrap();

    let lgpl = std::fs::read_to_string(f.dist().join("LICENSE-LGPL-3.0")).unwrap();
    let mpl = std::fs::read_to_string(f.dist().join("LICENSE-MPL-2.0")).unwrap();

    assert!(lgpl.contains("GNU LESSER GENERAL PUBLIC LICENSE"), "the LGPL, not a summary");
    assert!(lgpl.contains("Version 3"), "the LGPL-3.0 text must be complete enough to act on");
    assert!(mpl.contains("Mozilla Public License"), "the MPL, not a summary");
    assert!(mpl.contains("2.0"), "the MPL-2.0 text must be complete enough to act on");
    // Both full texts ship, and the old GPL-only boilerplate filenames do not.
    assert!(f.dist().join("LICENSE-LGPL-3.0").exists());
    assert!(f.dist().join("LICENSE-MPL-2.0").exists());
    assert!(!f.dist().join("COPYING").exists());
    assert!(!f.dist().join("COPYING.LESSER").exists());
}

// ---- what must never land in it ---------------------------------------

#[test]
fn no_tool_is_ever_shipped_with_a_game() {
    // Not a matter of tidiness: the tools are ordinary copyleft binaries, so
    // shipping one to a player obliges you to ship its source as well. The
    // distribution is assembled from a named list precisely so that this
    // cannot happen by accident, and this is the assertion that keeps it so.
    let f = Fixture::new("no-tools");
    f.ship_with_binary().unwrap();

    let mut found = Vec::new();
    walk(&f.dist(), &mut found);
    for path in &found {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        let stem = name.trim_end_matches(".exe");
        assert!(
            ![
                "chisel", "cleave", "umbra", "radiance", "alchemy", "timbre", "forge", "vault",
                "kiln"
            ]
            .contains(&stem),
            "a compiler reached the distribution: {}",
            path.display()
        );
    }
}

#[test]
fn nothing_is_written_on_a_dry_run() {
    let f = Fixture::new("dry");
    let settings = Settings { dry_run: true, ..f.settings.clone() };
    let binary = f.root.join("bin/kerosene");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, b"ELF").unwrap();

    let shipped = ship_from(&settings, &f.dist(), &binary).unwrap();

    assert!(!f.dist().exists(), "a dry run must not create the directory it describes");
    assert_eq!(shipped.binary, f.dist().join(if cfg!(windows) { "test_game.exe" } else { "test_game" }),
        "but it still reports what it would have written");
}

// ---- refusing to ship the wrong thing ----------------------------------

#[test]
fn shipping_without_an_archive_says_to_build_first() {
    let f = Fixture::new("no-archive");
    std::fs::remove_file(f.settings.archive()).unwrap();

    let err = f.ship_with_binary().unwrap_err().to_string();
    assert!(err.contains("does not exist"), "{err}");
    assert!(err.contains("Run kiln"), "the error has to say what to do: {err}");
}

#[test]
fn shipping_a_stale_archive_is_refused_and_names_what_changed() {
    // The failure this prevents is quiet and expensive: a map edited after the
    // last pack ships as whatever it was a week ago, and nothing anywhere
    // reports a problem.
    let f = Fixture::new("stale");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(f.settings.content.join("maps/a.kerobsp"), b"edited").unwrap();

    let err = f.ship_with_binary().unwrap_err().to_string();
    assert!(err.contains("older than"), "{err}");
    assert!(err.contains("a.kerobsp"), "the stale file must be named: {err}");
}

// ---- the licence notice is unconditional under both arms ------------

#[test]
fn the_notice_names_the_engine_and_disclaims_warranty() {
    // Both arms ask that a program carrying Kerosene preserve the notice and
    // point at the source; the README states the choice between them. There
    // is no relinking clause under MPL-2.0, because it copies only at file
    // level and never at the level of how the binary is linked.
    let f = Fixture::new("notice");
    f.ship_with_binary().unwrap();

    let readme = std::fs::read_to_string(f.dist().join("README.txt")).unwrap();
    assert!(readme.contains("Built with Kerosene"));
    assert!(readme.contains("LGPL-3.0-or-later"), "both arms must be stated: {readme}");
    assert!(readme.contains("Mozilla Public License"));
    assert!(readme.contains("NO WARRANTY"));
    assert!(readme.contains("pull request"), "the prefer-a-PR guidance: {readme}");
    assert!(readme.contains("Test Game"), "the game's own name belongs at the top: {readme}");
    // The fonts travel inside any binary linking egui, which includes the
    // engine's console overlay, and their notices have to travel with them.
    assert!(readme.contains("Open Font License"), "{readme}");
}

#[test]
fn the_notice_carries_the_mpl_crate_the_engine_links() {
    // `smartstring` reaches the engine through rhai, so a shipped game carries
    // MPL-2.0 code and owes its notice. Nobody would remember this; the point
    // of writing the file from code is that nobody has to.
    let f = Fixture::new("mpl");
    f.ship_with_binary().unwrap();

    let readme = std::fs::read_to_string(f.dist().join("README.txt")).unwrap();
    assert!(readme.contains("smartstring"), "{readme}");
    assert!(readme.contains("Mozilla Public License"), "{readme}");
    assert!(readme.contains("source is available"), "the licence asks where: {readme}");
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() { walk(&path, out) } else { out.push(path) }
    }
}
