//! The `.vault` archive format -- VoidEngine's answer to Source's VPK.
//!
//! One file holding a whole content tree, so a mod ships as a handful of
//! archives instead of tens of thousands of loose files. Layout:
//!
//! ```text
//! [ header 40 bytes ]
//! [ directory: entry_count records, sorted by path ]
//! [ data blob ]
//! ```
//!
//! Each directory record is `path_len:u16, path, crc32:u32, offset:u64, size:u64`
//! with `offset` relative to the start of the data blob. Paths are stored
//! already normalised, so a lookup is a binary search with no per-query
//! allocation.
//!
//! Reads are checked against the stored CRC32. A corrupt archive is a
//! genuinely common failure -- a truncated download, a bad copy -- and it is
//! far better caught here than as a garbled texture three subsystems later.

use crate::path::normalize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use thiserror::Error;

const MAGIC: [u8; 4] = *b"VLT1";
const VERSION: u32 = 1;
const HEADER_SIZE: u64 = 40;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a .vault archive (bad magic)")]
    BadMagic { path: String },
    #[error("{path} is a version {found} archive; this build reads version {expected}")]
    BadVersion { path: String, found: u32, expected: u32 },
    #[error("{path} is truncated or malformed: {detail}")]
    Malformed { path: String, detail: String },
    #[error("{archive}: entry {entry:?} failed its checksum (archive is corrupt)")]
    ChecksumMismatch { archive: String, entry: String },
    #[error("{0:?} is not a usable virtual path")]
    BadPath(String),
}

type Result<T> = std::result::Result<T, ArchiveError>;

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub crc: u32,
    pub offset: u64,
    pub size: u64,
}

/// A mounted archive. The directory is held in memory; file data is read on
/// demand, so mounting a multi-gigabyte archive costs only its directory.
pub struct Archive {
    source: String,
    file: std::sync::Mutex<File>,
    data_offset: u64,
    entries: Vec<Entry>,
}

impl Archive {
    /// Read an archive's directory and keep its file handle open.
    pub fn open(path: &Path) -> Result<Archive> {
        let name = path.display().to_string();
        let io = |source| ArchiveError::Io { path: name.clone(), source };
        let mut file = File::open(path).map_err(io)?;

        let mut header = [0u8; HEADER_SIZE as usize];
        file.read_exact(&mut header).map_err(|_| ArchiveError::Malformed {
            path: name.clone(),
            detail: "file is shorter than a header".into(),
        })?;

        if header[0..4] != MAGIC { return Err(ArchiveError::BadMagic { path: name }); }
        let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
        if version != VERSION {
            return Err(ArchiveError::BadVersion { path: name, found: version, expected: VERSION });
        }
        let entry_count = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
        let tree_size = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let data_offset = u64::from_le_bytes(header[20..28].try_into().unwrap());
        let data_size = u64::from_le_bytes(header[28..36].try_into().unwrap());

        let file_len = file.metadata().map_err(io)?.len();
        if data_offset.saturating_add(data_size) > file_len {
            return Err(ArchiveError::Malformed {
                path: name,
                detail: format!(
                    "directory claims {} bytes of data but the file holds {file_len}",
                    data_offset + data_size
                ),
            });
        }

        let mut tree = vec![0u8; tree_size];
        file.read_exact(&mut tree).map_err(|_| ArchiveError::Malformed {
            path: name.clone(),
            detail: "directory is truncated".into(),
        })?;

        let mut entries = Vec::with_capacity(entry_count);
        let mut cur = 0usize;
        let need = |cur: usize, n: usize, len: usize| -> Result<()> {
            if cur + n > len {
                Err(ArchiveError::Malformed {
                    path: String::new(),
                    detail: "directory record runs past the end of the tree".into(),
                })
            } else {
                Ok(())
            }
        };
        for _ in 0..entry_count {
            need(cur, 2, tree.len()).map_err(|_| ArchiveError::Malformed {
                path: name.clone(),
                detail: "directory record runs past the end of the tree".into(),
            })?;
            let plen = u16::from_le_bytes(tree[cur..cur + 2].try_into().unwrap()) as usize;
            cur += 2;
            if cur + plen + 20 > tree.len() {
                return Err(ArchiveError::Malformed {
                    path: name,
                    detail: "directory record runs past the end of the tree".into(),
                });
            }
            let epath = String::from_utf8_lossy(&tree[cur..cur + plen]).into_owned();
            cur += plen;
            let crc = u32::from_le_bytes(tree[cur..cur + 4].try_into().unwrap());
            let offset = u64::from_le_bytes(tree[cur + 4..cur + 12].try_into().unwrap());
            let size = u64::from_le_bytes(tree[cur + 12..cur + 20].try_into().unwrap());
            cur += 20;

            if offset.saturating_add(size) > data_size {
                return Err(ArchiveError::Malformed {
                    path: name,
                    detail: format!("entry {epath:?} points outside the data blob"),
                });
            }
            entries.push(Entry { path: epath, crc, offset, size });
        }

        // The writer sorts, but a hand-made archive might not; a binary search
        // over an unsorted directory would miss files at random.
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(Archive {
            source: name,
            file: std::sync::Mutex::new(file),
            data_offset,
            entries,
        })
    }

