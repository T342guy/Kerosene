// SPDX-License-Identifier: LGPL-3.0-or-later
use super::*;
use crate::Stage;
use void_vfs::project::Project;

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

/// A project with content, an archive, and a `void` binary standing in for the
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
        std::fs::write(content.join("maps/a.voidbsp"), b"map").unwrap();

        let path = root.join("game.voidproj");
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
    fn ship_with_binary(&self, library: Option<&str>) -> Result<Shipped> {
        let bin_dir = self.root.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let binary = bin_dir.join("void");
        std::fs::write(&binary, b"ELF").unwrap();
        if let Some(library) = library {
            std::fs::write(bin_dir.join(library), b"SO").unwrap();
        }
        ship_from(&self.settings, &self.root.join("dist"), &binary)
    }

    fn dist(&self) -> PathBuf { self.root.join("dist") }
}

// ---- what lands in the distribution -----------------------------------

#[test]
fn the_game_is_named_after_the_project() {
    let f = Fixture::new("named");
    let shipped = f.ship_with_binary(None).unwrap();

    let exe = if cfg!(windows) { "test_game.exe" } else { "test_game" };
    assert_eq!(shipped.binary, f.dist().join(exe));
    assert!(shipped.binary.is_file(), "the binary must actually be copied");
}

#[test]
fn the_archive_lands_under_content_where_the_game_looks_for_it() {
    let f = Fixture::new("archive");
    let shipped = f.ship_with_binary(None).unwrap();

    assert_eq!(shipped.archive, f.dist().join("content/test_game.vault"));
    assert_eq!(std::fs::read(&shipped.archive).unwrap(), b"vault");
}

#[test]
fn a_project_file_is_written_pointing_at_the_shipped_content() {
    let f = Fixture::new("project");
    f.ship_with_binary(None).unwrap();

    let written = std::fs::read_to_string(f.dist().join("test_game.voidproj")).unwrap();
    let project = Project::parse(&written, &f.dist().join("test_game.voidproj")).unwrap();

    assert_eq!(project.content, f.dist().join("content"), "content must be relative to the game");
    assert_eq!(project.start_map.as_deref(), Some("tg_intro"), "the start map has to survive");
    assert_eq!(
        project.game, None,
        "a player's copy is not built from source, so naming a cargo package would be a lie"
    );
}

#[test]
fn both_licence_texts_are_written_in_full() {
    let f = Fixture::new("licences");
    f.ship_with_binary(None).unwrap();

    let lgpl = std::fs::read_to_string(f.dist().join("COPYING.LESSER")).unwrap();
    let gpl = std::fs::read_to_string(f.dist().join("COPYING")).unwrap();

    assert!(lgpl.contains("GNU LESSER GENERAL PUBLIC LICENSE"), "the LGPL itself, not a summary");
    assert!(gpl.contains("GNU GENERAL PUBLIC LICENSE"), "the GPL the LGPL builds on");
    // Section 4 is the one this whole module exists to satisfy.
    assert!(lgpl.contains("Combined Works"), "the LGPL text must be complete enough to act on");
}

// ---- what must never land in it ---------------------------------------

#[test]
fn no_tool_is_ever_shipped_with_a_game() {
    // Not a matter of tidiness: the tools are ordinary copyleft binaries, so
    // shipping one to a player obliges you to ship its source as well. The
    // distribution is assembled from a named list precisely so that this
    // cannot happen by accident, and this is the assertion that keeps it so.
    let f = Fixture::new("no-tools");
    f.ship_with_binary(None).unwrap();

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
    let binary = f.root.join("bin/void");
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

    let err = f.ship_with_binary(None).unwrap_err().to_string();
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
    std::fs::write(f.settings.content.join("maps/a.voidbsp"), b"edited").unwrap();

    let err = f.ship_with_binary(None).unwrap_err().to_string();
    assert!(err.contains("older than"), "{err}");
    assert!(err.contains("a.voidbsp"), "the stale file must be named: {err}");
}

// ---- the licence notice tracks how the engine was linked ---------------

#[test]
fn a_shared_engine_is_shipped_and_reported_as_replaceable() {
    let library = if cfg!(windows) {
        "void_engine.dll"
    } else if cfg!(target_os = "macos") {
        "libvoid_engine.dylib"
    } else {
        "libvoid_engine.so"
    };
    let f = Fixture::new("dynamic");
    let shipped = f.ship_with_binary(Some(library)).unwrap();

    assert!(shipped.engine_is_replaceable());
    assert!(shipped.engine_library.as_ref().unwrap().is_file(), "the library must be copied");

    let readme = std::fs::read_to_string(f.dist().join("README.txt")).unwrap();
    assert!(readme.contains("replace that file"), "{readme}");
    assert!(readme.contains("1.94"), "relinking needs the compiler version: {readme}");
}

#[test]
fn a_static_engine_is_reported_as_not_replaceable_and_says_what_is_owed() {
    let f = Fixture::new("static");
    let shipped = f.ship_with_binary(None).unwrap();

    assert!(!shipped.engine_is_replaceable());
    assert_eq!(shipped.engine_library, None);

    let readme = std::fs::read_to_string(f.dist().join("README.txt")).unwrap();
    assert!(readme.contains("linked statically"), "{readme}");
    assert!(readme.contains("section 4"), "the recipient is owed a relink route: {readme}");
}

#[test]
fn the_notice_names_the_engine_and_disclaims_warranty() {
    // LGPL 4(a): prominent notice that the work uses the library and that the
    // library is covered by the licence.
    let f = Fixture::new("notice");
    f.ship_with_binary(None).unwrap();

    let readme = std::fs::read_to_string(f.dist().join("README.txt")).unwrap();
    assert!(readme.contains("Built with VoidEngine"));
    assert!(readme.contains("Lesser General Public License"));
    assert!(readme.contains("NO WARRANTY"));
    assert!(readme.contains("Test Game"), "the game's own name belongs at the top: {readme}");
    // The fonts travel inside any binary linking egui, which includes the
    // engine's console overlay, and their notices have to travel with them.
    assert!(readme.contains("Open Font License"), "{readme}");
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() { walk(&path, out) } else { out.push(path) }
    }
}
