// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
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

    fn dist(&self) -> PathBuf {
        self.root.join("dist")
    }
}

// ---- what lands in the distribution -----------------------------------

#[test]
fn the_game_is_named_after_the_project() {
    let f = Fixture::new("named");
    let shipped = f.ship_with_binary().unwrap();

    let exe = if cfg!(windows) {
        "test_game.exe"
    } else {
        "test_game"
    };
    assert_eq!(shipped.binary, f.dist().join(exe));
    assert!(
        shipped.binary.is_file(),
        "the binary must actually be copied"
    );
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

    assert_eq!(
        project.content,
        f.dist().join("content"),
        "content must be relative to the game"
    );
    assert_eq!(
        project.start_map.as_deref(),
        Some("tg_intro"),
        "the start map has to survive"
    );
    assert_eq!(
        project.game, None,
        "a player's copy is not built from source, so naming a cargo package would be a lie"
    );
}

#[test]
fn the_licence_texts_are_written_in_full() {
    let f = Fixture::new("licences");
    f.ship_with_binary().unwrap();

    let gpl = std::fs::read_to_string(f.dist().join("LICENSE")).unwrap();
    let exception = std::fs::read_to_string(f.dist().join("LICENSE-EXCEPTION")).unwrap();

    assert!(
        gpl.contains("GNU GENERAL PUBLIC LICENSE"),
        "the GPL, not a summary"
    );
    assert!(
        gpl.contains("Version 3, 29 June 2007"),
        "the GPL-3.0 text must be complete enough to act on"
    );
    assert!(
        exception.contains("KEROSENE EXCEPTION"),
        "the exception, not a summary"
    );
    assert!(
        exception.contains("Additional permission: linking")
            && exception.contains("Attribution Screen"),
        "both halves of the exception: the permission and the requirements"
    );
    // The old dual-licence file names are gone, and so are the GPL boilerplate ones.
    for stale in [
        "LICENSE-LGPL-3.0",
        "LICENSE-MPL-2.0",
        "COPYING",
        "COPYING.LESSER",
    ] {
        assert!(
            !f.dist().join(stale).exists(),
            "{stale} should not be written"
        );
    }
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
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let stem = name.trim_end_matches(".exe");
        assert!(
            ![
                "chisel",
                "cleave",
                "umbra",
                "resonance",
                "radiance",
                "alchemy",
                "timbre",
                "forge",
                "vault",
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
    let settings = Settings {
        dry_run: true,
        ..f.settings.clone()
    };
    let binary = f.root.join("bin/kerosene");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, b"ELF").unwrap();

    let shipped = ship_from(&settings, &f.dist(), &binary).unwrap();

    assert!(
        !f.dist().exists(),
        "a dry run must not create the directory it describes"
    );
    assert_eq!(
        shipped.binary,
        f.dist().join(if cfg!(windows) {
            "test_game.exe"
        } else {
            "test_game"
        }),
        "but it still reports what it would have written"
    );
}

// ---- refusing to ship the wrong thing ----------------------------------

#[test]
fn shipping_without_an_archive_says_to_build_first() {
    let f = Fixture::new("no-archive");
    std::fs::remove_file(f.settings.archive()).unwrap();

    let err = f.ship_with_binary().unwrap_err().to_string();
    assert!(err.contains("does not exist"), "{err}");
    assert!(
        err.contains("Run kiln"),
        "the error has to say what to do: {err}"
    );
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
    assert!(
        err.contains("a.kerobsp"),
        "the stale file must be named: {err}"
    );
}

// ---- the licence notice is unconditional -------------------------------

#[test]
fn the_notice_names_the_engine_and_disclaims_warranty() {
    // The exception asks that a program carrying Kerosene preserve the
    // notice, point at the source and show an attribution screen; the README
    // states the licence and those conditions so that a redistributor can
    // read them without opening the full text.
    let f = Fixture::new("notice");
    f.ship_with_binary().unwrap();

    let readme = std::fs::read_to_string(f.dist().join("README.txt")).unwrap();
    assert!(readme.contains("Built with Kerosene"));
    assert!(
        readme.contains("GNU General Public License") && readme.contains("Kerosene Exception"),
        "the licence and the exception must both be named: {readme}"
    );
    assert!(
        readme.contains("attribution screen"),
        "the attribution screen is a condition of shipping: {readme}"
    );
    assert!(
        readme.contains("modified from Kerosene"),
        "a modified engine must say so: {readme}"
    );
    assert!(readme.contains("NO WARRANTY"));
    assert!(
        readme.contains("pull request"),
        "the prefer-a-PR guidance: {readme}"
    );
    assert!(
        readme.contains("Test Game"),
        "the game's own name belongs at the top: {readme}"
    );
    assert!(
        readme.contains("https://github.com/t342guy/kerosene"),
        "Kerosene's own source is the pointer that never rots: {readme}"
    );
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
    assert!(
        readme.contains("source is available"),
        "the licence asks where: {readme}"
    );
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out)
        } else {
            out.push(path)
        }
    }
}

