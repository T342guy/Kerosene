// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Which streamed sections of the map are loaded.
//!
//! A section is a set of world brushes the designer put in a streamed
//! visgroup; Cleave tagged their faces and brushes with it. The BSP tree,
//! the entities and the player's traces are whole whatever is loaded --
//! the tree is small and the traces have to work everywhere -- and what
//! streams is the expensive part: the render mesh, the lightmap atlas and
//! the rigid-body hulls of each section.
//!
//! The rule is potential visibility. A section is wanted when any of its
//! faces sits in a cluster the player's cluster can *hear* -- the PAS,
//! which is the PVS flooded one doorway further -- so a room starts loading
//! a doorway before it can be seen, and that doorway is the margin that
//! hides the build. A section that stops being wanted lingers a few
//! seconds before it is dropped, so pacing back and forth over a threshold
//! does not thrash. A physics prop that is awake inside a section keeps it
//! loaded: a crate that has just been thrown into the next room must not
//! fall through a floor that is no longer there. With no vis data, or with
//! the player outside the world, everything is wanted; `sv_stream 0` wants
//! everything always.
//!
//! This module only decides. The host builds and drops the GPU data and
//! reports back with [`Streaming::mark_loaded`]; the physics adds and
//! removes hulls when it sees the state change.

use kerosene_bsp::{Bsp, VisData, VisKind};
use kerosene_math::{Aabb, Vec3};

/// Where one section is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionState {
    /// Not resident. Nothing of it is drawn or collided with by props.
    Unloaded,
    /// Should be resident; the host is building it.
    Wanted,
    /// Resident.
    Loaded,
}

/// The streaming state of a level.
#[derive(Clone, Debug)]
pub struct Streaming {
    /// Per section, a bit per cluster that holds any of its faces.
    masks: Vec<Vec<u8>>,
    bounds: Vec<Aabb>,
    state: Vec<SectionState>,
    /// Seconds left before an unwanted loaded section is dropped.
    linger: Vec<f32>,
    /// Bumped whenever any state changes, so a watcher can compare rather
    /// than diff the whole list every frame.
    revision: u64,
    /// The last cluster the decision was made for.
    cluster: i16,
    /// Scratch for the decompressed PAS row.
    row: Vec<u8>,
}

impl Streaming {
    pub fn new(bsp: &Bsp) -> Streaming {
        let count = bsp.section_count();
        let mut state = vec![SectionState::Unloaded; count];
        // The world is loaded by definition: the host builds it with the map.
        state[0] = SectionState::Loaded;
        Streaming {
            masks: bsp.section_cluster_masks(),
            bounds: (0..count)
                .map(|i| bsp.sections.get(i).map_or(Aabb::EMPTY, |s| s.bounds))
                .collect(),
            state,
            linger: vec![0.0; count],
            revision: 0,
            cluster: -2,
            row: Vec::new(),
        }
    }

    pub fn section_count(&self) -> usize {
        self.state.len()
    }

    /// Whether anything can stream at all: a map with only the world
    /// section has nothing to load or unload.
    pub fn is_static(&self) -> bool {
        self.state.len() <= 1
    }

    pub fn state(&self, section: usize) -> SectionState {
        self.state
            .get(section)
            .copied()
            .unwrap_or(SectionState::Unloaded)
    }

    pub fn states(&self) -> &[SectionState] {
        &self.state
    }

    pub fn is_loaded(&self, section: usize) -> bool {
        self.state(section) == SectionState::Loaded
    }

    /// Loaded or on the way: what physics keeps hulls for.
    pub fn is_resident(&self, section: usize) -> bool {
        matches!(
            self.state(section),
            SectionState::Loaded | SectionState::Wanted
        )
    }

