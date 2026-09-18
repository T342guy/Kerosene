// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! A window with egui in it, and nothing else.
//!
//! The toolset is one GUI application, and every part of it that draws is an
//! [`App`] shown in this same window. A compiler you have to remember the
//! flags for is a compiler most people will not open, and "most people" here
//! includes anyone on Windows for whom a terminal is a foreign object. So the
//! tools that benefit from being *seen* -- a texture with its mips, a sound
//! with its waveform, an editor's four viewports -- draw in the window, while
//! the same stages remain headless subcommands for scripts and build servers.
//!
//! That window is the same window every time, which is the reason this crate
//! exists. Winit's application handler, a wgpu surface, an egui integration
//! and the frame loop that drives them are three hundred lines that have
//! nothing to do with any particular tool, and a second copy of them is a
//! second place for a resize bug to live.
//!
//! It is also where the toolset's look lives: [`theme`] is the palette, the
//! spacing and the icon font every tool draws with, [`widgets`] the handful
//! of controls they share, and [`output`] the panel their jobs log into. A
//! tool that draws with these looks like the others without trying to.
//!
//! Implement [`App`], call [`run`]:
//!
//! ```no_run
//! struct Hello;
//! impl kerosene_ui::App for Hello {
//!     fn ui(&mut self, ctx: &egui::Context) {
//!         egui::CentralPanel::default().show(ctx, |ui| ui.label("hello"));
//!     }
//! }
//! kerosene_ui::run("Hello", (1024, 768), Hello).unwrap();
//! ```

pub mod output;
pub mod theme;
pub mod widgets;

use anyhow::Result;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// What a tool has to provide to get a window.
pub trait App {
    /// Build one frame of interface.
    fn ui(&mut self, ctx: &egui::Context);

    /// What the title bar says, asked once a frame.
    ///
    /// Set from the host rather than by the app, because egui's viewport
    /// commands are only carried out by an integration that processes them,
    /// and this one draws egui itself. An app that returned a title through
    /// `ViewportCommand::Title` would find it silently ignored.
    fn window_title(&self) -> String {
        String::new()
    }

    /// Whether to draw again immediately rather than waiting for an event.
    ///
    /// False by default, and that default matters: a tool sitting idle should
    /// not spin a core the way a game loop does. Return true only while
    /// something is genuinely moving -- a meter during playback, a progress
    /// bar during a build.
    fn wants_continuous_redraw(&self) -> bool {
        false
    }

    /// The window's close button was pressed. Return `true` to let it close.
    ///
    /// An app with unsaved work answers `false` and puts up its question;
    /// when the answer comes back it sets [`App::wants_to_quit`] instead.
    /// Without this hook the close button was the one way out of the editor
    /// that never asked, on the one occasion asking matters most.
    fn close_requested(&mut self) -> bool {
        true
    }

    /// Whether the app has decided, on its own, that the window should close.
    /// Polled after every frame.
    fn wants_to_quit(&self) -> bool {
        false
    }
}

