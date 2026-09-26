// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Repository chores, as `cargo xtask <task>`.
//!
//! ```text
//! cargo xtask bundle              # target/bundle/kerosene: the crate that is published
//! cargo xtask bundle --out <dir>
//! ```

mod bundle;
mod rewrite;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bundle") => bundle::run(&args[1..]),
        _ => {
            eprintln!(
                "usage: cargo xtask bundle [--out <dir>]\n\n\
                 bundle  assemble the one `kerosene` crate crates.io gets, from the workspace"
            );
            std::process::exit(2)
        }
    }
}
