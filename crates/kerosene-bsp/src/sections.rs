// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Sections: the parts of a map the engine loads and unloads around the
//! player.
//!
//! A section is a visgroup the designer marked as streamed. Cleave tags
//! every world face and every brush with the section it belongs to --
//! `0` for everything unassigned, which is the always-loaded world -- and
//! writes three lumps: [`lumps::SECTIONS`] with a name and bounds per
//! section, [`lumps::FACE_SECTIONS`] with one `u16` per face, and
//! [`lumps::BRUSH_SECTIONS`] with one per brush. The BSP tree, the entities
//! and the traces stay whole; what a section lets the engine drop is the
//! render mesh, the lightmaps and the rigid-body hulls of a part of the
//! map the player cannot see.
//!
//! The lumps are what a compile writes even when the map has no streamed
//! visgroups: one section, every face in it. An older file with the lumps
//! missing reads the same way, so nothing has to special-case a map from
//! before sections existed.
//!
//! [`lumps::SECTIONS`]: crate::lumps::SECTIONS
//! [`lumps::FACE_SECTIONS`]: crate::lumps::FACE_SECTIONS
//! [`lumps::BRUSH_SECTIONS`]: crate::lumps::BRUSH_SECTIONS

use bytemuck::{Pod, Zeroable, cast_slice};
use kerosene_math::{Aabb, Vec3};

/// The name of the section everything unassigned is in.
pub const WORLD_SECTION: &str = "world";

/// The most sections a map may have: the per-face index is a `u16`.
pub const MAX_SECTIONS: usize = u16::MAX as usize;

/// One record of the sections lump.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SectionRecord {
    /// Into the string table that follows the records.
    pub name_offset: u32,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// Reserved.
    pub flags: u32,
}

/// A section, as the engine sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    /// The bounds of every brush in it; empty for a section with none.
    pub bounds: Aabb,
}

impl Section {
    /// The section everything unassigned is in.
    pub fn world() -> Section {
        Section {
            name: WORLD_SECTION.to_string(),
            bounds: Aabb::EMPTY,
        }
    }
}

/// Encode the sections lump: a count, the records, then the names. An
/// unnamed list is written as the one-section world.
pub fn encode(sections: &[Section]) -> Vec<u8> {
    if sections.is_empty() {
        return encode(&[Section::world()]);
    }
    let mut names: Vec<u8> = Vec::new();
    let mut records: Vec<SectionRecord> = Vec::with_capacity(sections.len());
    for section in sections {
        let name_offset = names.len() as u32;
        names.extend_from_slice(section.name.as_bytes());
        names.push(0);
        let b = if section.bounds.is_empty() {
            Aabb::new(Vec3::ZERO, Vec3::ZERO)
        } else {
            section.bounds
        };
        records.push(SectionRecord {
            name_offset,
            mins: b.min.to_array(),
            maxs: b.max.to_array(),
            flags: 0,
        });
    }
    let mut out = Vec::new();
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    out.extend_from_slice(cast_slice(&records));
    out.extend_from_slice(&names);
    out
}

/// Decode the sections lump. An empty lump is the one-section world.
pub fn parse(lump: &[u8]) -> Result<Vec<Section>, String> {
    if lump.is_empty() {
        return Ok(vec![Section::world()]);
    }
    if lump.len() < 4 {
        return Err("sections lump is shorter than its count".into());
    }
    let count = u32::from_le_bytes(lump[0..4].try_into().unwrap()) as usize;
    if count == 0 || count > MAX_SECTIONS {
        return Err(format!("sections lump names {count} sections"));
    }
    let record = std::mem::size_of::<SectionRecord>();
    let table_end = 4 + count * record;
    if lump.len() < table_end {
        return Err(format!(
            "sections lump holds {} bytes, fewer than its {count} records need",
            lump.len()
        ));
    }
    let mut records: Vec<SectionRecord> = vec![SectionRecord::zeroed(); count];
    bytemuck::cast_slice_mut::<SectionRecord, u8>(&mut records)
        .copy_from_slice(&lump[4..table_end]);
    let names = &lump[table_end..];
    let mut sections = Vec::with_capacity(count);
    for (i, r) in records.iter().enumerate() {
        let offset = r.name_offset as usize;
        if offset >= names.len() {
            return Err(format!("section {i} names a string past the table"));
        }
        let rest = &names[offset..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let name = std::str::from_utf8(&rest[..end])
            .map_err(|_| format!("section {i} has a name that is not UTF-8"))?
            .to_string();
        let bounds = Aabb::new(Vec3::from_array(r.mins), Vec3::from_array(r.maxs));
        let bounds = if bounds.min == Vec3::ZERO && bounds.max == Vec3::ZERO {
            Aabb::EMPTY
        } else {
            bounds
        };
        sections.push(Section { name, bounds });
    }
    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_round_trip() {
        let sections = vec![
            Section::world(),
            Section {
                name: "Cave".into(),
                bounds: Aabb::new(Vec3::splat(-64.0), Vec3::splat(64.0)),
            },
            Section {
                name: "Pool house".into(),
                bounds: Aabb::EMPTY,
            },
        ];
        let bytes = encode(&sections);
        assert_eq!(parse(&bytes).unwrap(), sections);
        assert_eq!(parse(&[]).unwrap(), vec![Section::world()]);
    }

    #[test]
    fn a_truncated_lump_is_refused() {
        let bytes = encode(&[Section::world(), Section::world()]);
        assert!(parse(&bytes[..10]).is_err());
    }
}
