// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Repository chores, as `cargo xtask <task>`.
//!
//! ```text
//! cargo xtask bundle              # target/bundle/kerosene: the crate that is published
//! cargo xtask bundle --out <dir>
//! cargo xtask publish --dry-run   # check, bundle, and publish -- without the upload
//! cargo xtask publish             # check, bundle, and publish
//! cargo xtask layers              # every crate depends only on the layers below it
//! ```

mod bundle;
mod layers;
mod publish;
mod rewrite;

/// The repository: the directory above this crate.
pub fn repo_root() -> anyhow::Result<std::path::PathBuf> {
    use anyhow::Context;
    Ok(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask is inside the repository")?
        .to_path_buf())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bundle") => bundle::run(&args[1..]),
        Some("publish") => publish::run(&args[1..]),
        Some("layers") => layers::run(&args[1..]),
        // Asking is not a mistake: the usage, on stdout, and success.
        Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => {
            eprintln!("unknown task {other:?}\n\n{USAGE}");
            std::process::exit(2)
        }
        None => {
            eprintln!("{USAGE}");
            std::process::exit(2)
        }
    }
}

const USAGE: &str = "usage: cargo xtask bundle [--out <dir>]
       cargo xtask publish [--dry-run] [--yes]
       cargo xtask layers

bundle   assemble the one `kerosene` crate crates.io gets, from the workspace
publish  check the release, bundle it and publish it to crates.io
layers   check that every crate depends only on the layers below it";
