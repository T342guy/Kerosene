// SPDX-License-Identifier: MPL-2.0
//! Kiln -- building a project's content.
//!
//! Everything a project ships has a source that is not what the engine loads:
//! `.png` becomes `.kerotex`, `.obj` becomes `.keromdl`, `.keromap` becomes a
//! `.kerobsp` with visibility and lighting baked into it, and the lot is
//! packed into a `.vault`. Running those in the right order over a whole tree
//! is a job, and it used to be a shell script in the repository.
//!
//! A shell script is not shipped. Install the tools, or copy them somewhere,
//! and the thing that knows how to *use* them stays behind in a git checkout
//! -- so the first thing anyone does with a fresh copy of the toolchain is
//! discover that the build step is missing. Hence a program: it installs
//! beside the tools it drives, it works anywhere they do, and it needs no
//! shell.
//!
//! The compilers stay separate programs and Kiln shells out to them, exactly
//! as Chisel does. That is the shape of the toolchain and it is not an
//! accident: you can still run any stage by hand, from a Makefile, or on a
//! build server. Only the texture build is a library call, because Chisel
//! makes the same one and the two must not be able to disagree.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use kerosene_vfs::project::Project;
use kerosene_vfs::toolchain;

pub mod ship;

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
    pub const ALL: [Stage; 5] =
        [Stage::Textures, Stage::Sounds, Stage::Models, Stage::Maps, Stage::Pack];

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
        Stage::EVERY.into_iter().find(|s| s.name() == name.trim().to_ascii_lowercase())
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
            models_in_metres: true,
            ship_to: None,
        }
    }
}

impl Settings {
    fn runs(&self, stage: Stage) -> bool { self.stages.contains(&stage) }

    /// Where the packed archive goes.
    ///
    /// Inside the content tree, which is where a shipped game keeps its
    /// archives and where the engine looks without being told. Named after
    /// the project, so two projects installed side by side do not collide.
    pub fn archive(&self) -> PathBuf {
        let stem = match &self.project {
            Some(p) => slug(&p.name),
            None => "content".to_string(),
        };
        self.content.join(format!("{stem}.vault"))
    }
}

/// Turn a project name into something safe to use as a filename.
pub(crate) fn slug(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    while out.contains("__") { out = out.replace("__", "_") }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() { "content".to_string() } else { trimmed.to_string() }
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
            let built = alchemy::build_textures(&settings.content)
                .context("building textures")?;
            println!("  {built}");
            report.textures = built.textures.compiled;
            report.textures_skipped = built.textures.skipped;
        }
    }

    if settings.runs(Stage::Sounds) {
        say("sounds");
        if settings.dry_run {
            println!("  would build {}", settings.content.join("sound").display());
        } else {
            let built = timbre::build_sounds(&settings.content, false)
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
        if report.models == 0 { println!("  no .obj sources under art/") }
    }

    if settings.runs(Stage::Maps) {
        say("maps");
        let maps = sources(&settings.content.join("maps"), "keromap");
        if maps.is_empty() { println!("  no .keromap sources under maps/") }
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

/// Compile every `.obj` under the art tree.
fn build_models(settings: &Settings) -> Result<usize> {
    let art = settings.content.join("art");
    let sources = sources(&art, "obj");
    let mut built = 0;

    for source in &sources {
        // `art/props/crate.obj` becomes `models/props/crate.keromdl`: the
        // path under `art` is the path under `models`, so a model's name is
        // decided by where its source is rather than by a list somebody has
        // to remember to update.
        let relative = source.strip_prefix(&art).unwrap_or(source).with_extension("keromdl");
        let out = settings.content.join("models").join(&relative);

        let mut args = vec![
            "compile".to_string(),
            source.display().to_string(),
            "-o".to_string(),
            out.display().to_string(),
        ];
        if settings.models_in_metres { args.push("--scale-metres".into()) }
        run("forge", &args, settings)?;
        built += 1;
    }
    Ok(built)
}

/// Take one map through the three compilers.
fn build_map(settings: &Settings, map: &Path, report: &mut Report) -> Result<()> {
    let name = map.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    println!("--- {name}");

    let compiled = map.with_extension("kerobsp");
    let mut args = vec![map.display().to_string()];
    if settings.ignore_leaks { args.push("--ignore-leaks".into()) }
    run("cleave", &args, settings)?;

    // A leak is reported rather than fatal: the map still compiles, it just
    // will not light or cull correctly, and finding out at the end of a build
    // of forty maps beats finding out on the first one.
    if !settings.dry_run && map.with_extension("keroleak").is_file() {
        report.leaking.push(name);
    }

    let mut args = vec![compiled.display().to_string()];
    if settings.fast { args.push("--fast".into()) }
    run("umbra", &args, settings)?;

    let mut args = vec![compiled.display().to_string()];
    if settings.fast { args.push("--fast".into()) }
    run("radiance", &args, settings)?;
    Ok(())
}

/// What goes into the archive.
///
/// Compiled formats and the loose data the engine reads directly. Sources --
/// `.png`, `.obj`, `.wav`, `.keromap` -- are deliberately left out: shipping
/// them doubles the download to deliver files the engine can read a smaller
/// version of.
const PACKED: &[&str] = &[
    "kerotex", "keromat", "keromdl", "kerobsp", "kerowalk", "keroscript", "kerosnd", "keroaud",
    "kerodef",
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
    run("vault", &args, settings)?;
    run("vault", &["verify".to_string(), archive.display().to_string()], settings)
}

/// Every file with an extension under a directory, in a stable order.
fn sources(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(dir, extension, &mut found);
    found.sort();
    found
}

fn collect(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, extension, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension)) {
            out.push(path);
        }
    }
}

/// Run one tool, or say that it would be run.
fn run(tool: &str, args: &[String], settings: &Settings) -> Result<()> {
    if settings.dry_run {
        println!("  would run: {tool} {}", args.join(" "));
        return Ok(());
    }

    let status = toolchain::command(tool)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| {
            format!(
                "running {tool}. It should be beside kiln or on PATH; \
                 `kiln --tools` lists what was found"
            )
        })?;

    if !status.success() {
        bail!("{tool} failed ({status})");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
