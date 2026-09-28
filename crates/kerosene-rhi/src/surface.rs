// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The logical device, and the swap chain it draws to.

/// Ask the adapter for a device and its queue, with the defaults Kerosene
/// draws with: no optional features, the default limits.
pub async fn request_device(
    adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("kerosene"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await
}

/// The present mode for a vsync setting.
pub fn present_mode(vsync: bool) -> wgpu::PresentMode {
    if vsync {
        wgpu::PresentMode::AutoVsync
    } else {
        wgpu::PresentMode::AutoNoVsync
    }
}

/// How to configure `surface` for `adapter` at a size.
///
/// Prefers an sRGB target so the shader's gamma step lands correctly, and
/// asks for a surface that can be copied from where the platform allows it,
/// for [`crate::Capture`].
pub fn surface_config(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
    vsync: bool,
) -> wgpu::SurfaceConfiguration {
    let capabilities = surface.get_capabilities(adapter);
    let format = capabilities
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(capabilities.formats[0]);
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | (capabilities.usages & wgpu::TextureUsages::COPY_SRC),
        format,
        width: width.max(1),
        height: height.max(1),
        present_mode: present_mode(vsync),
        alpha_mode: capabilities.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    }
}
