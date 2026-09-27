// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Where a game keeps what belongs to the person playing it: saves,
//! settings, console history, screenshots.
//!
//! Not beside the executable. A game installed under Program Files, in a
//! read-only Steam library or in `/usr` cannot write there, and a machine
//! shared by two people should not share their saves. Each platform says
//! where such files go, and this is that answer:
//!
//! | Platform | Directory |
//! |---|---|
//! | Linux, BSD | `$XDG_DATA_HOME/<app_id>`, else `~/.local/share/<app_id>` |
//! | Windows | `%APPDATA%\<app_id>` |
//! | macOS | `~/Library/Application Support/<app_id>` |
//!
//! Worked out by hand from the environment -- three rules are not worth a
//! dependency.

use std::path::PathBuf;

/// The per-user directory for the game `app_id`, or `None` when the
/// environment does not say where home is.
pub fn user_data_dir(app_id: &str) -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        match env("XDG_DATA_HOME") {
            Some(dir) if dir.is_absolute() => dir,
            _ => env("HOME")?.join(".local/share"),
        }
    };
    Some(base.join(app_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directory_is_named_for_the_game() {
        if let Some(dir) = user_data_dir("my-game") {
            assert!(dir.ends_with("my-game"), "{}", dir.display());
            assert!(dir.is_absolute());
        }
    }
}
