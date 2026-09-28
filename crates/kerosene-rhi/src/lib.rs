// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The render hardware interface: the layer between Kerosene and the
//! graphics API.
//!
//! Everything above this -- materials, the scene, the renderer, the tools'
//! windows -- should reach the GPU through here, so that the graphics API is
//! one crate's business. Today that is wgpu, and this holds what the engine
//! and the tools share: opening a device for the configured [`Renderer`].
//! The rest of the device code moves here as the renderer is split up.

pub mod gpu;

use kerosene_config::Renderer;

/// The wgpu backends a configured renderer means.
///
/// The backend set is chosen when the wgpu instance is created, so asking
/// for Vulkan means creating an instance that only sees Vulkan -- and then
/// falling back to everything if no adapter shows up; see [`gpu::open`].
pub fn backends(renderer: Renderer) -> wgpu::Backends {
    match renderer {
        Renderer::Auto => wgpu::Backends::all(),
        Renderer::Vulkan => wgpu::Backends::VULKAN,
        Renderer::Metal => wgpu::Backends::METAL,
        Renderer::Dx12 => wgpu::Backends::DX12,
        Renderer::Gl => wgpu::Backends::GL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_renderer_maps_to_a_distinct_backend() {
        // Auto is everything; the named ones are exactly one backend each, so a
        // config that says "vulkan" cannot silently draw with metal.
        assert_eq!(backends(Renderer::Auto), wgpu::Backends::all());
        assert_eq!(backends(Renderer::Vulkan), wgpu::Backends::VULKAN);
        assert_eq!(backends(Renderer::Metal), wgpu::Backends::METAL);
        assert_eq!(backends(Renderer::Dx12), wgpu::Backends::DX12);
        assert_eq!(backends(Renderer::Gl), wgpu::Backends::GL);
    }
}
