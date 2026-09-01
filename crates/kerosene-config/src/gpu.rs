// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Opening a GPU for the configured renderer.
//!
//! wgpu decides its backend when the instance is created, not when an adapter
//! is requested, so honouring `renderer` means creating an instance that only
//! sees that backend and, when no adapter is there, trying again with every
//! backend rather than refusing to start.

use crate::Renderer;

/// A surface and the adapter that will draw to it.
///
/// The instance is not kept: wgpu needs it only to create the surface and the
/// adapter, and a caller that had to store it would be storing a handle to
/// nothing it ever uses again.
pub struct Gpu {
    pub surface: wgpu::Surface<'static>,
    pub adapter: wgpu::Adapter,
}

/// Open a surface and pick an adapter, honouring `renderer`.
///
/// The configured backend is tried first, then everything, so a config that
/// asks for Vulkan on a machine without it still starts -- it just logs that
/// it fell back. `make_surface` is handed each instance in turn, because a
/// surface belongs to the instance that made it.
pub async fn open(
    renderer: Renderer,
    power: wgpu::PowerPreference,
    make_surface: impl Fn(&wgpu::Instance) -> Option<wgpu::Surface<'static>>,
) -> Option<Gpu> {
    let preferred = renderer.wgpu_backends();
    let mut attempts = vec![preferred];
    if preferred != wgpu::Backends::all() {
        attempts.push(wgpu::Backends::all());
    }

    for (fallback, backends) in attempts.into_iter().enumerate() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let Some(surface) = make_surface(&instance) else { continue };
        let Ok(adapter) = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: power,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
        else { continue };

        if fallback == 0 {
            log::info!("renderer: {}", adapter.get_info().backend);
        } else {
            log::warn!(
                "renderer {} is not available; using {}",
                renderer.label(),
                adapter.get_info().backend,
            );
        }
        return Some(Gpu { surface, adapter });
    }

    None
}
