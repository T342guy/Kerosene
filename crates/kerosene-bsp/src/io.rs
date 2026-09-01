// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Reading and writing `.kerobsp` files.
//!
//! The file is a header, a lump directory, and then the lumps. Every lump is a
//! flat array of one record type, so loading is a bounds check and a cast --
//! see [`crate::types`]. Lumps carry their own version number so a format bump
//! to one lump does not invalidate the rest of the file.

use crate::{Bsp, LUMP_COUNT, lumps};
use bytemuck::{Pod, cast_slice};
use std::io::Write;
use std::path::Path;
use thiserror::Error;

/// File magic. Not Source's `VBSP`, deliberately -- a Kerosene map is not a
/// Source map and mistaking one for the other should fail loudly.
pub const MAGIC: [u8; 4] = *b"KROS";
pub const VERSION: u32 = 1;

const HEADER_SIZE: usize = 4 + 4 + LUMP_COUNT * 16 + 4;

#[derive(Debug, Error)]
pub enum BspError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a .kerobsp file (bad magic)")]
    BadMagic { path: String },
    #[error("{path} is format version {found}; this build reads version {expected}. Recompile the map with Cleave.")]
    BadVersion { path: String, found: u32, expected: u32 },
    #[error("{path}: lump {lump} ({name}) runs from {offset} to {end} but the file is {size} bytes")]
    LumpOutOfRange { path: String, lump: usize, name: &'static str, offset: u64, end: u64, size: u64 },
    #[error("{path}: lump {name} is {size} bytes, not a whole number of {record}-byte records")]
    LumpMisaligned { path: String, name: &'static str, size: usize, record: usize },
    #[error("{path}: entity lump is not valid UTF-8")]
    EntitiesNotUtf8 { path: String },
    #[error("{path} is structurally invalid: {detail}")]
    Invalid { path: String, detail: String },
}

type Result<T> = std::result::Result<T, BspError>;

/// One entry in the lump directory.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LumpDir {
    pub offset: u32,
    pub length: u32,
    pub version: u32,
    pub ident: [u8; 4],
}

