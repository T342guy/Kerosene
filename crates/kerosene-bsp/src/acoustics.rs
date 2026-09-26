// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What the map sounds like: the acoustics lumps.
//!
//! Resonance probes every leaf with rays, works out how long sound lingers
//! there and how open it is, and gathers leaves that sound alike into
//! *rooms*. Two lumps carry the result. [`lumps::ACOUSTICS`] holds one
//! [`AcousticRoom`] per room behind an [`AcousticHeader`]; it took one of the
//! two slots the directory kept spare, so the file format's version did not
//! move. [`lumps::ACOUSTIC_LEAFS`] is one `u16` per leaf saying which room it
//! is in, [`NO_ROOM`] for a solid leaf or one nothing could probe.
//!
//! A room's record is everything the mixer's reverb needs, in the units the
//! reverb takes, so the engine's job at runtime is a leaf lookup and a copy.
//! The record also keeps what it was derived from -- absorption per band,
//! the mean free path -- because a designer asking "why does this corridor
//! ring" wants the number, not the conclusion.
//!
//! [`lumps::ACOUSTICS`]: crate::lumps::ACOUSTICS
//! [`lumps::ACOUSTIC_LEAFS`]: crate::lumps::ACOUSTIC_LEAFS

use bytemuck::{Pod, Zeroable, cast_slice};

/// The first four bytes of the acoustics lump.
pub const MAGIC: [u8; 4] = *b"ACST";

/// The layout version, stored in the lump directory entry's `version`.
pub const VERSION: u32 = 1;

/// The bands every per-band figure is given in, in hertz. Written into the
/// header so a reader that expects different ones can say so.
pub const BANDS_HZ: [u32; 4] = [125, 500, 2000, 8000];

/// The leaf-to-room value for a leaf that is in no room: solid, or too small
/// or awkward to probe and with no neighbour to inherit from.
pub const NO_ROOM: u16 = u16::MAX;

/// The most rooms a map may have, since the leaf index is a `u16` with one
/// value spoken for.
pub const MAX_ROOMS: usize = NO_ROOM as usize;

/// Flags on a room.
pub mod room_flags {
    /// Under water. Never merged with a dry room.
    pub const WATER: u32 = 1 << 0;
    /// Open to the sky: enough rays escaped that this is outdoors.
    pub const OUTDOOR: u32 = 1 << 1;
    /// A designer's `env_acoustic_override` set these numbers, not the probe.
    pub const OVERRIDE: u32 = 1 << 2;
    /// Every leaf in it was too small to probe and took a neighbour's figures.
    pub const INHERITED: u32 = 1 << 3;
}

/// The head of the acoustics lump.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct AcousticHeader {
    pub magic: [u8; 4],
    pub bands_hz: [u32; 4],
    pub room_count: u32,
    pub path_count: u32,
    pub _pad: u32,
}

/// One room: a set of leaves that sound alike, and how they sound.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct AcousticRoom {
    /// Seconds for each band to fall 60 dB.
    pub rt60: [f32; 4],
    /// Average absorption of the surfaces sound met, per band, 0 to 1. What
    /// the decay was derived from; kept for the designer.
    pub absorption: [f32; 4],
    /// Average distance a ray travelled between surfaces, in units. Small in
    /// a corridor, large in a hangar.
    pub mean_free_path: f32,
    /// Seconds before the first reflection returns.
    pub predelay: f32,
    /// The share of rays that escaped to the sky, 0 to 1.
    pub openness: f32,
    /// How loud the room is next to the sound itself, 0 to 1.
    pub wet: f32,
    /// How evenly the reflections are spread, 0 to 1.
    pub diffusion: f32,
    /// Centre and radius of the sphere around every leaf in the room.
    pub centre: [f32; 3],
    pub radius: f32,
    /// Total volume of the room's leaves, in cubic units.
    pub volume: f32,
    /// [`room_flags`].
    pub flags: u32,
    pub leaf_count: u32,
}

impl AcousticRoom {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

/// How sound gets from one room to another that it cannot see: the length
/// of the shortest way through portals and how many rooms it crosses.
///
/// Optional -- the header says how many there are, and none is fine.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct AcousticPath {
    pub a: u16,
    pub b: u16,
    /// In quarter-units, so a `u16` reaches a quarter of a mile.
    pub path_len: u16,
    pub hops: u8,
    pub _pad: u8,
}

