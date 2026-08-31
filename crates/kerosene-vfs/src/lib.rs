// SPDX-License-Identifier: MPL-2.0
//! The virtual filesystem: search paths and mounted archives.
//!
//! Source's filesystem lets a mod, the base game and a set of VPKs stack into
//! one namespace, so `materials/dev/grid.keromat` resolves against whichever
//! layer provides it first. That is what makes a mod a mod -- you drop in
//! files that shadow the base game's without touching it. Kerosene works the
//! same way, with [`Vault`](archive) archives standing in for VPKs.
//!
//! Search paths are consulted **in the order they were added**, first hit
//! wins, so the caller controls override precedence explicitly:
//!
//! ```no_run
//! # use kerosene_vfs::Vfs;
//! # use std::path::Path;
//! let mut vfs = Vfs::new();
//! vfs.add_directory(Path::new("mods/mymod"), "MOD");   // searched first
//! vfs.add_directory(Path::new("content"), "GAME");     // fallback
//! let bytes = vfs.read("materials/dev/grid.keromat").unwrap();
//! ```

pub mod archive;
pub mod path;
pub mod project;
pub mod toolchain;
pub mod root;

pub use archive::{Archive, ArchiveBuilder, ArchiveError, crc32};
pub use path::{extension, normalize, parent, with_extension};
pub use project::Project;
pub use root::Found;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VfsError {
    #[error("{0:?} is not a usable virtual path")]
    BadPath(String),
    #[error("{0:?} was not found in any search path")]
    NotFound(String),
    #[error("no writable search path is mounted (every path is an archive)")]
    NoWritablePath,
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not valid UTF-8")]
    NotUtf8 { path: String },
}

type Result<T> = std::result::Result<T, VfsError>;

enum Layer {
    Directory(PathBuf),
    Archive(Box<Archive>),
}

struct SearchPath {
    layer: Layer,
    /// Group label, matching Source's path IDs: `GAME`, `MOD`, `PLATFORM`.
    /// Lookups can be restricted to one group.
    id: String,
}

/// A stack of search paths forming one virtual content tree.
#[derive(Default)]
pub struct Vfs {
    paths: Vec<SearchPath>,
}

impl Vfs {
    pub fn new() -> Self { Self::default() }

    /// Add a directory to the end of the search order.
    pub fn add_directory(&mut self, dir: &Path, id: &str) -> &mut Self {
        self.paths.push(SearchPath {
            layer: Layer::Directory(dir.to_path_buf()),
            id: id.to_string(),
        });
        self
    }

    /// Add a directory at the *front*, so it overrides everything mounted so far.
    pub fn add_directory_front(&mut self, dir: &Path, id: &str) -> &mut Self {
        self.paths.insert(0, SearchPath {
            layer: Layer::Directory(dir.to_path_buf()),
            id: id.to_string(),
        });
        self
    }

    /// Mount a `.vault` archive at the end of the search order.
    pub fn mount_archive(&mut self, file: &Path, id: &str) -> Result<&mut Self> {
        let archive = Archive::open(file)?;
        self.paths.push(SearchPath {
            layer: Layer::Archive(Box::new(archive)),
            id: id.to_string(),
        });
        Ok(self)
    }

    /// Mount every `.vault` in a directory, in sorted order.
    ///
    /// Sorted rather than whatever the OS hands back, so that a content set
    /// resolves identically on every machine -- directory iteration order is
    /// not stable across filesystems and a mod that works only on one
    /// developer's box is a miserable bug to chase.
    pub fn mount_archives_in(&mut self, dir: &Path, id: &str) -> Result<usize> {
        let Ok(read_dir) = std::fs::read_dir(dir) else { return Ok(0) };
        let mut files: Vec<PathBuf> = read_dir
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vault")))
            .collect();
        files.sort();
        let n = files.len();
        for f in files { self.mount_archive(&f, id)?; }
        Ok(n)
    }

    pub fn path_count(&self) -> usize { self.paths.len() }

