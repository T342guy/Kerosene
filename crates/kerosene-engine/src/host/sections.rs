// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What a loaded map keeps on the GPU, section by section, and its decals.
use kerosene_rhi::wgpu;

use super::*;

/// Decals cut out of the loaded sections' geometry, by decal id: each is
/// cut once, when it first appears, not every frame.
#[derive(Default)]
pub(super) struct DecalCache {
    pub(super) generation: Option<u64>,
    pub(super) cut: HashMap<u64, Vec<(usize, Vec<WorldVertex>)>>,
}

/// The GPU side of one section: its mesh, its buffers and materials, and
/// the frame bind group that carries its lightmap atlas.
pub(super) struct SectionGpu {
    pub(super) mesh: WorldMesh,
    pub(super) resources: MapResources,
    pub(super) frame_bind_group: wgpu::BindGroup,
}

/// What a worker thread hands back: a section's CPU-side data, ready to
/// upload, stamped with the load generation it was built for.
pub(super) struct BuiltSection {
    pub(super) generation: u64,
    pub(super) section: usize,
    pub(super) atlas: LightmapAtlas,
    pub(super) mesh: WorldMesh,
}

/// Geometry for the currently loaded map.
pub(super) struct LoadedMap {
    /// The engine's load generation these resources were built from.
    pub(super) generation: u64,
    /// Per section; `None` while not resident. Section 0, the world, is
    /// built with the map and never dropped.
    pub(super) sections: Vec<Option<SectionGpu>>,
    /// Sections a worker is building right now.
    pub(super) building: std::collections::HashSet<usize>,
    pub(super) tx: std::sync::mpsc::Sender<BuiltSection>,
    pub(super) rx: std::sync::mpsc::Receiver<BuiltSection>,
    /// The map's cubemap probes. Map-wide rather than per section: a probe
    /// is reflected by whatever can see it, loaded or not.
    pub(super) probes: GpuProbes,
}

impl LoadedMap {
    pub(super) fn world(&self) -> Option<&SectionGpu> {
        self.sections.first().and_then(Option::as_ref)
    }

    pub(super) fn loaded(&self) -> impl Iterator<Item = (usize, &SectionGpu)> {
        self.sections
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
    }
}

/// Upload one section's mesh and atlas to the device.
pub(super) fn upload_section(
    gfx: &Gfx,
    vfs: &kerosene_vfs::Vfs,
    mesh: WorldMesh,
    atlas: &LightmapAtlas,
    probes: &GpuProbes,
) -> SectionGpu {
    let resources = MapResources::upload(&gfx.device, &gfx.queue, &gfx.renderer, &mesh, atlas, vfs);
    let frame_bind_group =
        gfx.renderer
            .create_frame_bind_group(&gfx.device, &resources.lightmap_view, probes);
    SectionGpu {
        mesh,
        resources,
        frame_bind_group,
    }
}

/// Each streamed section's bounds as a wire box: green loaded, yellow on
/// its way, red unloaded.
pub(super) fn section_debug_lines(
    streaming: &crate::streaming::Streaming,
) -> Vec<crate::physics::DebugLine> {
    use crate::streaming::SectionState;
    let mut lines = Vec::new();
    for section in 1..streaming.section_count() {
        let b = streaming.bounds(section);
        if b.is_empty() {
            continue;
        }
        let color = match streaming.state(section) {
            SectionState::Loaded => [0.2, 1.0, 0.3],
            SectionState::Wanted => [1.0, 0.9, 0.2],
            SectionState::Unloaded => [1.0, 0.25, 0.25],
        };
        let c = |x: f32, y: f32, z: f32| kerosene_math::Vec3::new(x, y, z);
        let (lo, hi) = (b.min, b.max);
        let corners = [
            c(lo.x, lo.y, lo.z),
            c(hi.x, lo.y, lo.z),
            c(hi.x, hi.y, lo.z),
            c(lo.x, hi.y, lo.z),
            c(lo.x, lo.y, hi.z),
            c(hi.x, lo.y, hi.z),
            c(hi.x, hi.y, hi.z),
            c(lo.x, hi.y, hi.z),
        ];
        for (a, b) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            lines.push(crate::physics::DebugLine {
                a: corners[a],
                b: corners[b],
                color,
            });
        }
    }
    lines
}

/// Cut any decal not cut yet out of the loaded sections, and forget the ones
/// that have gone.
///
/// A section that streams in after a decal was placed does not get it: a
/// decal is cut once, against what was resident, like Source's.
pub(super) fn cut_decals(cache: &mut DecalCache, map: &LoadedMap, decals: &crate::ui::Decals) {
    if cache.generation != Some(map.generation) {
        cache.cut.clear();
        cache.generation = Some(map.generation);
    }
    cache
        .cut
        .retain(|id, _| decals.list.iter().any(|d| d.id == *id));
    for d in &decals.list {
        if cache.cut.contains_key(&d.id) {
            continue;
        }
        let mut spec = DecalSpec::new(&d.material, d.origin, d.normal, d.size);
        spec.rotation = d.rotation;
        let pieces = map
            .loaded()
            .filter_map(|(index, section)| {
                let surfaces = section.mesh.model_surfaces.first().map(Vec::as_slice);
                let cut = kerosene_render::decals::build(&section.mesh, &spec, surfaces);
                (!cut.is_empty()).then_some((index, cut))
            })
            .collect();
        cache.cut.insert(d.id, pieces);
    }
}
