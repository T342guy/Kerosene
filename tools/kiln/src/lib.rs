// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Kiln -- building a project's content.
//!
//! Everything a project ships has a source that is not what the engine loads:
//! `.png` becomes `.kerotex`, `.obj` becomes `.keromdl`, `.keromap` becomes a
//! `.kerobsp` with visibility and lighting baked into it, and the lot is
//! packed into a `.vault`. Running those in the right order over a whole tree
//! is a job, and it used to be a shell script in the repository.
//!
//! A shell script is not shipped. Install the toolset, or copy it somewhere,
//! and the thing that knows how to *use* it stays behind in a git checkout
//! -- so the first thing anyone does with a fresh copy of the toolchain is
//! discover that the build step is missing. Hence a program: it is part of
//! the one toolset executable, works anywhere that does, and needs no shell.
//!
//! The compilers are stages of the one toolset now, and Kiln drives them by
//! re-invoking the executable with the stage's name as a subcommand, exactly
//! as Chisel does. That is the shape of the toolchain and it is not an
//! accident: you can still run any stage by hand, from a Makefile, or on a
//! build server. Only the texture build is a library call, because Chisel
//! makes the same one and the two must not be able to disagree.

use anyhow::{Context, Result, bail};
use kerosene_vfs::project::Project;
use kerosene_vfs::toolchain;
use std::path::{Path, PathBuf};
use std::process::Stdio;

mod cli;
pub mod ship;

pub use cli::run;

/// Which stages to run.
///
/// All of them by default. The point of naming one is iteration: re-lighting
/// a map after changing a lamp should not recompile every texture in the
/// project first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Textures,
    Sounds,
    Models,
    Maps,
    Pack,
    /// Assemble a distribution. See [`ship`].
    Ship,
}

impl Stage {
    /// The stages that build content, run when nobody names any.
    ///
    /// `Ship` is deliberately not among them. Building content is what you do
    /// every few minutes; assembling something to hand out is not, and a
    /// stage that writes a directory of licence files on every compile would
    /// be a nuisance rather than a service.
    pub const ALL: [Stage; 5] = [
        Stage::Textures,
        Stage::Sounds,
        Stage::Models,
        Stage::Maps,
        Stage::Pack,
    ];

    /// Every stage that can be named on the command line.
    pub const EVERY: [Stage; 6] = [
        Stage::Textures,
        Stage::Sounds,
        Stage::Models,
        Stage::Maps,
        Stage::Pack,
        Stage::Ship,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stage::Textures => "textures",
            Stage::Sounds => "sounds",
            Stage::Models => "models",
            Stage::Maps => "maps",
            Stage::Pack => "pack",
            Stage::Ship => "ship",
        }
    }

    pub fn parse(name: &str) -> Option<Stage> {
        Stage::EVERY
            .into_iter()
            .find(|s| s.name() == name.trim().to_ascii_lowercase())
    }
}

/// How to build.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The content tree.
    pub content: PathBuf,
    /// The project that named it, when one did. Used for the archive's name.
    pub project: Option<Project>,
    /// Which stages to run.
    pub stages: Vec<Stage>,
    /// Skip the expensive visibility and lighting passes.
    pub fast: bool,
    /// Say what would run, and run nothing.
    pub dry_run: bool,
    /// Compile a map even if it leaks.
    pub ignore_leaks: bool,
    /// Rebuild sounds whose compiled form is already newer than the source.
    /// For when the compiler changed and the sources did not.
    pub force: bool,
    /// Treat `.obj` source as metres rather than kerosene units.
    ///
    /// True by default because modelling packages work in metres and a model
    /// a hundred times too small is the single most common thing to get wrong
    /// on the way in.
    pub models_in_metres: bool,
    /// Where to assemble a distribution, when one was asked for.
    pub ship_to: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            content: PathBuf::from("content"),
            project: None,
            stages: Stage::ALL.to_vec(),
            fast: false,
            dry_run: false,
            ignore_leaks: false,
            force: false,
            models_in_metres: true,
            ship_to: None,
        }
    }
}

impl Settings {
    fn runs(&self, stage: Stage) -> bool {
        self.stages.contains(&stage)
    }

    /// Where the packed archive goes.
    ///
    /// Inside the content tree, which is where a shipped game keeps its
    /// archives and where the engine looks without being told. Named after
    /// the project, so two projects installed side by side do not collide.
    pub fn archive(&self) -> PathBuf {
        archive_path(&self.content, self.project.as_ref())
    }
}

