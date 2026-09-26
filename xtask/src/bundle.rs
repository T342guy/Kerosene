// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! `cargo xtask bundle`: the one `kerosene` crate that crates.io gets.
//!
//! Kerosene is developed as a workspace -- twenty engine crates, the tools,
//! the facade -- because separate crates build in parallel and keep the
//! engine and the tools honest about what depends on what. It is published
//! as one crate, because a game should add one line to its `Cargo.toml` and
//! crates.io should hold one thing. This is the step between.
//!
//! ```text
//! target/bundle/kerosene/
//!   Cargo.toml             one package; every dependency from crates.io
//!   LICENSE  LICENSE-EXCEPTION  NOTICE  README.md  CHANGELOG.md
//!   src/lib.rs             crates/kerosene/src/lib.rs, rewritten
//!   src/__k/mod.rs         every other crate, as a module
//!   src/__k/math/...       crates/kerosene-math, whole
//!   src/__k/chisel/...     tools/chisel, behind the `tools` feature
//!   src/bin/kerosene.rs    the stock runtime
//!   src/bin/kerosene-tools.rs   the toolset, behind `tools`
//! ```
//!
//! Each crate is copied whole -- its `src/` and anything its `include_bytes!`
//! reaches, like the engine's `base/` -- so relative includes still land.
//! Its `lib.rs` becomes `mod.rs`, and [`crate::rewrite`] turns `crate::` and
//! other crates' names into `crate::__k::<module>::`. The public face is
//! exactly the facade's: `kerosene::engine`, `kerosene::internals::bsp` and
//! the rest re-export from `__k`, which is `#[doc(hidden)]`.
//!
//! Tests, examples and benches stay in the workspace, where they run. The
//! bundle is checked by building it, and by building a game against it.

use crate::rewrite::Rewrite;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// The facade: the bundle's root.
const FACADE: &str = "kerosene";
/// The stock runtime: the bundle's `kerosene` binary.
const RUNTIME: &str = "kerosene-runtime";
/// This crate, which is not part of Kerosene.
const XTASK: &str = "kerosene-xtask";
/// The toolset package, whose `main.rs` is the `kerosene-tools` binary.
const TOOLSET: &str = "kerosene-tools";

/// Every Cargo feature any Kerosene crate declares. The bundle flattens them
/// into one set, so a new one has to be added here on purpose; see
/// [`features_table`].
const KNOWN_FEATURES: &[&str] = &["default", "audio", "device", "steam", "test-world", "tools"];

/// Files and directories of a crate that stay behind.
const LEFT_OUT: &[&str] = &["Cargo.toml", "tests", "examples", "benches", "target"];

pub fn run(args: &[String]) -> Result<()> {
    let repo = crate::repo_root()?;
    let mut out = repo.join("target/bundle/kerosene");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = PathBuf::from(args.get(i).context("--out needs a directory")?);
            }
            other => bail!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let summary = bundle(&repo, &out)?;
    println!("{summary}");
    Ok(())
}

/// A workspace package, as the bundle needs it.
struct Package {
    name: String,
    /// The library's name, as code names it: `kerosene_math`, `chisel`.
    lib: Option<String>,
    dir: PathBuf,
    dependencies: Vec<Value>,
    features: BTreeMap<String, Vec<String>>,
}

impl Package {
    /// The module it becomes: its library name without `kerosene_`.
    fn module(&self) -> String {
        let lib = self.lib.as_deref().unwrap_or(&self.name);
        lib.strip_prefix("kerosene_").unwrap_or(lib).to_string()
    }
}