impl Bsp {
    pub fn load(path: &Path) -> Result<Bsp> {
        let bytes = std::fs::read(path).map_err(|source| BspError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Bsp::from_bytes(&bytes, &path.display().to_string())
    }

    pub fn from_bytes(bytes: &[u8], name: &str) -> Result<Bsp> {
        if bytes.len() < HEADER_SIZE {
            return Err(BspError::Invalid {
                path: name.to_string(),
                detail: format!("file is {} bytes, shorter than a header", bytes.len()),
            });
        }
        if bytes[0..4] != MAGIC { return Err(BspError::BadMagic { path: name.to_string() }); }
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != VERSION {
            return Err(BspError::BadVersion { path: name.to_string(), found: version, expected: VERSION });
        }

        let dir_bytes = &bytes[8..8 + LUMP_COUNT * 16];
        let dir: &[LumpDir] = cast_slice(dir_bytes);
        let revision = u32::from_le_bytes(
            bytes[8 + LUMP_COUNT * 16..8 + LUMP_COUNT * 16 + 4].try_into().unwrap(),
        );

        let slice = |i: usize| -> Result<&[u8]> {
            let d = dir[i];
            let (offset, end) = (d.offset as u64, d.offset as u64 + d.length as u64);
            if end > bytes.len() as u64 {
                return Err(BspError::LumpOutOfRange {
                    path: name.to_string(),
                    lump: i,
                    name: lumps::NAMES[i],
                    offset,
                    end,
                    size: bytes.len() as u64,
                });
            }
            Ok(&bytes[offset as usize..end as usize])
        };

        let records = |i: usize| -> Result<Vec<u8>> { Ok(slice(i)?.to_vec()) };

        let entities_raw = records(lumps::ENTITIES)?;
        let entities = String::from_utf8(entities_raw)
            .map_err(|_| BspError::EntitiesNotUtf8 { path: name.to_string() })?;

        let bsp = Bsp {
            revision,
            entities,
            planes: read_lump(slice(lumps::PLANES)?, name, lumps::NAMES[lumps::PLANES])?,
            vertices: read_lump(slice(lumps::VERTICES)?, name, lumps::NAMES[lumps::VERTICES])?,
            edges: read_lump(slice(lumps::EDGES)?, name, lumps::NAMES[lumps::EDGES])?,
            surfedges: read_lump(slice(lumps::SURFEDGES)?, name, lumps::NAMES[lumps::SURFEDGES])?,
            faces: read_lump(slice(lumps::FACES)?, name, lumps::NAMES[lumps::FACES])?,
            nodes: read_lump(slice(lumps::NODES)?, name, lumps::NAMES[lumps::NODES])?,
            leaves: read_lump(slice(lumps::LEAVES)?, name, lumps::NAMES[lumps::LEAVES])?,
            leaffaces: read_lump(slice(lumps::LEAFFACES)?, name, lumps::NAMES[lumps::LEAFFACES])?,
            leafbrushes: read_lump(slice(lumps::LEAFBRUSHES)?, name, lumps::NAMES[lumps::LEAFBRUSHES])?,
            models: read_lump(slice(lumps::MODELS)?, name, lumps::NAMES[lumps::MODELS])?,
            brushes: read_lump(slice(lumps::BRUSHES)?, name, lumps::NAMES[lumps::BRUSHES])?,
            brushsides: read_lump(slice(lumps::BRUSHSIDES)?, name, lumps::NAMES[lumps::BRUSHSIDES])?,
            texinfo: read_lump(slice(lumps::TEXINFO)?, name, lumps::NAMES[lumps::TEXINFO])?,
            texdata: read_lump(slice(lumps::TEXDATA)?, name, lumps::NAMES[lumps::TEXDATA])?,
            texdata_strings: slice(lumps::TEXDATA_STRINGS)?.to_vec(),
            visibility: slice(lumps::VISIBILITY)?.to_vec(),
            lighting: read_lump(slice(lumps::LIGHTING)?, name, lumps::NAMES[lumps::LIGHTING])?,
        };

        bsp.validate().map_err(|detail| BspError::Invalid { path: name.to_string(), detail })?;
        Ok(bsp)
    }

    pub fn save(&self, path: &Path) -> Result<u64> {
        let bytes = self.to_bytes();
        std::fs::write(path, &bytes).map_err(|source| BspError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Ok(bytes.len() as u64)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut dir = [LumpDir::default(); LUMP_COUNT];
        let mut body: Vec<u8> = Vec::new();

        let mut push = |dir: &mut [LumpDir; LUMP_COUNT], index: usize, data: &[u8]| {
            // Every lump starts 4-byte aligned so a zero-copy reader can cast
            // directly out of a memory-mapped file.
            while (HEADER_SIZE + body.len()) % 4 != 0 { body.push(0); }
            dir[index] = LumpDir {
                offset: (HEADER_SIZE + body.len()) as u32,
                length: data.len() as u32,
                version: 0,
                ident: [0; 4],
            };
            body.extend_from_slice(data);
        };

        push(&mut dir, lumps::ENTITIES, self.entities.as_bytes());
        push(&mut dir, lumps::PLANES, cast_slice(&self.planes));
        push(&mut dir, lumps::VERTICES, cast_slice(&self.vertices));
        push(&mut dir, lumps::EDGES, cast_slice(&self.edges));
        push(&mut dir, lumps::SURFEDGES, cast_slice(&self.surfedges));
        push(&mut dir, lumps::FACES, cast_slice(&self.faces));
        push(&mut dir, lumps::NODES, cast_slice(&self.nodes));
        push(&mut dir, lumps::LEAVES, cast_slice(&self.leaves));
        push(&mut dir, lumps::LEAFFACES, cast_slice(&self.leaffaces));
        push(&mut dir, lumps::LEAFBRUSHES, cast_slice(&self.leafbrushes));
        push(&mut dir, lumps::MODELS, cast_slice(&self.models));
        push(&mut dir, lumps::BRUSHES, cast_slice(&self.brushes));
        push(&mut dir, lumps::BRUSHSIDES, cast_slice(&self.brushsides));
        push(&mut dir, lumps::TEXINFO, cast_slice(&self.texinfo));
        push(&mut dir, lumps::TEXDATA, cast_slice(&self.texdata));
        push(&mut dir, lumps::TEXDATA_STRINGS, &self.texdata_strings);
        push(&mut dir, lumps::VISIBILITY, &self.visibility);
        push(&mut dir, lumps::LIGHTING, cast_slice(&self.lighting));

        let mut out = Vec::with_capacity(HEADER_SIZE + body.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(cast_slice(&dir));
        out.extend_from_slice(&self.revision.to_le_bytes());
        debug_assert_eq!(out.len(), HEADER_SIZE);
        out.extend_from_slice(&body);
        out
    }
}

/// Cast a lump's bytes into a record array, checking the length divides evenly.
fn read_lump<T: Pod>(bytes: &[u8], path: &str, name: &'static str) -> Result<Vec<T>> {
    let record = std::mem::size_of::<T>();
    if bytes.len() % record != 0 {
        return Err(BspError::LumpMisaligned {
            path: path.to_string(),
            name,
            size: bytes.len(),
            record,
        });
    }
    // `cast_slice` needs the source to be aligned for T; a lump read out of a
    // Vec<u8> may not be, so copy through an aligned buffer rather than risk it.
    let count = bytes.len() / record;
    let mut out: Vec<T> = vec![T::zeroed(); count];
    bytemuck::cast_slice_mut::<T, u8>(&mut out).copy_from_slice(bytes);
    Ok(out)
}

/// Write a `.kerobsp` and report its size, creating parent directories.
pub fn write_bsp(bsp: &Bsp, path: &Path) -> Result<u64> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| BspError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let bytes = bsp.to_bytes();
    let mut f = std::fs::File::create(path).map_err(|source| BspError::Io {
        path: path.display().to_string(),
        source,
    })?;
    f.write_all(&bytes).map_err(|source| BspError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(bytes.len() as u64)
}
