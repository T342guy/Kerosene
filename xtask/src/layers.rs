// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `cargo xtask layers`: every crate depends only on the layers below it.
//!
//! The rules come from the refactor design document
//! (`src/refactor/`): a crate may depend on crates in lower layers, never on
//! one above it, and subsystems never depend on each other -- when two need
//! to share something, it moves down a layer, or the engine passes it
//! across. The compiler already refuses cycles; this refuses the edges that
//! would compile but that the architecture says must not exist.
//!
//! Only normal and build dependencies count. Tests and examples may reach
//! anywhere, because a test that goes through the whole stack is the point.
//!
//! A new crate is an error until it is given a layer below, so where it sits
//! is decided on purpose rather than by whatever it first happened to use.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::process::Command;

/// Where each crate sits. Lower numbers are lower layers.
///
/// | Layer | What |
/// |---|---|
/// | 0 | core: math, keyvalues, the console |
/// | 1 | core services: config, the file system, the store |
/// | 2 | data and hardware: compiled resources, the scene contract, the RHI |
/// | 3 | subsystems: render, physics, audio, entities, scripting, UI |
/// | 4 | the engine host, and the stock game |
/// | 5 | the tools |
/// | 6 | the published facade |
/// | 7 | the stock runtime binary |
const LAYERS: &[(&str, u8)] = &[
    ("kerosene-math", CORE),
    ("kerosene-kv", CORE),
    ("kerosene-console", CORE),
    ("kerosene-config", SERVICES),
    ("kerosene-vfs", SERVICES),
    ("kerosene-platform", SERVICES),
    ("kerosene-asset", DATA),
    ("kerosene-bsp", DATA),
    ("kerosene-walk", DATA),
    ("kerosene-map", DATA),
    ("kerosene-scene", DATA),
    ("kerosene-rhi", DATA),
    ("kerosene-render", SUBSYSTEM),
    ("kerosene-physics", SUBSYSTEM),
    ("kerosene-rigid", SUBSYSTEM),
    ("kerosene-anim", SUBSYSTEM),
    ("kerosene-audio", SUBSYSTEM),
    ("kerosene-entity", SUBSYSTEM),
    ("kerosene-script", SUBSYSTEM),
    ("kerosene-ui", SUBSYSTEM),
    ("kerosene-engine", HOST),
    ("kerosene-game", HOST),
    ("kerosene-toolui", TOOLS),
    ("kerosene-chisel", TOOLS),
    ("kerosene-cleave", TOOLS),
    ("kerosene-umbra", TOOLS),
    ("kerosene-resonance", TOOLS),
    ("kerosene-radiance", TOOLS),
    ("kerosene-alchemy", TOOLS),
    ("kerosene-timbre", TOOLS),
    ("kerosene-forge", TOOLS),
    ("kerosene-kiln", TOOLS),
    ("kerosene-vault", TOOLS),
    ("kerosene-loupe", TOOLS),
    ("kerosene-tools", TOOLS),
    ("kerosene", FACADE),
    ("kerosene-runtime", RUNTIME),
];

const CORE: u8 = 0;
const SERVICES: u8 = 1;
const DATA: u8 = 2;
const SUBSYSTEM: u8 = 3;
const HOST: u8 = 4;
const TOOLS: u8 = 5;
const FACADE: u8 = 6;
const RUNTIME: u8 = 7;

/// Layers whose crates may depend on each other. Everywhere else, a crate
/// depends only on strictly lower layers.
///
/// Core and the data layer are libraries of plain types, which build on one
/// another (a compiled map is keyvalues and vectors). The tools are separate
/// programs that share code by linking each other.
const OPEN_LAYERS: &[u8] = &[CORE, SERVICES, DATA, TOOLS];

/// Edges between subsystems that are allowed anyway, each with the reason.
/// Every entry here is a known exception to revisit, not a precedent.
const EXCEPTIONS: &[(&str, &str, &str)] = &[(
    "kerosene-ui",
    "kerosene-script",
    "UI documents run their bindings in the script subsystem's VM, which is \
     the one owner of the scripting language",
)];

/// Crates that exist for the tools. A runtime crate (anything below the
/// tools layer) must not link them: the runtime loads compiled data, and
/// never the source formats or the compilers that read them.
const TOOLS_ONLY: &[&str] = &["kerosene-map"];