/// Both acoustics lumps, decoded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Acoustics {
    pub rooms: Vec<AcousticRoom>,
    /// One per leaf: an index into `rooms`, or [`NO_ROOM`].
    pub leaf_room: Vec<u16>,
    pub paths: Vec<AcousticPath>,
}

impl Acoustics {
    /// Decode the two lumps. An empty acoustics lump is a map Resonance has
    /// not run on, and is `None` rather than an error, the same way an empty
    /// visibility lump is a map without vis.
    pub fn parse(lump: &[u8], leaf_lump: &[u8]) -> Result<Option<Acoustics>, String> {
        if lump.is_empty() {
            return Ok(None);
        }
        let header_size = std::mem::size_of::<AcousticHeader>();
        if lump.len() < header_size {
            return Err(format!(
                "acoustics lump is {} bytes, shorter than its header",
                lump.len()
            ));
        }
        let mut header = AcousticHeader::zeroed();
        bytemuck::bytes_of_mut(&mut header).copy_from_slice(&lump[..header_size]);
        if header.magic != MAGIC {
            return Err("acoustics lump does not start with ACST".to_string());
        }
        if header.bands_hz != BANDS_HZ {
            return Err(format!(
                "acoustics lump is in bands {:?}, not {:?}",
                header.bands_hz, BANDS_HZ
            ));
        }
        let rooms_bytes = header.room_count as usize * std::mem::size_of::<AcousticRoom>();
        let paths_bytes = header.path_count as usize * std::mem::size_of::<AcousticPath>();
        if lump.len() != header_size + rooms_bytes + paths_bytes {
            return Err(format!(
                "acoustics lump is {} bytes; {} rooms and {} paths need {}",
                lump.len(),
                header.room_count,
                header.path_count,
                header_size + rooms_bytes + paths_bytes
            ));
        }
        let rooms = copy_records(&lump[header_size..header_size + rooms_bytes]);
        let paths = copy_records(&lump[header_size + rooms_bytes..]);
        if !leaf_lump.len().is_multiple_of(2) {
            return Err(format!(
                "acoustic leaf lump is {} bytes, not a whole number of u16",
                leaf_lump.len()
            ));
        }
        let leaf_room = copy_records(leaf_lump);
        Ok(Some(Acoustics {
            rooms,
            leaf_room,
            paths,
        }))
    }

    /// The two lumps' bytes: the acoustics lump, then the leaf lump.
    pub fn encode(&self) -> (Vec<u8>, Vec<u8>) {
        let header = AcousticHeader {
            magic: MAGIC,
            bands_hz: BANDS_HZ,
            room_count: self.rooms.len() as u32,
            path_count: self.paths.len() as u32,
            _pad: 0,
        };
        let mut lump = Vec::with_capacity(
            std::mem::size_of::<AcousticHeader>()
                + self.rooms.len() * std::mem::size_of::<AcousticRoom>()
                + self.paths.len() * std::mem::size_of::<AcousticPath>(),
        );
        lump.extend_from_slice(bytemuck::bytes_of(&header));
        lump.extend_from_slice(cast_slice(&self.rooms));
        lump.extend_from_slice(cast_slice(&self.paths));
        (lump, cast_slice(&self.leaf_room).to_vec())
    }

    /// Check every index against the map it belongs to.
    pub fn validate(&self, leaf_count: usize) -> Result<(), String> {
        if self.rooms.len() > MAX_ROOMS {
            return Err(format!(
                "{} acoustic rooms, more than fit in a u16",
                self.rooms.len()
            ));
        }
        if self.leaf_room.len() != leaf_count {
            return Err(format!(
                "acoustic leaf lump has {} entries for {} leaves",
                self.leaf_room.len(),
                leaf_count
            ));
        }
        for (leaf, &room) in self.leaf_room.iter().enumerate() {
            if room != NO_ROOM && room as usize >= self.rooms.len() {
                return Err(format!(
                    "leaf {leaf} is in acoustic room {room} of {}",
                    self.rooms.len()
                ));
            }
        }
        for (i, p) in self.paths.iter().enumerate() {
            if p.a as usize >= self.rooms.len() || p.b as usize >= self.rooms.len() {
                return Err(format!(
                    "acoustic path {i} joins rooms {} and {} of {}",
                    p.a,
                    p.b,
                    self.rooms.len()
                ));
            }
        }
        for (i, r) in self.rooms.iter().enumerate() {
            let finite = r.rt60.iter().all(|t| t.is_finite() && *t > 0.0)
                && r.predelay.is_finite()
                && r.wet.is_finite()
                && r.diffusion.is_finite();
            if !finite {
                return Err(format!("acoustic room {i} holds a non-finite figure"));
            }
        }
        Ok(())
    }