    pub fn bounds(&self, section: usize) -> Aabb {
        self.bounds.get(section).copied().unwrap_or(Aabb::EMPTY)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The host finished building a section. Ignored unless it is still
    /// wanted: a build that finishes after the section was dropped again
    /// is thrown away rather than resurrected.
    pub fn mark_loaded(&mut self, section: usize) -> bool {
        if self.state(section) == SectionState::Wanted {
            self.state[section] = SectionState::Loaded;
            self.revision += 1;
            return true;
        }
        false
    }

    /// Decide, once per tick. Returns whether anything changed.
    ///
    /// `cluster` is the player's; `keep_alive` are points that hold their
    /// section loaded (awake props); `enabled` false wants everything.
    pub fn update(
        &mut self,
        bsp: &Bsp,
        cluster: i16,
        keep_alive: &[Vec3],
        dt: f32,
        enabled: bool,
        linger: f32,
    ) -> bool {
        if self.is_static() {
            return false;
        }
        let before = self.revision;
        let vis = VisData::new(&bsp.visibility);
        let everything = !enabled || cluster < 0 || vis.is_none();
        let mut wanted = vec![everything; self.state.len()];
        wanted[0] = true;

        if !everything && let Some(vis) = vis {
            if cluster != self.cluster || self.row.is_empty() {
                vis.decompress_into(cluster as usize, VisKind::Pas, &mut self.row);
                self.cluster = cluster;
            }
            for (s, mask) in self.masks.iter().enumerate().skip(1) {
                wanted[s] = mask.iter().zip(self.row.iter()).any(|(m, r)| m & r != 0);
            }
            for &p in keep_alive {
                for (s, b) in self.bounds.iter().enumerate().skip(1) {
                    if !b.is_empty() && b.expanded(64.0).contains_point(p) {
                        wanted[s] = true;
                    }
                }
            }
        }

        for (s, &want) in wanted.iter().enumerate().skip(1) {
            match (self.state[s], want) {
                (SectionState::Unloaded, true) => {
                    self.state[s] = SectionState::Wanted;
                    self.linger[s] = linger;
                    self.revision += 1;
                }
                (SectionState::Wanted, false) => {
                    // Never arrived; nothing to linger.
                    self.state[s] = SectionState::Unloaded;
                    self.revision += 1;
                }
                (SectionState::Loaded, true) | (SectionState::Wanted, true) => {
                    self.linger[s] = linger;
                }
                (SectionState::Loaded, false) => {
                    self.linger[s] -= dt;
                    if self.linger[s] <= 0.0 {
                        self.state[s] = SectionState::Unloaded;
                        self.revision += 1;
                    }
                }
                (SectionState::Unloaded, false) => {}
            }
        }
        self.revision != before
    }

    /// `n loaded, m wanted of k`, for the console.
    pub fn summary(&self) -> String {
        let loaded = self
            .state
            .iter()
            .filter(|&&s| s == SectionState::Loaded)
            .count();
        let wanted = self
            .state
            .iter()
            .filter(|&&s| s == SectionState::Wanted)
            .count();
        format!(
            "{loaded} loaded, {wanted} loading, of {} sections",
            self.state.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake: three sections over four clusters, with a PAS where cluster
    /// 0 hears 0 and 1, cluster 2 hears 2 and 3.
    fn streaming() -> (Streaming, Bsp) {
        let mut bsp = Bsp::new();
        // Vis lump: 4 clusters, per cluster [pvs_offset, pas_offset], rows
        // of one byte, uncompressed (no zero bytes to run-length).
        let clusters = 4u32;
        let mut raw = Vec::new();
        raw.extend_from_slice(&clusters.to_le_bytes());
        let table = 4 + clusters as usize * 8;
        let rows: [u8; 4] = [0b0011, 0b0011, 0b1100, 0b1100];
        for c in 0..clusters as usize {
            let off = (table + c) as u32;
            raw.extend_from_slice(&off.to_le_bytes()); // pvs
            raw.extend_from_slice(&off.to_le_bytes()); // pas, the same here
        }
        raw.extend_from_slice(&rows);
        bsp.visibility = raw;
        let mut s = Streaming {
            masks: vec![vec![0b1111], vec![0b0010], vec![0b1000]],
            bounds: vec![
                Aabb::EMPTY,
                Aabb::new(Vec3::ZERO, Vec3::splat(64.0)),
                Aabb::new(Vec3::splat(512.0), Vec3::splat(576.0)),
            ],
            state: vec![SectionState::Unloaded; 3],
            linger: vec![0.0; 3],
            revision: 0,
            cluster: -2,
            row: Vec::new(),
        };
        s.state[0] = SectionState::Loaded;
        (s, bsp)
    }

    #[test]
    fn a_section_is_wanted_when_the_player_can_hear_its_cluster() {
        let (mut s, bsp) = streaming();
        assert!(s.update(&bsp, 0, &[], 0.1, true, 3.0));
        assert_eq!(
            s.state(1),
            SectionState::Wanted,
            "cluster 1 is audible from 0"
        );
        assert_eq!(s.state(2), SectionState::Unloaded);
        assert!(s.mark_loaded(1));
        assert!(s.is_loaded(1));
        assert!(!s.mark_loaded(2), "not wanted, so a late build is refused");
    }

    #[test]
    fn leaving_lingers_and_then_unloads() {
        let (mut s, bsp) = streaming();
        s.update(&bsp, 0, &[], 0.1, true, 1.0);
        s.mark_loaded(1);
        // Walk to the far end.
        s.update(&bsp, 2, &[], 0.1, true, 1.0);
        assert_eq!(s.state(1), SectionState::Loaded, "still lingering");
        assert_eq!(s.state(2), SectionState::Wanted);
        s.update(&bsp, 2, &[], 0.5, true, 1.0);
        assert_eq!(s.state(1), SectionState::Loaded);
        s.update(&bsp, 2, &[], 0.6, true, 1.0);
        assert_eq!(s.state(1), SectionState::Unloaded, "linger ran out");
    }

    #[test]
    fn an_awake_prop_holds_its_section() {
        let (mut s, bsp) = streaming();
        s.update(&bsp, 0, &[], 0.1, true, 0.5);
        s.mark_loaded(1);
        let crate_in_room_one = [Vec3::splat(32.0)];
        for _ in 0..10 {
            s.update(&bsp, 2, &crate_in_room_one, 0.5, true, 0.5);
        }
        assert_eq!(s.state(1), SectionState::Loaded);
        for _ in 0..10 {
            s.update(&bsp, 2, &[], 0.5, true, 0.5);
        }
        assert_eq!(s.state(1), SectionState::Unloaded);
    }

    #[test]
    fn streaming_off_or_outside_the_world_wants_everything() {
        let (mut s, bsp) = streaming();
        s.update(&bsp, 0, &[], 0.1, false, 3.0);
        assert_eq!(s.state(2), SectionState::Wanted);
        let (mut s, bsp) = streaming();
        s.update(&bsp, -1, &[], 0.1, true, 3.0);
        assert_eq!(s.state(2), SectionState::Wanted);
    }

    #[test]
    fn a_single_section_map_never_changes() {
        let bsp = Bsp::new();
        let mut s = Streaming::new(&bsp);
        assert!(s.is_static());
        assert!(!s.update(&bsp, 0, &[], 0.1, true, 3.0));
        assert!(s.is_loaded(0));
    }
}
