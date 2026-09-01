// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Which renderer to use, and what that means for wgpu.

/// The renderer a config file asks for.
///
/// wgpu sits on top of several backends; this is the name a person writes for
/// one of them. Vulkan is the default, and `auto` lets wgpu pick whatever it
/// can find.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Renderer {
    Auto,
    Vulkan,
    Metal,
    Dx12,
    Gl,
}

impl Default for Renderer {
    fn default() -> Self { Renderer::Vulkan }
}

impl Renderer {
    /// The name as it is written in a config file.
    pub fn label(self) -> &'static str {
        match self {
            Renderer::Auto => "auto",
            Renderer::Vulkan => "vulkan",
            Renderer::Metal => "metal",
            Renderer::Dx12 => "dx12",
            Renderer::Gl => "gl",
        }
    }

    /// Parse a config value, accepting a few friendly aliases.
    pub fn from_str(s: &str) -> Option<Renderer> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "any" => Some(Renderer::Auto),
            "vulkan" | "vk" => Some(Renderer::Vulkan),
            "metal" => Some(Renderer::Metal),
            "dx12" | "d3d12" | "directx12" => Some(Renderer::Dx12),
            "gl" | "opengl" | "gles" => Some(Renderer::Gl),
            _ => None,
        }
    }

    /// Every renderer a config may name, in the order a menu would show them.
    pub fn all() -> [Renderer; 5] {
        [
            Renderer::Vulkan,
            Renderer::Auto,
            Renderer::Metal,
            Renderer::Dx12,
            Renderer::Gl,
        ]
    }

    /// The wgpu backends this renderer means.
    ///
    /// The backend set is chosen when the wgpu instance is created, so asking
    /// for Vulkan means creating an instance that only sees Vulkan -- and then
    /// falling back to everything if no adapter shows up.
    pub fn wgpu_backends(self) -> wgpu::Backends {
        match self {
            Renderer::Auto => wgpu::Backends::all(),
            Renderer::Vulkan => wgpu::Backends::VULKAN,
            Renderer::Metal => wgpu::Backends::METAL,
            Renderer::Dx12 => wgpu::Backends::DX12,
            Renderer::Gl => wgpu::Backends::GL,
        }
    }
}
