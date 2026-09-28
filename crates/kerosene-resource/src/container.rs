// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The one binary container every compiled resource is written in.
//!
//! ```text
//! offset  size  field
//!      0     4  magic, "KRES"
//!      4     4  container version (this file's layout)
//!      8     4  kind: four bytes naming the payload, "KMAT"
//!     12     4  kind version (the payload's layout)
//!     16     8  source hash: FNV-1a of the source the file was compiled from
//!     24     4  block count
//!     28     4  reserved, zero
//!     32  12*n  block table: tag (4), offset (4), length (4)
//!      …        blocks, each starting on a 16-byte boundary
//! ```
//!
//! All integers are little-endian. Three block tags mean the same thing in
//! every kind:
//!
//! - `DATA`, the payload, whose layout the kind and its version decide;
//! - `REFS`, the other resources this one needs, as virtual paths -- what a
//!   packager follows and a loader can preload;
//! - `EDIT`, how the file was made: the compiler, its arguments and each
//!   input with its hash, so a tool can tell when the file is stale.
//!
//! A kind may add blocks of its own. A reader ignores the tags it does not
//! know, so a block can be added without a version bump.

use crate::bytes::{Reader, Writer};
use crate::{ResourceError, fourcc};

/// The first four bytes of every compiled resource.
pub const MAGIC: [u8; 4] = *b"KRES";
/// The container layout this build reads and writes.
pub const CONTAINER_VERSION: u32 = 1;

const HEADER_SIZE: usize = 32;
const BLOCK_ENTRY_SIZE: usize = 12;
const BLOCK_ALIGN: usize = 16;

/// The block tags every kind shares.
pub mod tag {
    /// The payload.
    pub const DATA: [u8; 4] = *b"DATA";
    /// External references: the other resources this one needs.
    pub const REFS: [u8; 4] = *b"REFS";
    /// Edit info: how the file was compiled, and from what.
    pub const EDIT: [u8; 4] = *b"EDIT";
}

/// Whether `bytes` start like a compiled resource.
///
/// Only the magic is looked at: a file can pass this and still fail to
/// parse. It is how a loader tells the container from a format that
/// predates it.
pub fn is_resource(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == MAGIC
}

/// A 64-bit FNV-1a hash, for the source hash and the edit info's inputs.
///
/// Not cryptographic, and not meant to be: it answers "has this source
/// changed since it was compiled", and must give the same answer on every
/// machine and in every build, which is why it is written out here rather
/// than borrowed from the standard library's randomly seeded hasher.
pub fn source_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// How a resource was compiled: the `EDIT` block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditInfo {
    /// What compiled it, `alchemy` say.
    pub compiler: String,
    /// The arguments it was given that change the output.
    pub args: Vec<String>,
    /// Each file read, as the path the compiler was given, with the hash of
    /// its bytes.
    pub inputs: Vec<(String, u64)>,
}

impl EditInfo {
    fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.str(&self.compiler);
        w.strs(&self.args);
        w.u32(self.inputs.len() as u32);
        for (path, hash) in &self.inputs {
            w.str(path);
            w.u64(*hash);
        }
        w.finish()
    }

    fn decode(bytes: &[u8]) -> Result<EditInfo, ResourceError> {
        let mut r = Reader::new(bytes);
        let compiler = r.str()?.to_string();
        let args = r.strs()?.into_iter().map(str::to_string).collect();
        let count = r.u32()? as usize;
        let mut inputs = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            let path = r.str()?.to_string();
            inputs.push((path, r.u64()?));
        }
        Ok(EditInfo {
            compiler,
            args,
            inputs,
        })
    }
}

/// A compiled resource being written.
///
/// ```
/// use kerosene_resource::{ResourceFile, ResourceView, tag};
///
/// let mut file = ResourceFile::new(*b"TEST", 1);
/// file.push(tag::DATA, b"payload".to_vec());
/// file.set_refs(["materials/dev/grid.ktex"]);
/// let bytes = file.to_bytes();
///
/// let view = ResourceView::parse(&bytes).unwrap();
/// assert_eq!(view.block(tag::DATA), Some(&b"payload"[..]));
/// assert_eq!(view.refs().unwrap(), ["materials/dev/grid.ktex"]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceFile {
    pub kind: [u8; 4],
    pub kind_version: u32,
    pub source_hash: u64,
    blocks: Vec<([u8; 4], Vec<u8>)>,
}

impl ResourceFile {
    pub fn new(kind: [u8; 4], kind_version: u32) -> Self {
        ResourceFile {
            kind,
            kind_version,
            source_hash: 0,
            blocks: Vec::new(),
        }
    }

    /// Record the hash of the source this was compiled from.
    pub fn with_source(mut self, source: &[u8]) -> Self {
        self.source_hash = source_hash(source);
        self
    }