    /// The room a leaf is in.
    pub fn room_of_leaf(&self, leaf: usize) -> Option<(u16, &AcousticRoom)> {
        let index = *self.leaf_room.get(leaf)?;
        if index == NO_ROOM {
            return None;
        }
        self.rooms.get(index as usize).map(|r| (index, r))
    }
}

/// Copy records out of bytes that make no alignment promise.
fn copy_records<T: Pod>(bytes: &[u8]) -> Vec<T> {
    let record = std::mem::size_of::<T>();
    let count = bytes.len() / record;
    let mut out: Vec<T> = vec![T::zeroed(); count];
    bytemuck::cast_slice_mut::<T, u8>(&mut out).copy_from_slice(&bytes[..count * record]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Acoustics {
        Acoustics {
            rooms: vec![
                AcousticRoom {
                    rt60: [1.2, 1.0, 0.8, 0.5],
                    absorption: [0.05, 0.06, 0.1, 0.2],
                    mean_free_path: 300.0,
                    predelay: 0.02,
                    openness: 0.0,
                    wet: 0.4,
                    diffusion: 0.6,
                    centre: [0.0, 0.0, 64.0],
                    radius: 400.0,
                    volume: 1e7,
                    flags: 0,
                    leaf_count: 3,
                },
                AcousticRoom {
                    rt60: [0.3, 0.25, 0.2, 0.1],
                    absorption: [0.5; 4],
                    mean_free_path: 2000.0,
                    predelay: 0.06,
                    openness: 0.8,
                    wet: 0.1,
                    diffusion: 0.2,
                    centre: [1000.0, 0.0, 64.0],
                    radius: 900.0,
                    volume: 4e8,
                    flags: room_flags::OUTDOOR,
                    leaf_count: 1,
                },
            ],
            leaf_room: vec![NO_ROOM, 0, 0, 0, 1],
            paths: vec![AcousticPath {
                a: 0,
                b: 1,
                path_len: 250,
                hops: 1,
                _pad: 0,
            }],
        }
    }

    #[test]
    fn record_sizes_are_pinned() {
        assert_eq!(std::mem::size_of::<AcousticHeader>(), 32);
        assert_eq!(std::mem::size_of::<AcousticRoom>(), 80);
        assert_eq!(std::mem::size_of::<AcousticPath>(), 8);
    }

    #[test]
    fn round_trips() {
        let a = sample();
        let (lump, leaves) = a.encode();
        assert_eq!(&lump[..4], b"ACST");
        let back = Acoustics::parse(&lump, &leaves).unwrap().unwrap();
        assert_eq!(back, a);
        assert!(back.validate(5).is_ok());
        assert_eq!(back.room_of_leaf(4).unwrap().0, 1);
        assert!(back.room_of_leaf(0).is_none());
        assert!(back.room_of_leaf(99).is_none());
    }

    #[test]
    fn an_empty_lump_is_no_acoustics_rather_than_an_error() {
        assert_eq!(Acoustics::parse(&[], &[]).unwrap(), None);
    }

    #[test]
    fn a_damaged_lump_is_refused_by_name() {
        let (mut lump, leaves) = sample().encode();
        lump[0] = b'X';
        assert!(
            Acoustics::parse(&lump, &leaves)
                .unwrap_err()
                .contains("ACST")
        );
        let (lump, leaves) = sample().encode();
        assert!(
            Acoustics::parse(&lump[..lump.len() - 8], &leaves)
                .unwrap_err()
                .contains("rooms")
        );
        assert!(Acoustics::parse(&lump[..8], &leaves).is_err());
        assert!(Acoustics::parse(&lump, &leaves[1..]).is_err());
    }

    #[test]
    fn validation_catches_a_leaf_in_a_room_that_is_not_there() {
        let mut a = sample();
        assert!(a.validate(4).is_err(), "wrong leaf count");
        a.leaf_room[1] = 7;
        assert!(a.validate(5).unwrap_err().contains("leaf 1"));
        let mut a = sample();
        a.rooms[0].rt60[2] = f32::NAN;
        assert!(a.validate(5).unwrap_err().contains("room 0"));
        let mut a = sample();
        a.paths[0].b = 9;
        assert!(a.validate(5).unwrap_err().contains("path 0"));
    }
}
