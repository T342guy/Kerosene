// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! The Potentially Visible Set.
//!
//! For every cluster, the PVS records which other clusters can possibly be
//! seen from anywhere inside it. At runtime the renderer finds the viewer's
//! cluster, decompresses its row, and skips every leaf whose cluster is not
//! set -- turning "draw the world and let the depth buffer sort it out" into
//! "draw the two rooms you can actually see".
//!
//! Rows are bit-vectors, one bit per cluster, run-length encoded on zero bytes.
//! That encoding is nearly free to decode and is enormously effective here,
//! because a typical row is mostly zeroes: from any given room, most of the
//! map is not visible. The scheme is Quake's, carried through Source unchanged.
//!
//! A second set of rows, the PAS (Potentially *Audible* Set), is the PVS
//! flooded one extra step: sound goes around a corner where sight does not.

use std::io::Write;

/// Bytes needed for a bit per cluster.
#[inline]
pub const fn row_bytes(num_clusters: usize) -> usize { num_clusters.div_ceil(8) }

/// Reader over a compiled visibility lump.
pub struct VisData<'a> {
    num_clusters: usize,
    /// `[pvs_offset, pas_offset]` per cluster, into `raw`.
    offsets: &'a [u8],
    raw: &'a [u8],
}

/// Which set to query.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisKind {
    /// What can be seen.
    Pvs,
    /// What can be heard -- the PVS flooded one extra cluster.
    Pas,
}