    /// Add a block, replacing any earlier one with the same tag.
    pub fn push(&mut self, tag: [u8; 4], data: Vec<u8>) -> &mut Self {
        self.blocks.retain(|(t, _)| *t != tag);
        self.blocks.push((tag, data));
        self
    }

    /// Set the external references. Written sorted and without repeats, so
    /// the same inputs always make the same bytes.
    pub fn set_refs<S: AsRef<str>>(&mut self, refs: impl IntoIterator<Item = S>) -> &mut Self {
        let mut refs: Vec<String> = refs.into_iter().map(|r| r.as_ref().to_string()).collect();
        refs.sort();
        refs.dedup();
        let mut w = Writer::new();
        w.strs(&refs);
        self.push(tag::REFS, w.finish())
    }

    pub fn set_edit_info(&mut self, info: &EditInfo) -> &mut Self {
        self.push(tag::EDIT, info.encode())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let table_end = HEADER_SIZE + self.blocks.len() * BLOCK_ENTRY_SIZE;
        let mut offsets = Vec::with_capacity(self.blocks.len());
        let mut at = align(table_end);
        for (_, data) in &self.blocks {
            offsets.push(at);
            at = align(at + data.len());
        }

        let mut out = Vec::with_capacity(at);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&CONTAINER_VERSION.to_le_bytes());
        out.extend_from_slice(&self.kind);
        out.extend_from_slice(&self.kind_version.to_le_bytes());
        out.extend_from_slice(&self.source_hash.to_le_bytes());
        out.extend_from_slice(&(self.blocks.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for ((tag, data), offset) in self.blocks.iter().zip(&offsets) {
            out.extend_from_slice(tag);
            out.extend_from_slice(&(*offset as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        }
        for ((_, data), offset) in self.blocks.iter().zip(&offsets) {
            out.resize(*offset, 0);
            out.extend_from_slice(data);
        }
        out
    }
}

fn align(n: usize) -> usize {
    n.div_ceil(BLOCK_ALIGN) * BLOCK_ALIGN
}

/// A compiled resource being read, borrowing the bytes it was parsed from.
#[derive(Clone, Debug)]
pub struct ResourceView<'a> {
    kind: [u8; 4],
    kind_version: u32,
    source_hash: u64,
    blocks: Vec<([u8; 4], &'a [u8])>,
}

impl<'a> ResourceView<'a> {
    /// Check the header and the block table. Every block is in bounds once
    /// this succeeds; what is *in* a block is its reader's business.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ResourceError> {
        if !is_resource(bytes) {
            return Err(ResourceError::NotAResource);
        }
        let header = bytes.get(..HEADER_SIZE).ok_or(ResourceError::Truncated {
            needed: HEADER_SIZE,
            available: bytes.len(),
        })?;
        let u32_at = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        let version = u32_at(4);
        if version != CONTAINER_VERSION {
            return Err(ResourceError::ContainerVersion(version));
        }
        let kind: [u8; 4] = header[8..12].try_into().unwrap();
        let kind_version = u32_at(12);
        let source_hash = u64::from_le_bytes(header[16..24].try_into().unwrap());
        let count = u32_at(24) as usize;

        let table_end = count
            .checked_mul(BLOCK_ENTRY_SIZE)
            .and_then(|n| n.checked_add(HEADER_SIZE))
            .filter(|&end| end <= bytes.len())
            .ok_or(ResourceError::Truncated {
                needed: HEADER_SIZE + count.saturating_mul(BLOCK_ENTRY_SIZE),
                available: bytes.len(),
            })?;
        let mut blocks = Vec::with_capacity(count);
        for entry in bytes[HEADER_SIZE..table_end]
            .as_chunks::<BLOCK_ENTRY_SIZE>()
            .0
        {
            let tag: [u8; 4] = entry[0..4].try_into().unwrap();
            let offset = u32::from_le_bytes(entry[4..8].try_into().unwrap()) as usize;
            let len = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
            let data = offset
                .checked_add(len)
                .filter(|&end| offset >= table_end && end <= bytes.len())
                .map(|end| &bytes[offset..end])
                .ok_or_else(|| ResourceError::BadBlock {
                    tag: fourcc(tag),
                    reason: format!(
                        "{len} bytes at {offset} is outside the file's {} bytes",
                        bytes.len()
                    ),
                })?;
            blocks.push((tag, data));
        }
        Ok(ResourceView {
            kind,
            kind_version,
            source_hash,
            blocks,
        })
    }

    pub fn kind(&self) -> [u8; 4] {
        self.kind
    }

    pub fn kind_version(&self) -> u32 {
        self.kind_version
    }

    pub fn source_hash(&self) -> u64 {
        self.source_hash
    }

    /// The block with this tag, if the file has one.
    pub fn block(&self, tag: [u8; 4]) -> Option<&'a [u8]> {
        self.blocks.iter().find(|(t, _)| *t == tag).map(|(_, d)| *d)
    }

    /// The block with this tag, or an error naming it.
    pub fn require(&self, tag: [u8; 4]) -> Result<&'a [u8], ResourceError> {
        self.block(tag)
            .ok_or_else(|| ResourceError::MissingBlock(fourcc(tag)))
    }