    pub fn source(&self) -> &str { &self.source }
    pub fn entries(&self) -> &[Entry] { &self.entries }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    fn find(&self, vpath: &str) -> Option<&Entry> {
        let i = self.entries.binary_search_by(|e| e.path.as_str().cmp(vpath)).ok()?;
        Some(&self.entries[i])
    }

    pub fn contains(&self, vpath: &str) -> bool { self.find(vpath).is_some() }

    /// Read one file out of the archive, verifying its checksum.
    pub fn read(&self, vpath: &str) -> Result<Option<Vec<u8>>> {
        let Some(entry) = self.find(vpath) else { return Ok(None) };
        let mut file = self.file.lock().expect("archive handle poisoned");
        let io = |source| ArchiveError::Io { path: self.source.clone(), source };
        file.seek(SeekFrom::Start(self.data_offset + entry.offset)).map_err(io)?;
        let mut buf = vec![0u8; entry.size as usize];
        file.read_exact(&mut buf).map_err(io)?;
        if crc32(&buf) != entry.crc {
            return Err(ArchiveError::ChecksumMismatch {
                archive: self.source.clone(),
                entry: vpath.to_string(),
            });
        }
        Ok(Some(buf))
    }

    /// Entries under `dir`, optionally filtered by extension.
    pub fn list(&self, dir: &str, ext: Option<&str>) -> Vec<String> {
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        self.entries
            .iter()
            .filter(|e| e.path.starts_with(&prefix))
            .filter(|e| match ext {
                Some(ext) => e.path.rsplit('.').next() == Some(ext),
                None => true,
            })
            .map(|e| e.path.clone())
            .collect()
    }
}

/// Builds a `.vault` archive.
///
/// Contents are staged in memory keyed by normalised path, which gets
/// deduplication and deterministic ordering for free: two calls with the same
/// path collapse, and the output byte-for-byte reproduces across runs.
#[derive(Default)]
pub struct ArchiveBuilder {
    files: BTreeMap<String, Vec<u8>>,
}

impl ArchiveBuilder {
    pub fn new() -> Self { Self::default() }

    /// Stage a file. A repeated path replaces the earlier content.
    pub fn add(&mut self, vpath: &str, data: Vec<u8>) -> Result<()> {
        let key = normalize(vpath).ok_or_else(|| ArchiveError::BadPath(vpath.to_string()))?;
        self.files.insert(key, data);
        Ok(())
    }

    /// Stage a file from disk under the given virtual path.
    pub fn add_file(&mut self, vpath: &str, disk: &Path) -> Result<()> {
        let data = std::fs::read(disk).map_err(|source| ArchiveError::Io {
            path: disk.display().to_string(),
            source,
        })?;
        self.add(vpath, data)
    }

    pub fn len(&self) -> usize { self.files.len() }
    pub fn is_empty(&self) -> bool { self.files.is_empty() }
    pub fn paths(&self) -> impl Iterator<Item = &str> { self.files.keys().map(|s| s.as_str()) }

    /// Write the archive out.
    pub fn write(&self, out: &Path) -> Result<u64> {
        let name = out.display().to_string();
        let io = |source| ArchiveError::Io { path: name.clone(), source };
        let file = File::create(out).map_err(io)?;
        let mut w = BufWriter::new(file);

        // Lay out the data blob first so the directory can record real offsets.
        let mut tree = Vec::new();
        let mut offset = 0u64;
        for (path, data) in &self.files {
            tree.extend_from_slice(&(path.len() as u16).to_le_bytes());
            tree.extend_from_slice(path.as_bytes());
            tree.extend_from_slice(&crc32(data).to_le_bytes());
            tree.extend_from_slice(&offset.to_le_bytes());
            tree.extend_from_slice(&(data.len() as u64).to_le_bytes());
            offset += data.len() as u64;
        }
        let data_size = offset;
        let data_offset = HEADER_SIZE + tree.len() as u64;

        let mut header = Vec::with_capacity(HEADER_SIZE as usize);
        header.extend_from_slice(&MAGIC);
        header.extend_from_slice(&VERSION.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // flags, reserved
        header.extend_from_slice(&(self.files.len() as u32).to_le_bytes());
        header.extend_from_slice(&(tree.len() as u32).to_le_bytes());
        header.extend_from_slice(&data_offset.to_le_bytes());
        header.extend_from_slice(&data_size.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // pad to 40
        debug_assert_eq!(header.len() as u64, HEADER_SIZE);

        w.write_all(&header).map_err(io)?;
        w.write_all(&tree).map_err(io)?;
        for data in self.files.values() { w.write_all(data).map_err(io)?; }
        w.flush().map_err(io)?;
        Ok(data_offset + data_size)
    }
}

/// CRC-32 (IEEE), computed with a lazily built table.
pub fn crc32(data: &[u8]) -> u32 {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("voidvault-{}-{name}", std::process::id()));
        p
    }

