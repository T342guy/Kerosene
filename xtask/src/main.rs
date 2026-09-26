// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Repository chores, as `cargo xtask <task>`.
//!
//! ```text
//! cargo xtask bundle              # target/bundle/kerosene: the crate that is published
//! cargo xtask bundle --out <dir>
//! cargo xtask publish --dry-run   # check, bundle, and publish -- without the upload
//! cargo xtask publish             # check, bundle, and publish
//! ```

mod bundle;
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
        _ => {
            eprintln!(
                "usage: cargo xtask bundle [--out <dir>]\n       \
                 cargo xtask publish [--dry-run] [--yes]\n\n\
                 bundle   assemble the one `kerosene` crate crates.io gets, from the workspace\n\
                 publish  check the release, bundle it and publish it to crates.io"
            );
            std::process::exit(2)
        }
    }
}