/// Build the bundle into `out`, replacing whatever is there. Returns a
/// line saying what it made.
pub fn bundle(repo: &Path, out: &Path) -> Result<String> {
    let metadata = cargo_metadata(repo)?;
    let members: BTreeSet<String> = metadata["workspace_members"]
        .as_array()
        .context("workspace_members")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let mut packages: BTreeMap<String, Package> = BTreeMap::new();
    let mut facade_meta = None;
    for p in metadata["packages"].as_array().context("packages")? {
        let id = p["id"].as_str().unwrap_or_default();
        if !members.contains(id) {
            continue;
        }
        let name = p["name"].as_str().unwrap_or_default().to_string();
        if name == FACADE {
            facade_meta = Some(p.clone());
        }
        let lib = p["targets"].as_array().into_iter().flatten().find_map(|t| {
            let kinds = t["kind"].as_array()?;
            kinds
                .iter()
                .any(|k| k == "lib" || k == "rlib")
                .then(|| t["name"].as_str().unwrap_or_default().replace('-', "_"))
        });
        let dir = PathBuf::from(p["manifest_path"].as_str().unwrap_or_default())
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let features: BTreeMap<String, Vec<String>> = p["features"]
            .as_object()
            .map(|f| {
                f.iter()
                    .map(|(k, v)| {
                        let list = v
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|s| s.as_str().map(str::to_string))
                            .collect();
                        (k.clone(), list)
                    })
                    .collect()
            })
            .unwrap_or_default();
        for feature in features.keys() {
            if !KNOWN_FEATURES.contains(&feature.as_str()) {
                bail!(
                    "{name} declares the feature `{feature}`, which the bundle does not know. \
                     Add it to KNOWN_FEATURES and features_table in xtask/src/bundle.rs."
                );
            }
        }
        packages.insert(
            name.clone(),
            Package {
                name,
                lib,
                dir,
                dependencies: p["dependencies"].as_array().cloned().unwrap_or_default(),
                features,
            },
        );
    }
    let facade_meta = facade_meta.context("no `kerosene` package in the workspace")?;
    let internal: BTreeSet<String> = packages
        .keys()
        .filter(|n| ![FACADE, RUNTIME, XTASK].contains(&n.as_str()))
        .cloned()
        .collect();

    // What the engine needs without the tools: everything the facade and
    // the runtime reach through dependencies that are not optional.
    let mut core: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = [FACADE.to_string(), RUNTIME.to_string()].into();
    while let Some(name) = queue.pop_front() {
        let Some(p) = packages.get(&name) else {
            continue;
        };
        for d in &p.dependencies {
            let dep = d["name"].as_str().unwrap_or_default();
            if is_normal(d) && !is_optional(d) && internal.contains(dep) && core.insert(dep.into())
            {
                queue.push_back(dep.to_string());
            }
        }
    }
    let tools: BTreeSet<String> = internal.difference(&core).cloned().collect();

    // Every library's name, and the module path that replaces it.
    let mut crates: HashMap<String, String> = HashMap::new();
    for name in &internal {
        let p = &packages[name];
        let lib = p
            .lib
            .clone()
            .with_context(|| format!("{name} has no library"))?;
        crates.insert(lib, format!("crate::__k::{}", p.module()));
    }

    if out.exists() {
        std::fs::remove_dir_all(out).with_context(|| format!("clearing {}", out.display()))?;
    }
    std::fs::create_dir_all(out.join("src/__k"))?;

    // The crates, as modules.
    let mut modules = String::from(
        "// The engine's crates, as modules of the one published crate. Written by\n\
         // `cargo xtask bundle`; see xtask/src/bundle.rs. Not part of Kerosene's\n\
         // API: `kerosene::engine`, `kerosene::internals::bsp` and the rest are.\n\
         #![allow(missing_docs, rustdoc::all, clippy::all)]\n\n",
    );
    for name in &internal {
        let p = &packages[name];
        let module = p.module();
        let into = out.join("src/__k").join(&module);
        copy_crate(&p.dir, &into)?;
        let lib_rs = into.join("src/lib.rs");
        if !lib_rs.is_file() {
            bail!("{name}'s library is not src/lib.rs");
        }
        std::fs::rename(&lib_rs, into.join("src/mod.rs"))?;
        let own = format!("crate::__k::{module}");
        let rewrite = Rewrite {
            crates: &crates,
            own: Some(&own),
        };
        rewrite_tree(&into, &rewrite)?;
        if tools.contains(name) {
            modules.push_str("#[cfg(feature = \"tools\")]\n");
        }
        modules.push_str(&format!(
            "#[path = \"{module}/src/mod.rs\"]\npub mod {module};\n"
        ));
    }
    std::fs::write(out.join("src/__k/mod.rs"), modules)?;

    // The root: the facade, with the modules attached.
    let facade = &packages[FACADE];
    let root = std::fs::read_to_string(facade.dir.join("src/lib.rs"))?;
    let rewrite = Rewrite {
        crates: &crates,
        own: None,
    };
    let mut root = rewrite.apply(&root);
    root.push_str(
        "\n/// The engine's crates, folded into this one for publishing. Not API:\n\
         /// use the modules above.\n\
         #[doc(hidden)]\n\
         pub mod __k;\n",
    );
    std::fs::write(out.join("src/lib.rs"), root)?;

    // The binaries.
    std::fs::create_dir_all(out.join("src/bin"))?;
    let runtime = std::fs::read_to_string(packages[RUNTIME].dir.join("src/main.rs"))?;
    std::fs::write(out.join("src/bin/kerosene.rs"), runtime)?;
    let toolset = std::fs::read_to_string(packages[TOOLSET].dir.join("src/main.rs"))?;
    std::fs::write(
        out.join("src/bin/kerosene-tools.rs"),
        toolset.replace("kerosene_tools::", "kerosene::tools::"),
    )?;

    // The notices and the texts the licence identifier refers to.
    for file in [
        "LICENSE",
        "LICENSE-EXCEPTION",
        "NOTICE",
        "README.md",
        "CHANGELOG.md",
    ] {
        std::fs::copy(repo.join(file), out.join(file))
            .with_context(|| format!("copying {file}"))?;
    }
    // The workspace's resolution, so the bundle builds with the versions the
    // workspace was tested with.
    std::fs::copy(repo.join("Cargo.lock"), out.join("Cargo.lock"))?;

    let manifest = manifest(&facade_meta, &packages, &internal, &tools)?;
    std::fs::write(out.join("Cargo.toml"), manifest)?;

    Ok(format!(
        "bundled {} crates ({} of them tools) into {}",
        internal.len() + 1,
        tools.len(),
        out.display()
    ))
}

