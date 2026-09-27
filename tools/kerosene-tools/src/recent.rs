// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The projects this person opened lately, newest first.
//!
//! Kept in the user data directory as plain lines -- when, name, content
//! root, separated by tabs -- so the file is one a person can read, fix or
//! delete by hand, and one a crash halfway through writing cannot make the
//! toolset refuse to start over.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The file under the user data directory.
pub const FILE: &str = "recent-projects.txt";

/// How many are kept. Enough for everything one person works on at once.
pub const LIMIT: usize = 12;

/// One project opened before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentProject {
    /// The content root, which is what the toolset is opened on: the
    /// project file above it is found from there.
    pub content: PathBuf,
    pub name: String,
    /// When it was last opened, in seconds since the Unix epoch.
    pub opened: u64,
}

impl RecentProject {
    /// How long ago it was opened.
    pub fn age(&self) -> Duration {
        Duration::from_secs(now().saturating_sub(self.opened))
    }

    /// Whether it is still there to open.
    pub fn exists(&self) -> bool {
        self.content.is_dir()
    }
}

/// The list, and the file it is kept in.
#[derive(Clone, Debug, Default)]
pub struct Recent {
    pub entries: Vec<RecentProject>,
    /// `None` keeps the list in memory only: tests, and a machine with no
    /// user data directory.
    file: Option<PathBuf>,
}

impl Recent {
    /// Read the list from `file`. A missing or unreadable file is an empty
    /// list, never an error.
    pub fn load(file: PathBuf) -> Recent {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        Recent {
            entries: parse(&text),
            file: Some(file),
        }
    }

    /// The list for this person, from the user data directory.
    pub fn for_user() -> Recent {
        match kerosene_vfs::user_data_dir("kerosene") {
            Some(dir) => Recent::load(dir.join(FILE)),
            None => Recent::default(),
        }
    }

    /// Put a project at the top, and write the list.
    pub fn add(&mut self, content: &Path, name: &str) {
        self.entries.retain(|e| e.content != content);
        self.entries.insert(
            0,
            RecentProject {
                content: content.to_path_buf(),
                name: name.to_string(),
                opened: now(),
            },
        );
        self.entries.truncate(LIMIT);
        self.save();
    }

    /// Forget a project, and write the list.
    pub fn remove(&mut self, content: &Path) {
        self.entries.retain(|e| e.content != content);
        self.save();
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = kerosene_vfs::write_atomic(file, format(&self.entries).as_bytes()) {
            log::warn!(
                "could not remember recent projects in {}: {e}",
                file.display()
            );
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse(text: &str) -> Vec<RecentProject> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let opened = parts.next()?.trim().parse().ok()?;
            let name = parts.next()?.to_string();
            let content = PathBuf::from(parts.next()?);
            Some(RecentProject {
                content,
                name,
                opened,
            })
        })
        .take(LIMIT)
        .collect()
}

fn format(entries: &[RecentProject]) -> String {
    entries
        .iter()
        .map(|e| {
            // A tab or a newline in a name would break the line apart.
            let name = e.name.replace(['\t', '\n', '\r'], " ");
            format!("{}\t{}\t{}\n", e.opened, name, e.content.display())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kerosene-recent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_newest_is_first_and_a_project_is_listed_once() {
        let mut recent = Recent::default();
        recent.add(Path::new("/a/content"), "A");
        recent.add(Path::new("/b/content"), "B");
        recent.add(Path::new("/a/content"), "A again");
        let names: Vec<&str> = recent.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["A again", "B"]);
    }

    #[test]
    fn the_list_is_capped() {
        let mut recent = Recent::default();
        for i in 0..LIMIT + 5 {
            recent.add(Path::new(&format!("/p{i}")), &format!("P{i}"));
        }
        assert_eq!(recent.entries.len(), LIMIT);
        assert_eq!(recent.entries[0].name, format!("P{}", LIMIT + 4));
    }

    #[test]
    fn what_is_written_is_read_back() {
        let dir = temp("roundtrip");
        let file = dir.join(FILE);
        let mut recent = Recent::load(file.clone());
        assert!(recent.entries.is_empty(), "no file is an empty list");
        recent.add(Path::new("/games/arena/content"), "Arena\tthe game");
        recent.add(Path::new("/games/other/content"), "Other");
        recent.remove(Path::new("/games/other/content"));
        let again = Recent::load(file);
        assert_eq!(again.entries.len(), 1);
        assert_eq!(again.entries[0].name, "Arena the game");
        assert_eq!(
            again.entries[0].content,
            PathBuf::from("/games/arena/content")
        );
        assert!(again.entries[0].age() < Duration::from_secs(60));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_line_is_skipped_rather_than_fatal() {
        let entries = parse("garbage\n12\tGood\t/x\nnot-a-number\tBad\t/y\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Good");
    }
}
