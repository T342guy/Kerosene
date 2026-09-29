// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `kerosene-tools doctor`: is this machine ready to build and run a game?
//!
//! Every check is one a first build has tripped on: a Rust older than
//! Kerosene needs, no ALSA headers on Linux, no GPU the renderer can use,
//! no project where one was expected. Each says what it found and, when
//! something is wrong, what to do. Nothing is changed.

use anyhow::Result;
use kerosene_rhi::wgpu;
use std::process::Command;

/// What one check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    /// Fine, and what was seen.
    Ok(String),
    /// Works, but worth knowing.
    Note(String),
    /// Will stop a build or a run, and what to do about it.
    Problem(String),
}

/// The minimum Rust, as this toolset's own manifest says it.
pub const MSRV: &str = env!("CARGO_PKG_RUST_VERSION");

pub fn run(args: Vec<String>) -> Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "usage: kerosene-tools doctor\n\n\
             Check this machine for what building and running a Kerosene game needs:\n\
             the Rust toolchain, sound headers on Linux, a GPU, and the project."
        );
        return Ok(());
    }
    if let Some(other) = args.first() {
        anyhow::bail!("doctor takes no arguments; got {other:?}");
    }

    let checks: [(&str, Finding); 5] = [
        ("rust", rust()),
        ("sound", sound()),
        ("gpu", gpu()),
        ("project", project()),
        ("tools", tools()),
    ];
    let mut problems = 0;
    for (name, finding) in &checks {
        let (mark, text) = match finding {
            Finding::Ok(t) => ("ok", t),
            Finding::Note(t) => ("note", t),
            Finding::Problem(t) => {
                problems += 1;
                ("PROBLEM", t)
            }
        };
        println!("{name:<8} {mark:<8} {text}");
    }
    println!();
    match problems {
        0 => println!("Nothing in the way."),
        n => anyhow::bail!("{n} problem(s) above; each says what to do"),
    }
    Ok(())
}

/// A `major.minor[.patch]` version as numbers, for comparing.
fn version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().split(['.', '-', ' ']).map(|p| p.parse::<u32>());
    let major = parts.next()?.ok()?;
    let minor = parts.next()?.ok()?;
    let patch = parts.next().and_then(|p| p.ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// Whether `found` (from `rustc --version`) is at least `wanted`.
pub fn new_enough(found: &str, wanted: &str) -> Option<bool> {
    let found = found.split_whitespace().nth(1)?;
    Some(version(found)? >= version(wanted)?)
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn rust() -> Finding {
    let Some(rustc) = output("rustc", &["--version"]) else {
        return Finding::Problem("no `rustc` on PATH. Install Rust from https://rustup.rs".into());
    };
    if output("cargo", &["--version"]).is_none() {
        return Finding::Problem(
            "`rustc` is here but `cargo` is not. Reinstall with rustup".into(),
        );
    }
    match new_enough(&rustc, MSRV) {
        Some(true) => Finding::Ok(rustc),
        Some(false) => Finding::Problem(format!(
            "{rustc}, and Kerosene needs {MSRV} or newer. `rustup update stable`"
        )),
        None => Finding::Note(format!("{rustc}; could not read its version")),
    }
}

fn sound() -> Finding {
    if !cfg!(target_os = "linux") {
        return Finding::Ok("the system's own audio API; nothing to install".into());
    }
    match Command::new("pkg-config")
        .args(["--exists", "alsa"])
        .status()
    {
        Ok(status) if status.success() => Finding::Ok("ALSA headers found".into()),
        Ok(_) => Finding::Problem(
            "no ALSA headers, which the `audio` feature builds against. Install \
             libasound2-dev (Debian, Ubuntu) or alsa-lib-devel (Fedora), or build \
             with --no-default-features for a silent game"
                .into(),
        ),
        Err(_) => Finding::Note(
            "no pkg-config to ask about the ALSA headers; the `audio` feature needs \
             them (libasound2-dev or alsa-lib-devel)"
                .into(),
        ),
    }
}

fn gpu() -> Finding {
    // Asked quietly: a Vulkan loader narrating its search for layers is not
    // what someone running `doctor` came to read.
    let level = log::max_level();
    log::set_max_level(log::LevelFilter::Error);
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        flags: wgpu::InstanceFlags::empty(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }));
    drop(instance);
    log::set_max_level(level);
    match adapter {
        Ok(adapter) => {
            let info = adapter.get_info();
            let found = format!("{} ({:?})", info.name, info.backend);
            match info.device_type {
                wgpu::DeviceType::Cpu => Finding::Note(format!(
                    "{found}: a software renderer. The game runs, slowly; check the GPU driver"
                )),
                _ => Finding::Ok(found),
            }
        }
        Err(_) => Finding::Problem(
            "no GPU adapter. The editor and a windowed game need Vulkan, Metal or \
             DirectX 12; headless runs (`--headless`) do not"
                .into(),
        ),
    }
}

fn project() -> Finding {
    match kerosene_vfs::root::find(None, None) {
        Some(found) => match &found.project {
            Some(project) => Finding::Ok(format!("{} at {}", project.name, found.root.display())),
            None => Finding::Note(format!(
                "content at {}, with no .kproj; `kerosene-tools init` makes one",
                found.root.display()
            )),
        },
        None => Finding::Note("no project here. `kerosene-tools new <dir>` starts a game".into()),
    }
}

fn tools() -> Finding {
    let missing: Vec<&str> = kerosene_vfs::toolchain::available()
        .into_iter()
        .filter(|(_, found)| !found)
        .map(|(name, _)| name)
        .collect();
    match missing.as_slice() {
        [] => Finding::Ok("every stage is here".into()),
        names => Finding::Note(format!(
            "not found: {}. Only needed for what uses them",
            names.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustc_versions_are_compared_as_numbers() {
        assert_eq!(
            new_enough("rustc 1.98.1 (abc 2026-05-01)", "1.94"),
            Some(true)
        );
        assert_eq!(new_enough("rustc 1.94.0", "1.94"), Some(true));
        assert_eq!(new_enough("rustc 1.9.0", "1.94"), Some(false));
        assert_eq!(new_enough("rustc 1.100.0-nightly", "1.94"), Some(true));
        assert_eq!(new_enough("garbage", "1.94"), None);
    }
}