fn cargo_metadata(repo: &Path) -> Result<Value> {
    let output = std::process::Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(repo)
        .output()
        .context("running cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn is_normal(dep: &Value) -> bool {
    dep["kind"].is_null()
}

fn is_optional(dep: &Value) -> bool {
    dep["optional"].as_bool().unwrap_or(false)
}

/// Copy a crate's directory, less what stays behind, and less its binary.
fn copy_crate(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if LEFT_OUT.contains(&name.as_str()) || name.starts_with('.') {
            continue;
        }
        copy_tree(&entry.path(), &to.join(&name))?;
    }
    // The toolset's main.rs is a binary of the bundle, not a module.
    let main = to.join("src/main.rs");
    if main.is_file() {
        std::fs::remove_file(main)?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to).with_context(|| format!("copying {}", from.display()))?;
    }
    Ok(())
}

/// Rewrite every `.rs` file under `dir`, and point includes of the
/// repository's licence texts at the bundle's own copies.
fn rewrite_tree(dir: &Path, rewrite: &Rewrite) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rewrite_tree(&path, rewrite)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path)?;
            let text = rewrite
                .apply(&text)
                .replace("\"/../../LICENSE-EXCEPTION\"", "\"/LICENSE-EXCEPTION\"")
                .replace("\"/../../LICENSE\"", "\"/LICENSE\"");
            std::fs::write(&path, text)?;
        }
    }
    Ok(())
}

/// One external dependency, as every crate that uses it asks for it.
#[derive(Default)]
struct Dep {
    package: String,
    req: String,
    default_features: bool,
    features: BTreeSet<String>,
    /// Needed by the engine whatever features are on.
    required: bool,
}

/// The flattened feature set. Each crate's features share names, so the
/// bundle has one of each; what each turns on is the union of what it
/// turned on in every crate, with internal crates folded away.
fn features_table(
    packages: &BTreeMap<String, Package>,
    optional: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut table: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    table.insert("default".into(), ["audio".to_string()].into());
    table.insert("audio".into(), ["device".to_string()].into());
    for feature in ["device", "steam", "test-world", "tools"] {
        table.entry(feature.into()).or_default();
    }
    for p in packages.values() {
        for (feature, entries) in &p.features {
            if feature == "default" {
                continue;
            }
            for entry in entries {
                // `dep:x` of an external dependency carries over; one of an
                // internal crate is the crate being a module, which the
                // `tools` feature and the module list already say.
                if let Some(dep) = entry.strip_prefix("dep:")
                    && optional.contains_key(dep)
                {
                    table
                        .entry(feature.clone())
                        .or_default()
                        .insert(format!("dep:{dep}"));
                }
            }
        }
    }
    // What a feature of the bundle enables, for each optional dependency.
    for (dep, features) in optional {
        for feature in features {
            table
                .entry(feature.clone())
                .or_default()
                .insert(format!("dep:{dep}"));
        }
    }
    table
}