    #[test]
    fn crc32_matches_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn round_trips_content() {
        let out = tmp("roundtrip.vault");
        let mut b = ArchiveBuilder::new();
        b.add("materials/dev/grid.voidmat", b"shader { }".to_vec()).unwrap();
        b.add(r"Maps\Void_Start.voidbsp", vec![7u8; 5000]).unwrap();
        b.write(&out).unwrap();

        let a = Archive::open(&out).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a.read("materials/dev/grid.voidmat").unwrap().unwrap(), b"shader { }");
        // Path was normalised on the way in, so it reads back lowercase.
        assert_eq!(a.read("maps/void_start.voidbsp").unwrap().unwrap().len(), 5000);
        assert!(a.read("nothing/here").unwrap().is_none());
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn output_is_byte_for_byte_reproducible() {
        // Insertion order must not leak into the file; reproducible builds of
        // content archives matter for patching and for diffing releases.
        let (p1, p2) = (tmp("repro-a.vault"), tmp("repro-b.vault"));
        let mut a = ArchiveBuilder::new();
        a.add("b.txt", b"bbb".to_vec()).unwrap();
        a.add("a.txt", b"aaa".to_vec()).unwrap();
        a.write(&p1).unwrap();
        let mut b = ArchiveBuilder::new();
        b.add("a.txt", b"aaa".to_vec()).unwrap();
        b.add("b.txt", b"bbb".to_vec()).unwrap();
        b.write(&p2).unwrap();
        assert_eq!(std::fs::read(&p1).unwrap(), std::fs::read(&p2).unwrap());
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn corruption_is_caught_on_read() {
        let out = tmp("corrupt.vault");
        let mut b = ArchiveBuilder::new();
        b.add("a.txt", b"the original bytes".to_vec()).unwrap();
        b.write(&out).unwrap();

        // Flip a byte in the data blob.
        let mut bytes = std::fs::read(&out).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(&out, &bytes).unwrap();

        let a = Archive::open(&out).unwrap();
        assert!(matches!(a.read("a.txt"), Err(ArchiveError::ChecksumMismatch { .. })));
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn garbage_is_rejected_rather_than_read() {
        let out = tmp("garbage.vault");
        std::fs::write(&out, b"this is definitely not an archive at all!!").unwrap();
        assert!(matches!(Archive::open(&out), Err(ArchiveError::BadMagic { .. })));
        std::fs::write(&out, b"tiny").unwrap();
        assert!(matches!(Archive::open(&out), Err(ArchiveError::Malformed { .. })));
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn truncated_archive_is_rejected() {
        let out = tmp("trunc.vault");
        let mut b = ArchiveBuilder::new();
        b.add("a.txt", vec![1u8; 4096]).unwrap();
        b.write(&out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        std::fs::write(&out, &bytes[..bytes.len() / 2]).unwrap();
        assert!(Archive::open(&out).is_err());
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn listing_filters_by_directory_and_extension() {
        let out = tmp("list.vault");
        let mut b = ArchiveBuilder::new();
        b.add("materials/a.voidmat", b"1".to_vec()).unwrap();
        b.add("materials/b.voidtex", b"2".to_vec()).unwrap();
        b.add("maps/c.voidbsp", b"3".to_vec()).unwrap();
        b.write(&out).unwrap();
        let a = Archive::open(&out).unwrap();
        assert_eq!(a.list("materials", None).len(), 2);
        assert_eq!(a.list("materials", Some("voidmat")), vec!["materials/a.voidmat"]);
        assert_eq!(a.list("", None).len(), 3);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn traversal_paths_are_refused_at_build_time() {
        let mut b = ArchiveBuilder::new();
        assert!(matches!(b.add("../escape.txt", vec![]), Err(ArchiveError::BadPath(_))));
    }

    #[test]
    fn empty_archive_is_valid() {
        let out = tmp("empty.vault");
        ArchiveBuilder::new().write(&out).unwrap();
        let a = Archive::open(&out).unwrap();
        assert!(a.is_empty());
        let _ = std::fs::remove_file(&out);
    }
}
