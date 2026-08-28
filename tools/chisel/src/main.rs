// SPDX-License-Identifier: LGPL-3.0-or-later
//! Chisel -- the VoidEngine world editor.
//!
//! ```text
//! chisel                          # start with a sample room
//! chisel content/maps/void_start.voidmap
//! chisel --content content        # where to look for materials
//! chisel --no-build               # skip the texture build on the way in
//! ```
//!
//! The editor's logic lives in the `chisel` library and is tested without a
//! window; this file is the window.

use anyhow::Result;
use chisel::app::{ChiselApp, starter_document};
use std::path::PathBuf;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let mut map: Option<PathBuf> = None;
    let mut content: Option<PathBuf> = None;
    let mut build = true;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--content" => {
                i += 1;
                content = args.get(i).map(PathBuf::from);
            }
            "--no-build" => build = false,
            "--help" | "-h" => {
                println!("chisel [map.voidmap] [--content <dir>] [--no-build]");
                println!();
                println!("With no --content, the content tree is found: beside the map,");
                println!("then from the working directory, then beside the executable.");
                println!();
                println!("Textures are built from the content tree's art before the editor");
                println!("opens. --no-build skips that; nothing already compiled is rebuilt");
                println!("either way.");
                return Ok(());
            }
            other => map = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    // Searched for rather than assumed. Guessing `./content` worked from the
    // repository root and silently found nothing anywhere else, which left
    // the editor with no entity classes and no materials -- looking broken
    // rather than misconfigured.
    let found = void_vfs::root::find(content.as_deref(), map.as_deref());
    log::info!("{}", void_vfs::root::describe(&found));
    let root = found.as_ref().map(|f| f.root.clone()).unwrap_or_default();

    // Before the editor is built, not after: `ChiselApp::new` scans the
    // materials tree once, and an editor that scanned it before the textures
    // existed is an editor with no textures in it -- which is what happened,
    // and which no amount of reloading afterwards makes obvious.
    let build_note = if build { build_content(&found) } else { None };

    let mut app = ChiselApp::new(root);
    app.content_note = void_vfs::root::describe(&found);
    if found.is_none() {
        app.status = app.content_note.clone();
    } else if let Some(note) = build_note {
        app.status = note;
    }
    match map {
        Some(path) => app.open(path),
        // A fresh editor opens on a room rather than an empty void: an empty
        // 3D view gives no sense of scale and nothing to orient against.
        None => app.document = starter_document(),
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut host = Host { app, gfx: None, title: String::new() };
    event_loop.run_app(&mut host)?;
    Ok(())
}

/// Build the content tree's textures, before anything reads them.
///
/// Returns what to put in the status bar, or `None` when there was nothing to
/// say. A failure here is reported and then ignored: a texture that will not
/// compile is a reason to show the editor and say so, not a reason to refuse
/// to open at all.
fn build_content(found: &Option<void_vfs::root::Found>) -> Option<String> {
    let root = &found.as_ref()?.root;
    match alchemy::build_textures(root) {
        Ok(build) => {
            log::info!("{build}");
            // Silent when there was nothing to do. The interesting case is the
            // first run in a fresh clone, where it explains the pause.
            build.did_anything().then(|| build.to_string())
        }
        Err(e) => {
            log::warn!("could not build textures: {e:#}");
            Some(format!("could not build textures: {e}"))
        }
    }
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

struct Host {
    app: ChiselApp,
    gfx: Option<Gfx>,
    /// The title the window is currently wearing.
    ///
    /// Kept so it is only set when it changes: `set_title` is a call into the
    /// window manager, and making it sixty times a second to say the same
    /// thing is work nobody asked for.
    title: String,
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() { return; }
        match pollster::block_on(create_gfx(event_loop)) {
            Ok(gfx) => self.gfx = Some(gfx),
            Err(e) => {
                log::error!("could not open a window: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = &mut self.gfx else { return };

        // egui gets first refusal on every event; anything it consumes is a
        // click on a menu or a text field rather than in the world.
        let response = gfx.egui_state.on_window_event(&gfx.window, &event);
        if response.repaint {
            gfx.window.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(&gfx.device, &gfx.config);
                gfx.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.draw() {
                    log::warn!("frame skipped: {e}");
                }
            }
            _ => {}
        }
    }
}

impl Host {
    fn draw(&mut self) -> Result<()> {
        let Some(gfx) = &mut self.gfx else { return Ok(()) };

        let frame = match gfx.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                return Ok(());
            }
            Err(e) => return Err(anyhow::anyhow!("{e}")),
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let raw_input = gfx.egui_state.take_egui_input(&gfx.window);
        let context = gfx.egui_state.egui_ctx().clone();
        let output = context.run(raw_input, |ctx| self.app.ui(ctx));

        let Some(gfx) = &mut self.gfx else { return Ok(()) };
        gfx.egui_state.handle_platform_output(&gfx.window, output.platform_output);

        // The title says which map is open and whether it is saved. Set from
        // here rather than from the editor, because the window is the host's:
        // egui's viewport commands are only carried out by an integration
        // that processes them, and this one draws egui itself.
        let title = self.app.window_title();
        if title != self.title {
            gfx.window.set_title(&title);
            self.title = title;
        }

        let jobs = context.tessellate(output.shapes, output.pixels_per_point);
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gfx.config.width, gfx.config.height],
            pixels_per_point: output.pixels_per_point,
        };

        let mut encoder = gfx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("chisel") });

        for (id, delta) in &output.textures_delta.set {
            gfx.egui_renderer.update_texture(&gfx.device, &gfx.queue, *id, delta);
        }
        gfx.egui_renderer.update_buffers(&gfx.device, &gfx.queue, &mut encoder, &jobs, &descriptor);

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.06, g: 0.07, b: 0.08, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut pass = pass.forget_lifetime();
            gfx.egui_renderer.render(&mut pass, &jobs, &descriptor);
        }

        for id in &output.textures_delta.free {
            gfx.egui_renderer.free_texture(id);
        }

        gfx.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        // Only redraw when something asks to: an editor sitting idle should
        // not spin a core the way a game loop does.
        if output.viewport_output.values().any(|v| v.repaint_delay.is_zero()) {
            gfx.window.request_redraw();
        }
        Ok(())
    }
}

async fn create_gfx(event_loop: &ActiveEventLoop) -> Result<Gfx> {
    let attributes = Window::default_attributes()
        .with_title("Chisel -- VoidEngine world editor")
        .with_inner_size(winit::dpi::LogicalSize::new(1600, 950));
    let window = Arc::new(event_loop.create_window(attributes)?);

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let surface = instance.create_surface(window.clone())?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| anyhow::anyhow!("no suitable GPU adapter: {e}"))?;

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("chisel"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await?;

    let size = window.inner_size();
    let capabilities = surface.get_capabilities(&adapter);
    let format = capabilities
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(capabilities.formats[0]);

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: capabilities.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    let context = egui::Context::default();
    context.set_visuals(egui::Visuals::dark());
    let egui_state = egui_winit::State::new(
        context,
        egui::ViewportId::ROOT,
        &window,
        Some(window.scale_factor() as f32),
        None,
        None,
    );
    let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

    Ok(Gfx { window, surface, device, queue, config, egui_state, egui_renderer })
}