    /// Describe the mounted layers, in search order -- what a `path` console
    /// command prints.
    pub fn describe(&self) -> Vec<String> {
        self.paths
            .iter()
            .map(|p| match &p.layer {
                Layer::Directory(d) => format!("{} (dir) {}", p.id, d.display()),
                Layer::Archive(a) => format!("{} (vault, {} files) {}", p.id, a.len(), a.source()),
            })
            .collect()
    }

    /// Read a file from the first search path that provides it.
    pub fn read(&self, vpath: &str) -> Result<Vec<u8>> {
        let key = normalize(vpath).ok_or_else(|| VfsError::BadPath(vpath.to_string()))?;
        self.read_normalized(&key, None)?
            .ok_or_else(|| VfsError::NotFound(vpath.to_string()))
    }

    /// Read, but only from search paths carrying the given id.
    pub fn read_from(&self, vpath: &str, id: &str) -> Result<Vec<u8>> {
        let key = normalize(vpath).ok_or_else(|| VfsError::BadPath(vpath.to_string()))?;
        self.read_normalized(&key, Some(id))?
            .ok_or_else(|| VfsError::NotFound(vpath.to_string()))
    }

    fn read_normalized(&self, key: &str, id: Option<&str>) -> Result<Option<Vec<u8>>> {
        let folded = key.to_lowercase();
        for sp in &self.paths {
            if id.is_some_and(|want| sp.id != want) { continue; }
            match &sp.layer {
                Layer::Directory(dir) => {
                    let full = dir.join(key);
                    match std::fs::read(&full) {
                        Ok(bytes) => return Ok(Some(bytes)),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            // The name as written missed. Content paths are
                            // case-insensitive by contract -- an archive gives
                            // that for free -- so a loose tree has to agree,
                            // or a game works from a checkout and breaks the
                            // moment it is packed.
                            match path::find_ignoring_case(dir, key) {
                                Some(found) => return Ok(Some(std::fs::read(&found).map_err(
                                    |source| VfsError::Io {
                                        path: found.display().to_string(),
                                        source,
                                    },
                                )?)),
                                None => continue,
                            }
                        }
                        // A permissions error or a bad sector is worth
                        // surfacing; quietly falling through to a stale copy
                        // in a lower layer would be worse than failing.
                        Err(source) => {
                            return Err(VfsError::Io { path: full.display().to_string(), source });
                        }
                    }
                }
                Layer::Archive(a) => {
                    if let Some(bytes) = a.read(&folded)? { return Ok(Some(bytes)); }
                }
            }
        }
        Ok(None)
    }

    /// Read a file as UTF-8 text.
    pub fn read_string(&self, vpath: &str) -> Result<String> {
        let bytes = self.read(vpath)?;
        String::from_utf8(bytes).map_err(|_| VfsError::NotUtf8 { path: vpath.to_string() })
    }

    /// Read a file, returning `None` if it is simply absent.
    pub fn read_optional(&self, vpath: &str) -> Result<Option<Vec<u8>>> {
        match self.read(vpath) {
            Ok(b) => Ok(Some(b)),
            Err(VfsError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn exists(&self, vpath: &str) -> bool {
        let Some(key) = normalize(vpath) else { return false };
        let folded = key.to_lowercase();
        self.paths.iter().any(|sp| match &sp.layer {
            Layer::Directory(dir) => {
                dir.join(&key).is_file() || path::find_ignoring_case(dir, &key).is_some()
            }
            Layer::Archive(a) => a.contains(&folded),
        })
    }

    /// Which layer would serve this path -- what a `whereis` command reports.
    pub fn locate(&self, vpath: &str) -> Option<String> {
        let key = normalize(vpath)?;
        let folded = key.to_lowercase();
        self.paths.iter().find_map(|sp| match &sp.layer {
            Layer::Directory(dir) => {
                let exact = dir.join(&key);
                if exact.is_file() {
                    return Some(exact.display().to_string());
                }
                path::find_ignoring_case(dir, &key).map(|p| p.display().to_string())
            }
            Layer::Archive(a) => a.contains(&folded).then(|| format!("{}:{folded}", a.source())),
        })
    }

    /// Every file under `dir`, across all layers, deduplicated and sorted.
    pub fn list(&self, dir: &str, ext: Option<&str>) -> Vec<String> {
        let key = normalize(dir).unwrap_or_default();
        let mut out: BTreeSet<String> = BTreeSet::new();
        for sp in &self.paths {
            match &sp.layer {
                Layer::Directory(root) => {
                    collect_dir(&root.join(&key), &key, ext, &mut out);
                }
                Layer::Archive(a) => out.extend(a.list(&key, ext)),
            }
        }
        out.into_iter().collect()
    }

    /// Write a file into the first directory layer.
    ///
    /// Archives are read-only by design: mutating a mounted archive under a
    /// running engine would invalidate every offset it has cached.
    pub fn write(&self, vpath: &str, data: &[u8]) -> Result<PathBuf> {
        let key = normalize(vpath).ok_or_else(|| VfsError::BadPath(vpath.to_string()))?;
        let dir = self
            .paths
            .iter()
            .find_map(|sp| match &sp.layer {
                Layer::Directory(d) => Some(d.clone()),
                Layer::Archive(_) => None,
            })
            .ok_or(VfsError::NoWritablePath)?;
        let full = dir.join(&key);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|source| VfsError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }
        std::fs::write(&full, data).map_err(|source| VfsError::Io {
            path: full.display().to_string(),
            source,
        })?;
        Ok(full)
    }
}

fn collect_dir(disk: &Path, prefix: &str, ext: Option<&str>, out: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(disk) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        let vpath = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let ty = match e.file_type() { Ok(t) => t, Err(_) => continue };
        if ty.is_dir() {
            collect_dir(&e.path(), &vpath, ext, out);
        } else if ext.is_none_or(|want| vpath.rsplit('.').next() == Some(want)) {
            out.insert(vpath);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("kerosenevfs-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn file(&self, rel: &str, body: &[u8]) -> PathBuf {
            let full = self.0.join(rel);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, body).unwrap();
            full
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn first_matching_layer_wins() {
        let base = TempDir::new("base");
        let modd = TempDir::new("mod");
        base.file("materials/grid.keromat", b"base version");
        modd.file("materials/grid.keromat", b"mod version");

        let mut vfs = Vfs::new();
        vfs.add_directory(&modd.0, "MOD");
        vfs.add_directory(&base.0, "GAME");
        assert_eq!(vfs.read("materials/grid.keromat").unwrap(), b"mod version");

        // Adding at the front overrides even the mod.
        let over = TempDir::new("over");
        over.file("materials/grid.keromat", b"override");
        vfs.add_directory_front(&over.0, "OVERRIDE");
        assert_eq!(vfs.read("materials/grid.keromat").unwrap(), b"override");
    }

    #[test]
    fn falls_through_to_a_lower_layer_when_absent() {
        let base = TempDir::new("ft-base");
        let modd = TempDir::new("ft-mod");
        base.file("maps/a.kerobsp", b"only in base");
        let mut vfs = Vfs::new();
        vfs.add_directory(&modd.0, "MOD");
        vfs.add_directory(&base.0, "GAME");
        assert_eq!(vfs.read("maps/a.kerobsp").unwrap(), b"only in base");
    }

    #[test]
    fn archives_and_directories_share_one_namespace() {
        let dir = TempDir::new("mix");
        dir.file("materials/loose.keromat", b"loose");
        let vault = dir.0.join("content.vault");
        let mut b = ArchiveBuilder::new();
        b.add("materials/packed.keromat", b"packed".to_vec()).unwrap();
        b.write(&vault).unwrap();

        let mut vfs = Vfs::new();
        vfs.add_directory(&dir.0, "GAME");
        vfs.mount_archive(&vault, "GAME").unwrap();
        assert_eq!(vfs.read("materials/loose.keromat").unwrap(), b"loose");
        assert_eq!(vfs.read("materials/packed.keromat").unwrap(), b"packed");
        assert_eq!(vfs.list("materials", Some("keromat")).len(), 2);
    }

    #[test]
    fn loose_files_shadow_packed_ones() {
        // The mod-development workflow: drop a loose file next to a shipped
        // archive and have it win without repacking.
        let dir = TempDir::new("shadow");
        let vault = dir.0.join("c.vault");
        let mut b = ArchiveBuilder::new();
        b.add("materials/x.keromat", b"packed".to_vec()).unwrap();
        b.write(&vault).unwrap();
        dir.file("materials/x.keromat", b"loose wins");

        let mut vfs = Vfs::new();
        vfs.add_directory(&dir.0, "GAME");
        vfs.mount_archive(&vault, "GAME").unwrap();
        assert_eq!(vfs.read("materials/x.keromat").unwrap(), b"loose wins");
    }

    #[test]
    fn case_and_separator_insensitive() {
        let dir = TempDir::new("case");
        dir.file("materials/dev/grid.keromat", b"x");
        let mut vfs = Vfs::new();
        vfs.add_directory(&dir.0, "GAME");
        assert!(vfs.exists(r"Materials\Dev\Grid.keromat"));
        assert!(vfs.read(r"MATERIALS/DEV/GRID.KEROMAT").is_ok());
    }

    #[test]
    fn traversal_cannot_escape_the_search_root() {
        let dir = TempDir::new("escape");
        dir.file("inside.txt", b"x");
        let mut vfs = Vfs::new();
        vfs.add_directory(&dir.0, "GAME");
        assert!(matches!(vfs.read("../../../etc/passwd"), Err(VfsError::BadPath(_))));
        assert!(!vfs.exists("../secrets"));
    }

    #[test]
    fn missing_file_is_distinguishable_from_an_error() {
        let dir = TempDir::new("missing");
        let mut vfs = Vfs::new();
        vfs.add_directory(&dir.0, "GAME");
        assert!(matches!(vfs.read("nope.txt"), Err(VfsError::NotFound(_))));
        assert!(vfs.read_optional("nope.txt").unwrap().is_none());
    }

    #[test]
    fn writes_land_in_the_first_directory_layer() {
        let dir = TempDir::new("write");
        let mut vfs = Vfs::new();
        vfs.add_directory(&dir.0, "GAME");
        vfs.write("maps/generated.kerobsp", b"data").unwrap();
        assert_eq!(vfs.read("maps/generated.kerobsp").unwrap(), b"data");
    }

    #[test]
    fn writing_with_only_archives_mounted_is_an_error() {
        let dir = TempDir::new("nowrite");
        let vault = dir.0.join("c.vault");
        ArchiveBuilder::new().write(&vault).unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_archive(&vault, "GAME").unwrap();
        assert!(matches!(vfs.write("a.txt", b"x"), Err(VfsError::NoWritablePath)));
    }

    #[test]
    fn read_from_restricts_to_one_path_id() {
        let base = TempDir::new("id-base");
        let modd = TempDir::new("id-mod");
        base.file("a.txt", b"base");
        modd.file("b.txt", b"mod");
        let mut vfs = Vfs::new();
        vfs.add_directory(&modd.0, "MOD");
        vfs.add_directory(&base.0, "GAME");
        assert_eq!(vfs.read_from("a.txt", "GAME").unwrap(), b"base");
        assert!(vfs.read_from("a.txt", "MOD").is_err());
    }

    #[test]
    fn listing_recurses_and_deduplicates() {
        let a = TempDir::new("list-a");
        let b = TempDir::new("list-b");
        a.file("materials/x.keromat", b"1");
        a.file("materials/sub/y.keromat", b"2");
        b.file("materials/x.keromat", b"dup");
        b.file("materials/z.kerotex", b"3");
        let mut vfs = Vfs::new();
        vfs.add_directory(&a.0, "MOD");
        vfs.add_directory(&b.0, "GAME");
        assert_eq!(
            vfs.list("materials", Some("keromat")),
            vec!["materials/sub/y.keromat", "materials/x.keromat"]
        );
    }

    #[test]
    fn locate_names_the_serving_layer() {
        let dir = TempDir::new("locate");
        dir.file("a.txt", b"x");
        let mut vfs = Vfs::new();
        vfs.add_directory(&dir.0, "GAME");
        assert!(vfs.locate("a.txt").unwrap().ends_with("a.txt"));
        assert!(vfs.locate("missing.txt").is_none());
    }
}