/// Open a window and run `app` in it until it is closed.
pub fn run(title: &str, size: (u32, u32), app: impl App + 'static) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut host = Host {
        app: Box::new(app),
        gfx: None,
        title: title.to_string(),
        initial_title: title.to_string(),
        size,
    };
    event_loop.run_app(&mut host)?;
    Ok(())
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
    app: Box<dyn App>,
    gfx: Option<Gfx>,
    /// The title the window is currently wearing.
    ///
    /// Kept so it is only set when it changes: `set_title` is a call into the
    /// window manager, and making it sixty times a second to say the same
    /// thing is work nobody asked for.
    title: String,
    initial_title: String,
    size: (u32, u32),
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match pollster::block_on(create_gfx(event_loop, &self.initial_title, self.size)) {
            Ok(gfx) => self.gfx = Some(gfx),
            Err(e) => {
                log::error!("could not open a window: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = &mut self.gfx else { return };

        // egui gets first refusal on every event.
        let response = gfx.egui_state.on_window_event(&gfx.window, &event);
        if response.repaint {
            gfx.window.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => {
                if self.app.close_requested() {
                    event_loop.exit();
                } else {
                    // The app has a question to draw.
                    gfx.window.request_redraw();
                }
            }
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
                if self.app.wants_to_quit() {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

impl Host {
    fn draw(&mut self) -> Result<()> {
        let Some(gfx) = &mut self.gfx else {
            return Ok(());
        };

        let frame = match gfx.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                return Ok(());
            }
            Err(e) => return Err(anyhow::anyhow!("{e}")),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let raw_input = gfx.egui_state.take_egui_input(&gfx.window);
        let context = gfx.egui_state.egui_ctx().clone();
        let output = context.run(raw_input, |ctx| self.app.ui(ctx));

        let Some(gfx) = &mut self.gfx else {
            return Ok(());
        };
        gfx.egui_state
            .handle_platform_output(&gfx.window, output.platform_output);

        let title = self.app.window_title();
        if !title.is_empty() && title != self.title {
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
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ui") });

        for (id, delta) in &output.textures_delta.set {
            gfx.egui_renderer
                .update_texture(&gfx.device, &gfx.queue, *id, delta);
        }
        gfx.egui_renderer
            .update_buffers(&gfx.device, &gfx.queue, &mut encoder, &jobs, &descriptor);

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.07,
                            b: 0.08,
                            a: 1.0,
                        }),
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

        // Only redraw when something asks to, or when the app says it is in
        // the middle of something worth watching.
        let egui_wants = output
            .viewport_output
            .values()
            .any(|v| v.repaint_delay.is_zero());
        if egui_wants || self.app.wants_continuous_redraw() {
            gfx.window.request_redraw();
        }
        Ok(())
    }
}

async fn create_gfx(event_loop: &ActiveEventLoop, title: &str, size: (u32, u32)) -> Result<Gfx> {
    let attributes = Window::default_attributes()
        .with_title(title)
        .with_window_icon(kerosene_config::icon::window_icon().and_then(|icon| {
            winit::window::Icon::from_rgba(icon.rgba, icon.width, icon.height).ok()
        }))
        .with_inner_size(winit::dpi::LogicalSize::new(size.0, size.1));
    // Wayland ignores an icon set on the window: the compositor shows the
    // icon of the .desktop file whose name matches the app id. Naming the
    // window "kerosene" is what makes a `kerosene.desktop` apply, and on X11
    // the same string is the WM_CLASS.
    #[cfg(target_os = "linux")]
    let attributes = {
        // Both traits have a `with_name`; each applies to its own backend
        // and is a no-op on the other, so both are set.
        winit::platform::wayland::WindowAttributesExtWayland::with_name(
            winit::platform::x11::WindowAttributesExtX11::with_name(
                attributes, "kerosene", "kerosene",
            ),
            "kerosene",
            "kerosene",
        )
    };
    let window = Arc::new(event_loop.create_window(attributes)?);

    // Tools share the engine's renderer default (Vulkan), falling back to
    // whatever is there when it is not. The integrated GPU is preferred:
    // a tool window is egui and nothing else.
    let gpu = kerosene_config::gpu::open(
        kerosene_config::Renderer::default(),
        wgpu::PowerPreference::LowPower,
        |instance| instance.create_surface(window.clone()).ok(),
    )
    .await
    .ok_or_else(|| anyhow::anyhow!("no suitable GPU adapter"))?;
    let surface = gpu.surface;
    let adapter = gpu.adapter;

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("kerosene-ui"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await?;

    let physical = window.inner_size();
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
        width: physical.width.max(1),
        height: physical.height.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: capabilities.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    let context = egui::Context::default();
    theme::install(&context);
    let egui_state = egui_winit::State::new(
        context,
        egui::ViewportId::ROOT,
        &window,
        Some(window.scale_factor() as f32),
        None,
        None,
    );
    let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

    Ok(Gfx {
        window,
        surface,
        device,
        queue,
        config,
        egui_state,
        egui_renderer,
    })
}