/// The bundle's `Cargo.toml`.
fn manifest(
    facade: &Value,
    packages: &BTreeMap<String, Package>,
    internal: &BTreeSet<String>,
    tools: &BTreeSet<String>,
) -> Result<String> {
    // Every external dependency, per target (`None` for all of them), keyed
    // by the name code uses.
    let mut deps: BTreeMap<Option<String>, BTreeMap<String, Dep>> = BTreeMap::new();
    // Optional dependencies, and the bundle features that turn each on.
    let mut optional: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let included = internal
        .iter()
        .chain([FACADE.to_string(), RUNTIME.to_string()].iter())
        .cloned()
        .collect::<Vec<_>>();
    for name in &included {
        let p = &packages[name];
        let is_tool = tools.contains(name);
        for d in &p.dependencies {
            let dep = d["name"].as_str().unwrap_or_default();
            if !is_normal(d) || internal.contains(dep) || dep == FACADE {
                continue;
            }
            if !d["source"]
                .as_str()
                .unwrap_or_default()
                .starts_with("registry+")
            {
                bail!("{name} depends on {dep} from outside crates.io, which crates.io refuses");
            }
            let key = d["rename"].as_str().unwrap_or(dep).to_string();
            let target = d["target"].as_str().map(str::to_string);
            let entry = deps
                .entry(target)
                .or_default()
                .entry(key.clone())
                .or_default();
            let req = d["req"].as_str().unwrap_or("*").to_string();
            if !entry.req.is_empty() && entry.req != req {
                bail!(
                    "{dep} is asked for as {} and as {req}; the workspace should agree",
                    entry.req
                );
            }
            entry.package = dep.to_string();
            entry.req = req;
            entry.default_features |= d["uses_default_features"].as_bool().unwrap_or(true);
            for f in d["features"].as_array().into_iter().flatten() {
                if let Some(f) = f.as_str() {
                    entry.features.insert(f.to_string());
                }
            }
            if is_optional(d) {
                // Turned on by whichever of this crate's features names it.
                let gates: BTreeSet<String> = p
                    .features
                    .iter()
                    .filter(|(_, entries)| entries.iter().any(|e| e == &format!("dep:{key}")))
                    .map(|(f, _)| f.clone())
                    .collect();
                if gates.is_empty() {
                    bail!("{name}'s optional {dep} is not turned on by any feature");
                }
                let mut gates = gates;
                if is_tool {
                    // And only ever with the tools there at all.
                    gates.retain(|g| g != "default");
                }
                optional.entry(key).or_default().extend(gates);
            } else if is_tool {
                optional.entry(key).or_default().insert("tools".into());
            } else {
                entry.required = true;
            }
        }
    }
    // A dependency the engine itself needs is never optional.
    for per_target in deps.values() {
        for (key, dep) in per_target {
            if dep.required {
                optional.remove(key);
            }
        }
    }
    let features = features_table(packages, &optional);

    let text = |key: &str| facade[key].as_str().unwrap_or_default().to_string();
    let list = |key: &str| {
        facade[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(quote))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = String::new();
    out.push_str(
        "# Written by `cargo xtask bundle` from the Kerosene workspace; edit the\n\
         # workspace, not this. See xtask/src/bundle.rs.\n\n",
    );
    out.push_str("[package]\n");
    out.push_str(&format!("name = {}\n", quote(FACADE)));
    out.push_str(&format!("version = {}\n", quote(&text("version"))));
    out.push_str(&format!("edition = {}\n", quote(&text("edition"))));
    out.push_str(&format!(
        "rust-version = {}\n",
        quote(&text("rust_version"))
    ));
    out.push_str(&format!("license = {}\n", quote(&text("license"))));
    out.push_str(&format!("description = {}\n", quote(&text("description"))));
    out.push_str(&format!("repository = {}\n", quote(&text("repository"))));
    out.push_str(&format!("homepage = {}\n", quote(&text("homepage"))));
    out.push_str("readme = \"README.md\"\n");
    out.push_str(&format!("authors = [{}]\n", list("authors")));
    out.push_str(&format!("keywords = [{}]\n", list("keywords")));
    out.push_str(&format!("categories = [{}]\n", list("categories")));
    out.push_str("\n[package.metadata.docs.rs]\nfeatures = [\"tools\"]\n");

    out.push_str("\n[features]\n");
    for (feature, enables) in &features {
        let enables: Vec<String> = enables.iter().map(|e| quote(e)).collect();
        out.push_str(&format!("{feature} = [{}]\n", enables.join(", ")));
    }

    out.push_str("\n[lib]\npath = \"src/lib.rs\"\n");
    out.push_str("\n[[bin]]\nname = \"kerosene\"\npath = \"src/bin/kerosene.rs\"\n");
    out.push_str(
        "\n[[bin]]\nname = \"kerosene-tools\"\npath = \"src/bin/kerosene-tools.rs\"\n\
         required-features = [\"tools\"]\n",
    );

    for (target, per_target) in &deps {
        match target {
            None => out.push_str("\n[dependencies]\n"),
            Some(t) => out.push_str(&format!("\n[target.'{t}'.dependencies]\n")),
        }
        for (key, dep) in per_target {
            let mut fields = Vec::new();
            if &dep.package != key {
                fields.push(format!("package = {}", quote(&dep.package)));
            }
            fields.push(format!("version = {}", quote(&dep.req)));
            if !dep.default_features {
                fields.push("default-features = false".into());
            }
            if !dep.features.is_empty() {
                let f: Vec<String> = dep.features.iter().map(|f| quote(f)).collect();
                fields.push(format!("features = [{}]", f.join(", ")));
            }
            if optional.contains_key(key) {
                fields.push("optional = true".into());
            }
            out.push_str(&format!("{key} = {{ {} }}\n", fields.join(", ")));
        }
    }
    // Not a member of the workspace it is written inside.
    out.push_str("\n[workspace]\n");
    Ok(out)
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