// ---- --steam ------------------------------------------------------------

impl Fixture {
    /// The fixture as a Steam game with app id 480.
    fn steam(name: &str, dev: bool) -> Fixture {
        let mut f = Fixture::new(name);
        let project = f.settings.project.as_mut().unwrap();
        project.steam_appid = Some(480);
        f.settings.steam = Some(crate::steam::SteamShip {
            dev,
            upload_as: None,
        });
        // Re-stamp the archive: the fixture's own writes came after it.
        std::fs::write(f.settings.archive(), b"vault").unwrap();
        f
    }

    /// A binary and a stand-in for Valve's library, where a build leaves it.
    fn steam_build(&self) -> (PathBuf, PathBuf) {
        let target = self.root.join("target/ship");
        let out = target.join("release/build/steamworks-sys-0123abcd/out");
        std::fs::create_dir_all(&out).unwrap();
        let redist = out.join(crate::steam::redist_name());
        std::fs::write(&redist, b"VALVE").unwrap();
        let binary = target.join("release/kerosene");
        std::fs::write(&binary, b"ELF").unwrap();
        (binary, target)
    }
}

#[test]
fn a_steam_ship_installs_valves_library_beside_the_game() {
    let f = Fixture::steam("steam-redist", false);
    let (binary, target) = f.steam_build();
    let redist = crate::steam::find_redist(&target, toolchain::Profile::Release)
        .expect("the build's copy is found");
    let shipped = ship_built(&f.settings, &f.dist(), &binary, Some(&redist)).unwrap();

    let installed = f.dist().join(crate::steam::redist_name());
    assert_eq!(shipped.steam_redist.as_deref(), Some(installed.as_path()));
    assert_eq!(std::fs::read(installed).unwrap(), b"VALVE");
    assert!(
        !f.dist().join("steam_appid.txt").exists(),
        "Valve asks that steam_appid.txt never ship"
    );
}

#[test]
fn a_dev_steam_ship_can_run_outside_the_client() {
    let f = Fixture::steam("steam-dev", true);
    let (binary, target) = f.steam_build();
    let redist = crate::steam::find_redist(&target, toolchain::Profile::Release).unwrap();
    ship_built(&f.settings, &f.dist(), &binary, Some(&redist)).unwrap();
    assert_eq!(
        std::fs::read_to_string(f.dist().join("steam_appid.txt")).unwrap(),
        "480\n"
    );
}

#[test]
fn steampipe_scripts_are_written_beside_the_distribution() {
    let f = Fixture::steam("steam-vdf", false);
    let (binary, target) = f.steam_build();
    let redist = crate::steam::find_redist(&target, toolchain::Profile::Release).unwrap();
    let shipped = ship_built(&f.settings, &f.dist(), &binary, Some(&redist)).unwrap();

    let scripts = shipped.steam_scripts.expect("scripts are written");
    assert_eq!(scripts.app, f.root.join("steam_build/app_build_480.vdf"));
    let app = std::fs::read_to_string(&scripts.app).unwrap();
    assert!(app.contains("\"AppID\" \"480\""), "{app}");
    assert!(app.contains("\"481\" \"depot_build_481.vdf\""), "{app}");
    assert!(
        app.contains(&format!(
            "\"ContentRoot\" \"{}\"",
            f.dist().display().to_string().replace('\\', "/")
        )),
        "{app}"
    );
    let depot = std::fs::read_to_string(&scripts.depot).unwrap();
    assert!(depot.contains("\"DepotID\" \"481\""), "{depot}");
    assert!(
        depot.contains("\"FileExclusion\" \"steam_appid.txt\""),
        "{depot}"
    );
    assert!(
        !f.dist().join("steam_build").exists(),
        "the scripts are not part of what players download"
    );
}