    /// The tags of every block, in file order.
    pub fn tags(&self) -> impl Iterator<Item = [u8; 4]> + '_ {
        self.blocks.iter().map(|(t, _)| *t)
    }

    /// The external references; empty when there is no `REFS` block.
    pub fn refs(&self) -> Result<Vec<&'a str>, ResourceError> {
        match self.block(tag::REFS) {
            Some(b) => Reader::new(b).strs(),
            None => Ok(Vec::new()),
        }
    }

    /// The edit info, when the file has any.
    pub fn edit_info(&self) -> Result<Option<EditInfo>, ResourceError> {
        self.block(tag::EDIT).map(EditInfo::decode).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ResourceFile {
        let mut f = ResourceFile::new(*b"TEST", 3).with_source(b"source text");
        f.push(tag::DATA, vec![1, 2, 3]);
        f.push(*b"XTRA", vec![9; 40]);
        f.set_refs(["b.ktex", "a.ktex", "b.ktex"]);
        f.set_edit_info(&EditInfo {
            compiler: "alchemy".into(),
            args: vec!["--fast".into()],
            inputs: vec![("a.kmat".into(), 7)],
        });
        f
    }

    #[test]
    fn a_file_reads_back_as_it_was_written() {
        let bytes = sample().to_bytes();
        let v = ResourceView::parse(&bytes).unwrap();
        assert_eq!(v.kind(), *b"TEST");
        assert_eq!(v.kind_version(), 3);
        assert_eq!(v.source_hash(), source_hash(b"source text"));
        assert_eq!(v.block(tag::DATA), Some(&[1, 2, 3][..]));
        assert_eq!(v.block(*b"XTRA").unwrap().len(), 40);
        assert_eq!(
            v.refs().unwrap(),
            ["a.ktex", "b.ktex"],
            "sorted, no repeats"
        );
        let edit = v.edit_info().unwrap().unwrap();
        assert_eq!(edit.compiler, "alchemy");
        assert_eq!(edit.inputs, [("a.kmat".to_string(), 7)]);
        assert!(v.block(*b"NONE").is_none());
    }

    #[test]
    fn every_block_starts_on_a_16_byte_boundary() {
        let bytes = sample().to_bytes();
        let count = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
        for i in 0..count {
            let at = HEADER_SIZE + i * BLOCK_ENTRY_SIZE + 4;
            let offset = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            assert_eq!(offset % 16, 0);
        }
    }

    #[test]
    fn the_same_input_makes_the_same_bytes() {
        assert_eq!(sample().to_bytes(), sample().to_bytes());
    }

    #[test]
    fn pushing_a_tag_twice_keeps_the_last() {
        let mut f = ResourceFile::new(*b"TEST", 1);
        f.push(tag::DATA, vec![1]);
        f.push(tag::DATA, vec![2]);
        let bytes = f.to_bytes();
        let v = ResourceView::parse(&bytes).unwrap();
        assert_eq!(v.tags().count(), 1);
        assert_eq!(v.block(tag::DATA), Some(&[2][..]));
    }

    #[test]
    fn damage_is_an_error_not_a_panic() {
        let bytes = sample().to_bytes();
        // Every truncation, and a garbage byte at every position.
        for len in 0..bytes.len() {
            let _ = ResourceView::parse(&bytes[..len]);
        }
        for i in 0..bytes.len() {
            let mut b = bytes.clone();
            b[i] ^= 0xff;
            if let Ok(v) = ResourceView::parse(&b) {
                let _ = v.refs();
                let _ = v.edit_info();
            }
        }
        assert!(matches!(
            ResourceView::parse(b"KRES"),
            Err(ResourceError::Truncated { .. })
        ));
        assert!(matches!(
            ResourceView::parse(b"KRMD........"),
            Err(ResourceError::NotAResource)
        ));
    }

    #[test]
    fn a_block_pointing_into_the_header_is_refused() {
        let mut bytes = sample().to_bytes();
        let at = HEADER_SIZE + 4;
        bytes[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            ResourceView::parse(&bytes),
            Err(ResourceError::BadBlock { .. })
        ));
    }

    #[test]
    fn a_newer_container_is_refused_by_name() {
        let mut bytes = sample().to_bytes();
        bytes[4..8].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            ResourceView::parse(&bytes),
            Err(ResourceError::ContainerVersion(99))
        ));
    }

    #[test]
    fn the_source_hash_is_fnv1a() {
        // The published test vectors: the hash is part of the file format,
        // so it must never drift.
        assert_eq!(source_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(source_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(source_hash(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