impl<'a> VisData<'a> {
    /// Wrap a visibility lump. Returns `None` for an empty or malformed lump,
    /// which callers should treat as "everything is visible" -- an
    /// uncompiled map must still render, just slowly.
    pub fn new(raw: &'a [u8]) -> Option<VisData<'a>> {
        if raw.len() < 4 { return None; }
        let num_clusters = u32::from_le_bytes(raw[0..4].try_into().ok()?) as usize;
        if num_clusters == 0 { return None; }
        let table_end = 4 + num_clusters * 8;
        if raw.len() < table_end { return None; }
        Some(VisData { num_clusters, offsets: &raw[4..table_end], raw })
    }

    pub fn num_clusters(&self) -> usize { self.num_clusters }

    fn offset_of(&self, cluster: usize, kind: VisKind) -> Option<usize> {
        if cluster >= self.num_clusters { return None; }
        let base = cluster * 8 + if kind == VisKind::Pas { 4 } else { 0 };
        let off = u32::from_le_bytes(self.offsets[base..base + 4].try_into().ok()?) as usize;
        (off < self.raw.len()).then_some(off)
    }

    /// Decompress one cluster's row into `out`, which is resized to fit.
    pub fn decompress_into(&self, cluster: usize, kind: VisKind, out: &mut Vec<u8>) -> bool {
        let bytes = row_bytes(self.num_clusters);
        out.clear();
        out.resize(bytes, 0);
        let Some(offset) = self.offset_of(cluster, kind) else { return false };
        decompress_row(&self.raw[offset..], out);
        true
    }

    pub fn decompress(&self, cluster: usize, kind: VisKind) -> Vec<u8> {
        let mut out = Vec::new();
        self.decompress_into(cluster, kind, &mut out);
        out
    }

    /// Whether `to` is in `from`'s set.
    ///
    /// Decompresses the whole row, so callers that test many targets from one
    /// viewpoint should decompress once and use [`row_test`] instead.
    pub fn is_visible(&self, from: usize, to: usize, kind: VisKind) -> bool {
        if from >= self.num_clusters || to >= self.num_clusters { return false; }
        let row = self.decompress(from, kind);
        row_test(&row, to)
    }
}

/// Test a bit in a decompressed row.
#[inline]
pub fn row_test(row: &[u8], cluster: usize) -> bool {
    row.get(cluster >> 3).is_some_and(|b| b & (1 << (cluster & 7)) != 0)
}

/// Set a bit in a row.
#[inline]
pub fn row_set(row: &mut [u8], cluster: usize) {
    if let Some(b) = row.get_mut(cluster >> 3) { *b |= 1 << (cluster & 7); }
}

/// Expand a run-length-encoded row.
///
/// A non-zero byte is literal. A zero byte is followed by a repeat count, and
/// stands for that many zero bytes. Decoding stops when `out` is full, so a
/// row that encodes more than it should cannot overrun.
pub fn decompress_row(compressed: &[u8], out: &mut [u8]) {
    let mut w = 0usize;
    let mut r = 0usize;
    while w < out.len() {
        let Some(&byte) = compressed.get(r) else {
            // Truncated data. Zero the remainder rather than leaving whatever
            // the caller's buffer held: failing safe here means hiding
            // geometry, whereas stale bits would show geometry at random.
            out[w..].fill(0);
            break;
        };
        r += 1;
        if byte != 0 {
            out[w] = byte;
            w += 1;
            continue;
        }
        let count = compressed.get(r).copied().unwrap_or(0) as usize;
        r += 1;
        // A zero count would spin forever; treat it as one byte.
        let count = count.max(1);
        let end = (w + count).min(out.len());
        out[w..end].fill(0);
        w = end;
    }
}

/// Run-length encode a row.
pub fn compress_row(row: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < row.len() {
        if row[i] != 0 {
            out.push(row[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < row.len() && row[i] == 0 { i += 1; }
        let mut run = i - start;
        // The count is one byte, so long runs emit several records.
        while run > 0 {
            let chunk = run.min(255);
            out.push(0);
            out.push(chunk as u8);
            run -= chunk;
        }
    }
}

/// Assembles a visibility lump from per-cluster rows.
///
/// Identical rows are stored once and shared, which matters more than it
/// sounds: in a map with long corridors, many clusters see exactly the same
/// set, and in the fully-uncompiled case *every* row is identical.
pub struct VisBuilder {
    num_clusters: usize,
    pvs: Vec<Vec<u8>>,
    pas: Vec<Vec<u8>>,
}

impl VisBuilder {
    pub fn new(num_clusters: usize) -> Self {
        let bytes = row_bytes(num_clusters);
        VisBuilder {
            num_clusters,
            pvs: vec![vec![0u8; bytes]; num_clusters],
            pas: vec![vec![0u8; bytes]; num_clusters],
        }
    }

    /// Every cluster sees every other -- the "vis was not run" fallback.
    pub fn all_visible(num_clusters: usize) -> Self {
        let mut b = VisBuilder::new(num_clusters);
        for row in b.pvs.iter_mut().chain(b.pas.iter_mut()) {
            row.fill(0xFF);
        }
        b
    }

    pub fn num_clusters(&self) -> usize { self.num_clusters }

    pub fn set_visible(&mut self, from: usize, to: usize) {
        if from < self.num_clusters { row_set(&mut self.pvs[from], to); }
    }

    pub fn set_audible(&mut self, from: usize, to: usize) {
        if from < self.num_clusters { row_set(&mut self.pas[from], to); }
    }

    pub fn pvs_row(&self, cluster: usize) -> &[u8] { &self.pvs[cluster] }
    pub fn pvs_row_mut(&mut self, cluster: usize) -> &mut [u8] { &mut self.pvs[cluster] }
    pub fn pas_row_mut(&mut self, cluster: usize) -> &mut [u8] { &mut self.pas[cluster] }

    /// Number of visible clusters summed over every row -- the headline
    /// statistic a vis compile reports.
    pub fn total_visible(&self) -> usize {
        self.pvs
            .iter()
            .map(|row| row.iter().map(|b| b.count_ones() as usize).sum::<usize>())
            .sum()
    }

    /// Derive the PAS from the PVS by flooding one extra step: a cluster is
    /// audible if it is visible from anything visible from here.
    pub fn derive_pas(&mut self) {
        let bytes = row_bytes(self.num_clusters);
        for from in 0..self.num_clusters {
            let mut acc = vec![0u8; bytes];
            for mid in 0..self.num_clusters {
                if !row_test(&self.pvs[from], mid) { continue; }
                for (a, b) in acc.iter_mut().zip(self.pvs[mid].iter()) { *a |= b; }
            }
            self.pas[from] = acc;
        }
    }

    /// Serialise into the visibility lump layout.
    pub fn build(&self) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();
        let mut offsets: Vec<[u32; 2]> = Vec::with_capacity(self.num_clusters);
        // Rows repeat often, so intern them by their compressed bytes.
        let mut interned: std::collections::HashMap<Vec<u8>, u32> = std::collections::HashMap::new();

        let table_size = 4 + self.num_clusters * 8;
        let emit = |row: &[u8],
                        data: &mut Vec<u8>,
                        interned: &mut std::collections::HashMap<Vec<u8>, u32>| -> u32 {
            let mut packed = Vec::new();
            compress_row(row, &mut packed);
            if let Some(&at) = interned.get(&packed) { return at; }
            let at = (table_size + data.len()) as u32;
            interned.insert(packed.clone(), at);
            data.extend_from_slice(&packed);
            at
        };

        for c in 0..self.num_clusters {
            let p = emit(&self.pvs[c], &mut data, &mut interned);
            let a = emit(&self.pas[c], &mut data, &mut interned);
            offsets.push([p, a]);
        }

        let mut out = Vec::with_capacity(table_size + data.len());
        let _ = out.write_all(&(self.num_clusters as u32).to_le_bytes());
        for [p, a] in &offsets {
            let _ = out.write_all(&p.to_le_bytes());
            let _ = out.write_all(&a.to_le_bytes());
        }
        out.extend_from_slice(&data);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_round_trips_sparse_rows() {
        let mut row = vec![0u8; 64];
        row[0] = 0x01;
        row[31] = 0xFF;
        row[63] = 0x80;
        let mut packed = Vec::new();
        compress_row(&row, &mut packed);
        assert!(packed.len() < row.len(), "sparse rows should shrink: {}", packed.len());

        let mut back = vec![0u8; 64];
        decompress_row(&packed, &mut back);
        assert_eq!(back, row);
    }

    #[test]
    fn rle_handles_runs_longer_than_a_count_byte() {
        // 1000 zero bytes cannot be one record; the encoder must split them.
        let mut row = vec![0u8; 1000];
        row[999] = 0x42;
        let mut packed = Vec::new();
        compress_row(&row, &mut packed);
        let mut back = vec![0u8; 1000];
        decompress_row(&packed, &mut back);
        assert_eq!(back, row);
    }

    #[test]
    fn rle_round_trips_a_dense_row() {
        let row: Vec<u8> = (0..255u8).collect();
        let mut packed = Vec::new();
        compress_row(&row, &mut packed);
        let mut back = vec![0u8; row.len()];
        decompress_row(&packed, &mut back);
        assert_eq!(back, row);
    }

    #[test]
    fn truncated_data_does_not_overrun() {
        let mut out = vec![0xAAu8; 32];
        decompress_row(&[0x01], &mut out); // claims one byte, row wants 32
        assert_eq!(out[0], 0x01);
        assert!(out[1..].iter().all(|&b| b == 0), "the rest must be zeroed, not garbage");
    }

    #[test]
    fn a_zero_repeat_count_terminates() {
        // Malformed input: a zero-run of length zero would loop forever if
        // taken literally.
        let mut out = vec![0u8; 8];
        decompress_row(&[0x00, 0x00, 0x00, 0x00], &mut out);
        assert!(out.iter().all(|&b| b == 0));
    }

    #[test]
    fn builder_round_trips_through_the_lump() {
        let mut b = VisBuilder::new(10);
        b.set_visible(0, 0);
        b.set_visible(0, 3);
        b.set_visible(0, 9);
        b.set_visible(5, 5);
        b.derive_pas();
        let lump = b.build();

        let vis = VisData::new(&lump).unwrap();
        assert_eq!(vis.num_clusters(), 10);
        assert!(vis.is_visible(0, 3, VisKind::Pvs));
        assert!(vis.is_visible(0, 9, VisKind::Pvs));
        assert!(!vis.is_visible(0, 4, VisKind::Pvs));
        assert!(vis.is_visible(5, 5, VisKind::Pvs));
        assert!(!vis.is_visible(5, 0, VisKind::Pvs));
    }

    #[test]
    fn all_visible_fallback_sees_everything() {
        let lump = VisBuilder::all_visible(20).build();
        let vis = VisData::new(&lump).unwrap();
        for to in 0..20 {
            assert!(vis.is_visible(7, to, VisKind::Pvs), "cluster {to}");
        }
    }

    #[test]
    fn identical_rows_are_stored_once() {
        // Every cluster sees everything, so all rows are identical and the
        // lump should be barely bigger than its offset table.
        let lump = VisBuilder::all_visible(200).build();
        let table = 4 + 200 * 8;
        assert!(
            lump.len() < table + 200,
            "duplicate rows were not interned: {} bytes for {table} of table",
            lump.len()
        );
    }

    #[test]
    fn pas_reaches_further_than_pvs() {
        // A -> B -> C chain: A sees B, B sees C, so A *hears* C.
        let mut b = VisBuilder::new(3);
        for (f, t) in [(0, 0), (0, 1), (1, 0), (1, 1), (1, 2), (2, 1), (2, 2)] {
            b.set_visible(f, t);
        }
        b.derive_pas();
        let lump = b.build();
        let vis = VisData::new(&lump).unwrap();
        assert!(!vis.is_visible(0, 2, VisKind::Pvs));
        assert!(vis.is_visible(0, 2, VisKind::Pas), "sound should carry around the corner");
    }

    #[test]
    fn empty_lump_reads_as_none() {
        assert!(VisData::new(&[]).is_none());
        assert!(VisData::new(&[0, 0, 0, 0]).is_none());
        assert!(VisData::new(&[1, 0, 0, 0]).is_none(), "table is missing");
    }

    #[test]
    fn out_of_range_clusters_are_not_visible() {
        let lump = VisBuilder::all_visible(4).build();
        let vis = VisData::new(&lump).unwrap();
        assert!(!vis.is_visible(99, 0, VisKind::Pvs));
        assert!(!vis.is_visible(0, 99, VisKind::Pvs));
    }

    #[test]
    fn row_bits_address_the_right_clusters() {
        let mut row = vec![0u8; row_bytes(20)];
        row_set(&mut row, 0);
        row_set(&mut row, 8);
        row_set(&mut row, 19);
        assert!(row_test(&row, 0) && row_test(&row, 8) && row_test(&row, 19));
        assert!(!row_test(&row, 1) && !row_test(&row, 18));
        assert!(!row_test(&row, 200), "out of range must not panic");
    }
}