#[test]
fn the_readme_names_valves_library_only_when_it_ships() {
    let f = Fixture::steam("steam-readme", false);
    let (binary, target) = f.steam_build();
    let redist = crate::steam::find_redist(&target, toolchain::Profile::Release).unwrap();
    ship_built(&f.settings, &f.dist(), &binary, Some(&redist)).unwrap();
    let readme = std::fs::read_to_string(f.dist().join("README.txt")).unwrap();
    assert!(readme.contains("Steamworks SDK Access"), "{readme}");
    assert!(!readme.contains("contains no Valve"), "{readme}");

    let plain = Fixture::new("plain-readme");
    plain.ship_with_binary().unwrap();
    let readme = std::fs::read_to_string(plain.dist().join("README.txt")).unwrap();
    assert!(!readme.contains("Steamworks"), "{readme}");
    assert!(readme.contains("contains no Valve"), "{readme}");
}

#[test]
fn a_plain_ship_carries_nothing_of_steams() {
    let f = Fixture::new("no-steam");
    let shipped = f.ship_with_binary().unwrap();
    assert_eq!(shipped.steam_redist, None);
    for file in std::fs::read_dir(f.dist()).unwrap().flatten() {
        let name = file.file_name().to_string_lossy().to_lowercase();
        assert!(!name.contains("steam"), "{name} in a non-Steam ship");
    }
    assert!(!f.root.join("steam_build").exists());
}

#[test]
fn a_steam_ship_without_the_library_or_an_app_id_is_refused() {
    let f = Fixture::steam("steam-missing", false);
    let (binary, _) = f.steam_build();
    let e = ship_built(&f.settings, &f.dist(), &binary, None).unwrap_err();
    assert!(e.to_string().contains(crate::steam::redist_name()), "{e}");
    assert!(!f.dist().exists(), "nothing half-assembled");

    let mut f = Fixture::steam("steam-no-appid", false);
    f.settings.project.as_mut().unwrap().steam_appid = None;
    let (binary, target) = f.steam_build();
    let redist = crate::steam::find_redist(&target, toolchain::Profile::Release).unwrap();
    let e = ship_built(&f.settings, &f.dist(), &binary, Some(&redist)).unwrap_err();
    assert!(e.to_string().contains("steam_appid"), "{e}");
}

#[test]
fn the_newest_library_wins_when_several_builds_left_one() {
    let root = scratch("steam-newest");
    let build = root.join("release/build");
    for (dir, body) in [("steamworks-sys-old", "old"), ("steamworks-sys-new", "new")] {
        let out = build.join(dir).join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join(crate::steam::redist_name()), body).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let found = crate::steam::find_redist(&root, toolchain::Profile::Release).unwrap();
    assert_eq!(std::fs::read_to_string(found).unwrap(), "new");
    assert!(
        crate::steam::find_redist(&root.join("nowhere"), toolchain::Profile::Release).is_none()
    );
}

#[test]
fn a_steam_build_asks_for_the_feature_and_a_way_to_find_the_library() {
    let options = crate::steam::build_options(true, Path::new("/t/ship"));
    assert_eq!(options.features, vec!["steam".to_string()]);
    assert_eq!(options.target_dir.as_deref(), Some(Path::new("/t/ship")));
    let flags = options.rustflags.unwrap_or_default();
    if cfg!(target_os = "linux") {
        assert!(flags.contains("rpath,$ORIGIN"), "{flags}");
    }
    if cfg!(windows) {
        assert!(flags.contains("crt-static"), "{flags}");
    }
    let plain = crate::steam::build_options(false, Path::new("/t/ship"));
    assert!(plain.features.is_empty());
    if !cfg!(windows) {
        assert_eq!(
            plain,
            toolchain::BuildOptions::default(),
            "an ordinary build is untouched"
        );
    }
}

#[test]
fn the_shipped_project_keeps_what_the_store_needs() {
    let mut f = Fixture::steam("steam-project", false);
    {
        let p = f.settings.project.as_mut().unwrap();
        p.achievements = vec![("ACH_A".into(), "The \"first\" one".into())];
        p.stats = vec![("kills".into(), "int".into())];
        p.dlc = vec![(111, "Soundtrack".into())];
    }
    let (binary, target) = f.steam_build();
    let redist = crate::steam::find_redist(&target, toolchain::Profile::Release).unwrap();
    ship_built(&f.settings, &f.dist(), &binary, Some(&redist)).unwrap();

    let path = f.dist().join("test_game.keroproj");
    let shipped = Project::read(&path).unwrap();
    assert_eq!(shipped.steam_appid, Some(480));
    assert_eq!(
        shipped.achievements,
        vec![("ACH_A".to_string(), "The \"first\" one".to_string())]
    );
    assert_eq!(
        shipped.stats,
        vec![("kills".to_string(), "int".to_string())]
    );
    assert_eq!(shipped.dlc, vec![(111, "Soundtrack".to_string())]);
    assert_eq!(shipped.start_map.as_deref(), Some("tg_intro"));
}