pub fn run(args: &[String]) -> Result<()> {
    if let Some(extra) = args.first() {
        bail!("`layers` takes no arguments (got {extra:?})");
    }
    let edges = workspace_edges()?;
    let problems = check(&edges);
    if problems.is_empty() {
        println!(
            "{} crates, {} dependencies, every one pointing down",
            edges.len(),
            edges.values().map(Vec::len).sum::<usize>()
        );
        return Ok(());
    }
    for p in &problems {
        eprintln!("{p}");
    }
    bail!("{} layering problem(s)", problems.len())
}

/// Each workspace crate's normal and build dependencies on other workspace
/// crates.
fn workspace_edges() -> Result<BTreeMap<String, Vec<String>>> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(crate::repo_root()?)
        .output()
        .context("running cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let meta: Value = serde_json::from_slice(&out.stdout).context("reading cargo metadata")?;
    let packages = meta["packages"]
        .as_array()
        .context("cargo metadata has no packages")?;
    let names: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();

    let mut edges = BTreeMap::new();
    for p in packages {
        let name = p["name"].as_str().context("a package with no name")?;
        if name == "kerosene-xtask" {
            continue;
        }
        let mut deps: Vec<String> = p["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            // `kind` is null for normal dependencies, "build" or "dev" otherwise.
            .filter(|d| d["kind"].as_str() != Some("dev"))
            .filter_map(|d| d["name"].as_str())
            .filter(|d| names.contains(d))
            .map(str::to_string)
            .collect();
        deps.sort();
        deps.dedup();
        edges.insert(name.to_string(), deps);
    }
    Ok(edges)
}

/// Every edge that breaks a rule, as a sentence.
fn check(edges: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let layer = |name: &str| LAYERS.iter().find(|(n, _)| *n == name).map(|&(_, l)| l);
    let mut problems = Vec::new();

    for (from, deps) in edges {
        let Some(from_layer) = layer(from) else {
            problems.push(format!(
                "{from} has no layer: add it to LAYERS in xtask/src/layers.rs"
            ));
            continue;
        };
        for to in deps {
            let Some(to_layer) = layer(to) else {
                // Reported once, as its own crate.
                continue;
            };
            if TOOLS_ONLY.contains(&to.as_str()) && from_layer < TOOLS {
                problems.push(format!(
                    "{from} depends on {to}, which only the tools may link"
                ));
            }
            let allowed = to_layer < from_layer
                || (to_layer == from_layer && OPEN_LAYERS.contains(&from_layer))
                || EXCEPTIONS.iter().any(|&(f, t, _)| f == from && t == to);
            if !allowed {
                let how = if to_layer == from_layer {
                    "sideways, to another crate in"
                } else {
                    "upward, to"
                };
                problems.push(format!(
                    "{from} (layer {from_layer}) depends {how} layer {to_layer}: {to}"
                ));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(f, ts)| (f.to_string(), ts.iter().map(|t| t.to_string()).collect()))
            .collect()
    }

    #[test]
    fn downward_edges_pass() {
        let g = graph(&[
            ("kerosene-math", &[]),
            ("kerosene-bsp", &["kerosene-math"]),
            ("kerosene-render", &["kerosene-bsp", "kerosene-math"]),
        ]);
        assert!(check(&g).is_empty(), "{:?}", check(&g));
    }

    #[test]
    fn an_upward_edge_is_reported() {
        let g = graph(&[("kerosene-bsp", &["kerosene-render"])]);
        let p = check(&g);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("upward"), "{}", p[0]);
    }

    #[test]
    fn subsystems_may_not_depend_on_each_other() {
        let g = graph(&[("kerosene-render", &["kerosene-physics"])]);
        let p = check(&g);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("sideways"), "{}", p[0]);
    }

    #[test]
    fn a_listed_exception_is_allowed() {
        let g = graph(&[("kerosene-ui", &["kerosene-script"])]);
        assert!(check(&g).is_empty());
    }

    #[test]
    fn the_runtime_may_not_link_the_source_format() {
        let g = graph(&[("kerosene-entity", &["kerosene-map"])]);
        let p = check(&g);
        assert!(p.iter().any(|p| p.contains("only the tools")), "{p:?}");
    }

    #[test]
    fn an_unplaced_crate_is_an_error() {
        let g = graph(&[("kerosene-newthing", &[])]);
        assert!(check(&g)[0].contains("has no layer"));
    }

    #[test]
    fn every_layered_crate_is_in_the_workspace_once() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in LAYERS {
            assert!(seen.insert(name), "{name} is listed twice");
        }
    }
}
