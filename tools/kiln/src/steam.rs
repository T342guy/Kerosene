// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! `kiln --ship dist --steam`: a distribution Steam can run and SteamPipe
//! can upload.
//!
//! Three things separate a Steam build from any other, and each is easy to
//! forget:
//!
//! * **Valve's redistributable.** A game built with the `steam` feature links
//!   `libsteam_api.so` / `steam_api64.dll` / `libsteam_api.dylib` and will
//!   not start without it. `cargo run` finds it in the build directory, which
//!   is why a game works on its developer's machine and not on a player's.
//!   Kiln builds with the feature, finds the library the `steamworks-sys`
//!   build script left in the build directory, and copies it beside the game.
//!
//! * **Finding it.** Windows looks beside the executable on its own. Linux
//!   and macOS do not, so the binary is built with an rpath of `$ORIGIN` /
//!   `@executable_path`, into a target directory of its own so the flags do
//!   not throw away the everyday build.
//!
//! * **Uploading.** SteamPipe reads an app build script and a depot build
//!   script. Kiln writes both, beside the distribution rather than in it,
//!   and prints the `steamcmd` line that uploads them. With `--steam-upload
//!   <account>` it runs that line, and `steamcmd` asks for the password and
//!   Steam Guard code itself. Kiln never sees a credential.
//!
//! On Windows every ship, Steam or not, links the C runtime statically, so a
//! player needs no Visual C++ redistributable installed either.

use anyhow::{Context, Result, bail};
use kerosene_vfs::toolchain::{BuildOptions, Profile};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What `--steam` asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SteamShip {
    /// Write `steam_appid.txt` beside the game, so it runs outside the Steam
    /// client. For testing only: Valve asks that it not be shipped, and the
    /// depot script excludes it.
    pub dev: bool,
    /// Upload with `steamcmd`, signed in as this account.
    pub upload_as: Option<String>,
}

/// The library `steamworks-sys` links, as it is named on this platform.
pub fn redist_name() -> &'static str {
    if cfg!(windows) {
        "steam_api64.dll"
    } else if cfg!(target_os = "macos") {
        "libsteam_api.dylib"
    } else {
        "libsteam_api.so"
    }
}

/// How to build a game that ships.
///
/// `steam` adds the feature and, off Windows, the rpath. Windows gets a
/// static C runtime regardless.
pub fn build_options(steam: bool, target_dir: &Path) -> BuildOptions {
    let mut flags: Vec<&str> = Vec::new();
    if cfg!(windows) {
        flags.push("-C target-feature=+crt-static");
    }
    if steam {
        if cfg!(target_os = "macos") {
            flags.push("-C link-arg=-Wl,-rpath,@executable_path");
        } else if !cfg!(windows) {
            flags.push("-C link-arg=-Wl,-rpath,$ORIGIN");
        }
    }
    BuildOptions {
        features: if steam {
            vec!["steam".to_string()]
        } else {
            Vec::new()
        },
        // Only a build with different flags needs a directory of its own.
        target_dir: (!flags.is_empty() || steam).then(|| target_dir.to_path_buf()),
        rustflags: (!flags.is_empty()).then(|| flags.join(" ")),
    }
}

/// The newest Steam redistributable under a target directory's build
/// scripts' output: `<target>/<profile>/build/steamworks-sys-*/out/`.
pub fn find_redist(target_dir: &Path, profile: Profile) -> Option<PathBuf> {
    let build = target_dir.join(profile.dir()).join("build");
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(build).into_iter().flatten().flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("steamworks-sys-")
        {
            continue;
        }
        let candidate = entry.path().join("out").join(redist_name());
        let Ok(when) = candidate.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| when > *t) {
            best = Some((when, candidate));
        }
    }
    best.map(|(_, p)| p)
}

/// The engine's own source tree, for building the stock runtime with Steam
/// when a project has no game package of its own: the nearest directory
/// above `from` holding `apps/kerosene/Cargo.toml`.
pub fn engine_workspace(from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .find(|d| d.join("apps/kerosene/Cargo.toml").is_file())
        .map(Path::to_path_buf)
}