/// Where a project's content archive lives: `<content>/<slug>.vault`, or
/// `content.vault` when nothing names the project.
///
/// One function so the build panel and the archive panel agree on it -- when
/// each guessed on its own, one built `my_game.vault` and the other looked
/// for `content.vault`.
pub fn archive_path(content: &Path, project: Option<&kerosene_vfs::Project>) -> PathBuf {
    let stem = match project {
        Some(p) => slug(&p.name),
        None => "content".to_string(),
    };
    content.join(format!("{stem}.vault"))
}

/// Turn a project name into something safe to use as a filename: lower-case
/// ASCII, `_` for anything else.
///
/// Shared with `init`, so the project file and the archive it builds carry
/// the same spelling of the same name.
pub fn slug(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_")
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "content".to_string()
    } else {
        trimmed.to_string()
    }
}

/// What a build did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub textures: usize,
    pub textures_skipped: usize,
    pub sounds: usize,
    pub sounds_skipped: usize,
    pub models: usize,
    pub maps: usize,
    /// Maps that compiled but do not seal the world.
    pub leaking: Vec<String>,
    pub packed: Option<PathBuf>,
    pub shipped: Option<ship::Shipped>,
}

/// Build a project's content.
pub fn build(settings: &Settings) -> Result<Report> {
    if !settings.content.is_dir() {
        bail!("{} is not a directory", settings.content.display());
    }
    let mut report = Report::default();

    if settings.runs(Stage::Textures) {
        say("textures");
        if settings.dry_run {
            println!("  would build {}", settings.content.join("art").display());
        } else {
            let built = alchemy::build_textures(&settings.content).context("building textures")?;
            println!("  {built}");
            // Loose images and folder sets both: the summary line used to
            // count only the first, and a project built entirely from sets
            // reported "0 textures".
            report.textures = built.textures.compiled + built.sets.compiled;
            report.textures_skipped = built.textures.skipped + built.sets.skipped;
        }
    }

    if settings.runs(Stage::Sounds) {
        say("sounds");
        if settings.dry_run {
            println!("  would build {}", settings.content.join("sound").display());
        } else {
            let built = timbre::build_sounds(&settings.content, settings.force)
                .context("building sounds")?;
            for done in &built.compiled {
                for warning in &done.warnings {
                    println!("  {}: {warning}", done.output.display());
                }
            }
            // Every failure, not the first: finding out about the second
            // broken sound on the next build is how a fix takes three runs.
            for (_, error) in &built.failed {
                eprintln!("  error: {error}");
            }
            if !built.failed.is_empty() {
                bail!("{} sound(s) failed to compile", built.failed.len());
            }
            println!("  {built}");
            report.sounds = built.compiled.len();
            report.sounds_skipped = built.skipped;
        }
    }

    if settings.runs(Stage::Models) {
        say("models");
        report.models = build_models(settings)?;
        if report.models == 0 {
            println!("  no .obj, .gltf or .glb sources under art/")
        }
    }

    if settings.runs(Stage::Maps) {
        say("maps");
        let maps = sources(&settings.content.join("maps"), "keromap");
        if maps.is_empty() {
            println!("  no .keromap sources under maps/")
        }
        for map in &maps {
            build_map(settings, map, &mut report)?;
        }
        report.maps = maps.len();
    }

    if settings.runs(Stage::Pack) {
        say("pack");
        let archive = settings.archive();
        pack(settings, &archive)?;
        report.packed = Some(archive);
    }

    if settings.runs(Stage::Ship) {
        let Some(out) = settings.ship_to.clone() else {
            bail!("the ship stage needs somewhere to put the result: pass --ship <dir>");
        };
        say("ship");
        report.shipped = Some(ship::ship(settings, &out)?);
    }

    Ok(report)
}

fn say(stage: &str) {
    println!("==> {stage}");
}

