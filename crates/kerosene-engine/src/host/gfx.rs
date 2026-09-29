// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The window, surface and device, and reading a frame back.
use kerosene_rhi::wgpu;

use super::*;

/// Window, surface, device: everything that only exists once there is a display.
pub(super) struct Gfx {
    pub(super) window: Arc<Window>,
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) renderer: Renderer,
    /// egui, for the developer console. Nothing else in the game uses it, and
    /// it costs nothing while the console is closed.
    pub(super) egui: egui::Context,
    pub(super) egui_state: egui_winit::State,
    pub(super) egui_renderer: egui_wgpu::Renderer,
    /// The game UI: HUD, menus, overlays and world panels.
    pub(super) ui_renderer: UiRenderer,
    pub(super) decals: GpuDecals,
    /// The `r_fullscreen` mode the window is in; `None` before the first
    /// frame sets it.
    pub(super) fullscreen: Option<i32>,
}

/// The window mode for an `r_fullscreen` value: 0 a window, 1 borderless
/// over the whole screen, 2 exclusive at the monitor's largest mode.
pub(super) fn fullscreen_mode(window: &Window, mode: i32) -> Option<winit::window::Fullscreen> {
    use winit::window::Fullscreen;
    match mode {
        0 => None,
        1 => Some(Fullscreen::Borderless(None)),
        _ => window
            .current_monitor()
            .and_then(|m| {
                m.video_modes().max_by_key(|v| {
                    let size = v.size();
                    (size.width * size.height, v.refresh_rate_millihertz())
                })
            })
            .map(Fullscreen::Exclusive)
            // Some platforms have no modes to offer (Wayland); borderless
            // is the nearest thing.
            .or(Some(Fullscreen::Borderless(None))),
    }
}

/// A frame on its way out of the GPU for `screenshot`.
pub(super) struct Screenshot(kerosene_rhi::Capture);

impl Screenshot {
    /// Copy the frame into a buffer the CPU can read. `None`, with a
    /// warning, where the surface cannot be copied from.
    pub(super) fn copy(
        gfx: &Gfx,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Option<Screenshot> {
        kerosene_rhi::Capture::copy(&gfx.device, &gfx.config, encoder, texture).map(Screenshot)
    }

    /// Wait for the copy, and write it as a PNG under `screenshots/` in the
    /// player's directory, named after the map and the time.
    pub(super) fn save(self, gfx: &Gfx, engine: &Engine) -> anyhow::Result<std::path::PathBuf> {
        let frame = self
            .0
            .read(&gfx.device)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, frame.width, frame.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&frame.rgba)?;
        }
        let stem = engine.map_name().unwrap_or("screenshot").to_string();
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let name = (0..)
            .map(|n| match n {
                0 => format!("screenshots/{stem}_{seconds}.png"),
                n => format!("screenshots/{stem}_{seconds}_{n}.png"),
            })
            .find(|name| !engine.vfs().exists(name))
            .expect("some name is free");
        Ok(engine.vfs().write(&name, &png)?)
    }
}

pub(super) async fn create_gfx(
    event_loop: &ActiveEventLoop,
    config: &EngineConfig,
) -> anyhow::Result<Gfx> {
    let attributes = Window::default_attributes()
        .with_title(config.title.as_str())
        .with_window_icon(kerosene_config::icon::window_icon().and_then(|icon| {
            winit::window::Icon::from_rgba(icon.rgba, icon.width, icon.height).ok()
        }))
        .with_inner_size(winit::dpi::LogicalSize::new(
            config.window_width,
            config.window_height,
        ));
    // Wayland ignores an icon set on the window: the compositor shows the
    // icon of the .desktop file whose name matches the app id. Naming the
    // window after the game's app id is what makes a `<app_id>.desktop`
    // apply, and on X11 the same string is the WM_CLASS.
    #[cfg(target_os = "linux")]
    let attributes = {
        let id = config.app_id.as_str();
        // Both traits have a `with_name`; each applies to its own backend
        // and is a no-op on the other, so both are set.
        winit::platform::wayland::WindowAttributesExtWayland::with_name(
            winit::platform::x11::WindowAttributesExtX11::with_name(attributes, id, id),
            id,
            id,
        )
    };
    let window = Arc::new(event_loop.create_window(attributes)?);

    let gpu = kerosene_rhi::gpu::open(
        config.renderer,
        wgpu::PowerPreference::HighPerformance,
        |instance| instance.create_surface(window.clone()).ok(),
    )
    .await
    .ok_or_else(|| anyhow::anyhow!("no suitable GPU adapter"))?;
    let surface = gpu.surface;
    let adapter = gpu.adapter;

    log::info!("gpu: {}", adapter.get_info().name);

    let (device, queue) = kerosene_rhi::request_device(&adapter).await?;

    let size = window.inner_size();
    let config =
        kerosene_rhi::surface_config(&surface, &adapter, size.width, size.height, config.vsync);
    let format = config.format;
    surface.configure(&device, &config);

    let mut renderer = Renderer::new(&device, format);
    renderer.ensure_targets(&device, config.width, config.height);

    let egui = egui::Context::default();
    let egui_state = egui_winit::State::new(
        egui.clone(),
        egui.viewport_id(),
        &window,
        Some(window.scale_factor() as f32),
        None,
        None,
    );
    // No depth attachment for the UI pass: the console is an overlay and is
    // meant to be in front of everything.
    let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);
    let ui_renderer = UiRenderer::new(&device, &queue);
    let decals = GpuDecals::new(&device, &queue);

    Ok(Gfx {
        window,
        surface,
        device,
        queue,
        config,
        renderer,
        egui,
        egui_state,
        egui_renderer,
        ui_renderer,
        decals,
        fullscreen: None,
    })
}