/// The app and depot ids, from the project.
pub fn ids(project: Option<&kerosene_vfs::Project>) -> Result<(u32, u32)> {
    let Some(appid) = project.and_then(|p| p.steam_appid) else {
        bail!(
            "--steam needs the game's app id. Add \"steam_appid\" \"<id>\" to the .keroproj \
             (480 is Valve's test app, Spacewar)."
        );
    };
    let depot = project.and_then(|p| p.steam_depot).unwrap_or(appid + 1);
    Ok((appid, depot))
}

/// Paths of the build scripts written for SteamPipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildScripts {
    pub app: PathBuf,
    pub depot: PathBuf,
}

/// Write the app and depot build scripts for `dist` into `dir`.
pub fn write_build_scripts(
    dir: &Path,
    dist: &Path,
    appid: u32,
    depot: u32,
    description: &str,
) -> Result<BuildScripts> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let dist = absolute(dist);
    let output = absolute(&dir.join("output"));
    let scripts = BuildScripts {
        app: dir.join(format!("app_build_{appid}.vdf")),
        depot: dir.join(format!("depot_build_{depot}.vdf")),
    };
    let app = format!(
        "// Written by `kiln --ship --steam`. Upload with:\n\
         //   steamcmd +login <account> +run_app_build \"{app_path}\" +quit\n\
         \"AppBuild\"\n\
         {{\n\
         \t\"AppID\" \"{appid}\"\n\
         \t\"Desc\" \"{desc}\"\n\
         \t\"ContentRoot\" \"{dist}\"\n\
         \t\"BuildOutput\" \"{output}\"\n\
         \t\"Depots\"\n\
         \t{{\n\
         \t\t\"{depot}\" \"depot_build_{depot}.vdf\"\n\
         \t}}\n\
         }}\n",
        app_path = absolute(&scripts.app).display(),
        desc = description.replace('"', "'"),
        dist = vdf_path(&dist),
        output = vdf_path(&output),
    );
    let depot_script = format!(
        "// Written by `kiln --ship --steam`: everything in the distribution,\n\
         // except what must never reach a player.\n\
         \"DepotBuild\"\n\
         {{\n\
         \t\"DepotID\" \"{depot}\"\n\
         \t\"FileMapping\"\n\
         \t{{\n\
         \t\t\"LocalPath\" \"*\"\n\
         \t\t\"DepotPath\" \".\"\n\
         \t\t\"Recursive\" \"1\"\n\
         \t}}\n\
         \t\"FileExclusion\" \"steam_appid.txt\"\n\
         \t\"FileExclusion\" \"*.pdb\"\n\
         }}\n"
    );
    std::fs::write(&scripts.app, app)
        .with_context(|| format!("writing {}", scripts.app.display()))?;
    std::fs::write(&scripts.depot, depot_script)
        .with_context(|| format!("writing {}", scripts.depot.display()))?;
    Ok(scripts)
}

/// The `steamcmd` command line that uploads a build.
pub fn upload_command(account: &str, app_script: &Path) -> Vec<String> {
    vec![
        "+login".to_string(),
        account.to_string(),
        "+run_app_build".to_string(),
        absolute(app_script).display().to_string(),
        "+quit".to_string(),
    ]
}

/// Run `steamcmd`, handing it the terminal so it can ask for the password
/// and the Steam Guard code itself.
pub fn upload(account: &str, app_script: &Path) -> Result<()> {
    let args = upload_command(account, app_script);
    println!("  steamcmd {}", args.join(" "));
    let status = std::process::Command::new("steamcmd")
        .args(&args)
        .status()
        .context(
            "running steamcmd. Install it from Valve (it is not on PATH), or upload with \
             the command above from a machine that has it.",
        )?;
    if !status.success() {
        bail!("steamcmd failed ({status})");
    }
    Ok(())
}

/// VDF strings take forward slashes on every platform.
fn vdf_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}