/// Compile every `.obj`, `.gltf` and `.glb` under the art tree.
fn build_models(settings: &Settings) -> Result<usize> {
    let art = settings.content.join("art");
    let mut sources = sources(&art, "obj");
    sources.extend(self::sources(&art, "gltf"));
    sources.extend(self::sources(&art, "glb"));
    sources.sort();
    let mut built = 0;

    for source in &sources {
        // `art/props/crate.obj` becomes `models/props/crate.keromdl`: the
        // path under `art` is the path under `models`, so a model's name is
        // decided by where its source is rather than by a list somebody has
        // to remember to update.
        let relative = source
            .strip_prefix(&art)
            .unwrap_or(source)
            .with_extension("keromdl");
        let out = settings.content.join("models").join(&relative);

        let mut args = vec![
            "compile".to_string(),
            source.display().to_string(),
            "-o".to_string(),
            out.display().to_string(),
        ];
        // glTF is metres by definition; only OBJ needs telling.
        let is_obj = source
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("obj"));
        if settings.models_in_metres && is_obj {
            args.push("--scale-metres".into())
        }
        run_tool("forge", &args, settings)?;
        built += 1;
    }
    Ok(built)
}

/// Take one map through the four compilers.
fn build_map(settings: &Settings, map: &Path, report: &mut Report) -> Result<()> {
    let name = map
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    println!("--- {name}");

    let compiled = map.with_extension("kerobsp");
    let mut args = vec![map.display().to_string()];
    if settings.ignore_leaks {
        args.push("--ignore-leaks".into())
    }
    let sealed = run_tool("cleave", &args, settings);

    // A leak is reported rather than fatal to the *build*: Cleave writes the
    // trace and refuses the map, and finding out at the end of a build of
    // forty maps beats finding out on the first one. The other stages are
    // skipped -- there is no BSP to light or cull -- unless the leak was
    // waved through with --ignore-leaks, in which case there is.
    if !settings.dry_run && map.with_extension("keroleak").is_file() {
        report.leaking.push(name.clone());
    }
    if let Err(e) = sealed {
        if report.leaking.last() == Some(&name) {
            println!("  {name} leaks; skipping vis, acoustics and lighting");
            return Ok(());
        }
        return Err(e);
    }

    let mut args = vec![compiled.display().to_string()];
    if settings.fast {
        args.push("--fast".into())
    }
    run_tool("umbra", &args, settings)?;

    // Acoustics read the materials, so they are told where the content is
    // rather than left to find it.
    let mut args = vec![
        compiled.display().to_string(),
        "--content".into(),
        settings.content.display().to_string(),
    ];
    if settings.fast {
        args.push("--fast".into())
    }
    run_tool("resonance", &args, settings)?;

    let mut args = vec![compiled.display().to_string()];
    if settings.fast {
        args.push("--fast".into())
    }
    run_tool("radiance", &args, settings)?;
    Ok(())
}

/// What goes into the archive.
///
/// Compiled formats and the loose data the engine reads directly. Sources --
/// `.png`, `.obj`, `.wav`, `.keromap` -- are deliberately left out: shipping
/// them doubles the download to deliver files the engine can read a smaller
/// version of.
pub const PACKED: &[&str] = &[
    "kerotex",
    "keromat",
    "keromdl",
    "kerobsp",
    "kerowalk",
    "keroscript",
    "kerosnd",
    "keroaud",
    "kerodef",
    // The game UI: layouts and stylesheets are read as they are written, and
    // fonts a stylesheet names with `@font-face` are loaded as they are.
    "keroui",
    "kerocss",
    "ttf",
    "otf",
];

fn pack(settings: &Settings, archive: &Path) -> Result<()> {
    let mut args = vec![
        "pack".to_string(),
        settings.content.display().to_string(),
        "-o".to_string(),
        archive.display().to_string(),
    ];
    for extension in PACKED {
        args.push("--ext".into());
        args.push((*extension).to_string());
    }
    run_tool("vault", &args, settings)?;
    run_tool(
        "vault",
        &["verify".to_string(), archive.display().to_string()],
        settings,
    )
}

/// Every file with an extension under a directory, in a stable order.
fn sources(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(dir, extension, &mut found);
    found.sort();
    found
}

fn collect(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    // Missing means "none of these"; unreadable is said aloud rather than
    // reported as none.
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            println!("  warning: could not read {}: {e}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, extension, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(extension))
        {
            out.push(path);
        }
    }
}

/// Run one tool, or say that it would be run.
///
/// The tool is a subcommand of this same executable, so running it means
/// re-invoking ourselves with the subcommand first. That keeps the old
/// property: a compiler crash cannot take the build driver down with it.
fn run_tool(tool: &str, args: &[String], settings: &Settings) -> Result<()> {
    if settings.dry_run {
        println!("  would run: {tool} {}", args.join(" "));
        return Ok(());
    }

    let status = toolchain::command(tool)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("running the {tool} stage"))?;

    if !status.success() {
        bail!("{tool} failed ({status})");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
