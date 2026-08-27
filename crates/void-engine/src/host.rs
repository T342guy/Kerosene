// SPDX-License-Identifier: LGPL-3.0-or-later
//! The window, the GPU, and the frame loop.
//!
//! Everything display-dependent lives here, so that [`crate::engine::Engine`]
//! can run without any of it. The loop is:
//!
//! 1. Take real elapsed time and let the engine run as many fixed ticks as it
//!    covers.
//! 2. Build a camera from the interpolated player position.
//! 3. Ask the renderer what is visible and draw it.
//!
//! Simulation is decoupled from rendering on purpose. A 240 Hz display should
//! draw 240 smooth frames of a 64 Hz simulation, not simulate 240 times.

use crate::engine::{Engine, EngineConfig, take_console_requests};
use crate::input::InputSystem;
use void_console::ConsoleUi;
use std::sync::Arc;
use std::time::Instant;
use void_render::gpu::{CameraUniform, MapResources, Renderer};
use void_render::{Camera, FrameStats, LightmapAtlas, WorldMesh};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

/// Window, surface, device: everything that only exists once there is a display.
struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    /// egui, for the developer console. Nothing else in the game uses it, and
    /// it costs nothing while the console is closed.
    egui: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

/// Geometry for the currently loaded map.
struct LoadedMap {
    name: String,
    mesh: WorldMesh,
    resources: MapResources,
    frame_bind_group: wgpu::BindGroup,
}

struct App {
    engine: Engine,
    input: InputSystem,
    gfx: Option<Gfx>,
    map: Option<LoadedMap>,
    last_frame: Instant,
    /// Whether the mouse is captured for looking around.
    mouse_captured: bool,
    stats: FrameStats,
    /// Seconds since the last `r_speeds` report.
    since_report: f32,
    console_ui: ConsoleUi,
}

/// Start the engine with a window.
pub fn run(config: EngineConfig) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    // Poll rather than Wait: a game redraws continuously.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        engine: Engine::new(&config),
        input: InputSystem::new(),
        gfx: None,
        map: None,
        last_frame: Instant::now(),
        mouse_captured: false,
        stats: FrameStats::default(),
        since_report: 0.0,
        console_ui: ConsoleUi::new(),
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() { return; }
        match pollster::block_on(create_gfx(event_loop)) {
            Ok(gfx) => {
                self.gfx = Some(gfx);
                self.last_frame = Instant::now();
            }
            Err(e) => {
                log::error!("could not start the renderer: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // egui gets first refusal while the console is open, and the game
        // sees nothing: a console you cannot type an `n` into without walking
        // forward is not a console.
        if self.console_ui.open {
            if let Some(gfx) = &mut self.gfx {
                let response = gfx.egui_state.on_window_event(&gfx.window, &event);
                let swallow = response.consumed
                    && !matches!(event, WindowEvent::RedrawRequested | WindowEvent::CloseRequested);
                if swallow { return }
            }
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.config.width = size.width.max(1);
                    gfx.config.height = size.height.max(1);
                    gfx.surface.configure(&gfx.device, &gfx.config);
                    gfx.renderer.ensure_depth(&gfx.device, gfx.config.width, gfx.config.height);
                }
            }

            WindowEvent::Focused(focused) => {
                // Releasing held keys on focus loss stops the player running
                // forever after an alt-tab.
                if !focused {
                    self.input.release_all();
                    self.set_mouse_capture(false);
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if event.repeat { return; }

                if let PhysicalKey::Code(code) = event.physical_key {
                    // The console key is handled here rather than through a
                    // binding so that it always works, including out of a
                    // console whose bindings someone has just broken.
                    if matches!(code, KeyCode::Backquote) && pressed {
                        self.toggle_console();
                        return;
                    }
                    if code == KeyCode::Escape && pressed {
                        if self.console_ui.open {
                            self.toggle_console();
                            return;
                        }
                        self.set_mouse_capture(false);
                        return;
                    }
                    // Held movement keys must not stay held while the console
                    // has the keyboard, or the player walks the whole time it
                    // is open.
                    if self.console_ui.open { return }
                    if let Some(name) = key_name(code) {
                        if let Some(command) = self.input.key_event(name, pressed) {
                            self.engine.console.execute_user(&command);
                        }
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if self.console_ui.open { return }
                if button == MouseButton::Left && state == ElementState::Pressed && !self.mouse_captured {
                    // Clicking the window takes the mouse, the way every game
                    // does; escape gives it back.
                    self.set_mouse_capture(true);
                    return;
                }
                if let Some(name) = mouse_button_name(button) {
                    let pressed = state == ElementState::Pressed;
                    if let Some(command) = self.input.key_event(name, pressed) {
                        self.engine.console.execute_user(&command);
                    }
                }
            }

            WindowEvent::RedrawRequested => self.frame(event_loop),

            _ => {}
        }
    }

    fn device_event(&mut self, _loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // Raw device motion rather than cursor position: it keeps working when
        // the pointer is locked, and it is not affected by OS mouse
        // acceleration or by hitting the edge of the screen.
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.mouse_captured {
                self.input.mouse_moved(delta.0 as f32, delta.1 as f32);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }
}

impl App {
    /// Open or close the console, and hand the mouse and keyboard over.
    fn toggle_console(&mut self) {
        self.console_ui.toggle();
        if self.console_ui.open {
            // Whatever was held stays held forever otherwise.
            self.input.release_all();
            self.set_mouse_capture(false);
        }
    }

    fn set_mouse_capture(&mut self, capture: bool) {
        let Some(gfx) = &self.gfx else { return };
        self.mouse_captured = capture;
        let mode = if capture { CursorGrabMode::Locked } else { CursorGrabMode::None };
        // Locked is unavailable on some platforms; confined is the next best
        // thing, and failing at both is not worth stopping over.
        if gfx.window.set_cursor_grab(mode).is_err() && capture {
            let _ = gfx.window.set_cursor_grab(CursorGrabMode::Confined);
        }
        gfx.window.set_cursor_visible(!capture);
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let real_dt = (now - self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;

        self.input.update_view(&self.engine.console);
        let input_state = self.input.state();

        self.engine.frame(real_dt, &input_state);
        take_console_requests(&mut self.engine);

        if self.engine.should_quit {
            event_loop.exit();
            return;
        }

        // Rebuild GPU resources when the map changes.
        let current = self.engine.level.as_ref().map(|l| l.name.clone());
        let loaded = self.map.as_ref().map(|m| m.name.clone());
        if current != loaded {
            self.rebuild_map();
        }

        self.draw(real_dt);
    }

    fn rebuild_map(&mut self) {
        let (Some(gfx), Some(level)) = (&self.gfx, &self.engine.level) else {
            self.map = None;
            return;
        };

        let exposure = self.engine.console.float("mat_exposure");
        let atlas = LightmapAtlas::build(&level.bsp, exposure);
        let mesh = WorldMesh::build(&level.bsp, &atlas);
        let resources = MapResources::upload(
            &gfx.device,
            &gfx.queue,
            &gfx.renderer,
            &mesh,
            &atlas,
            &self.engine.vfs,
        );
        let frame_bind_group =
            gfx.renderer.create_frame_bind_group(&gfx.device, &resources.lightmap_view);

        for missing in &resources.missing_materials {
            self.engine.console.warn(format!("missing material: {missing}"));
        }
        self.engine.console.print(format!(
            "{} surfaces, {} triangles, {} materials, lightmap atlas {:.0}% full",
            mesh.surfaces.len(),
            mesh.triangle_count(),
            mesh.materials.len(),
            atlas.occupancy() * 100.0
        ));

        self.map = Some(LoadedMap {
            name: level.name.clone(),
            mesh,
            resources,
            frame_bind_group,
        });
    }

    fn draw(&mut self, real_dt: f32) {
        let Some(gfx) = &mut self.gfx else { return };

        let frame = match gfx.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                return;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => return,
            Err(_) => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        gfx.renderer.ensure_depth(&gfx.device, gfx.config.width, gfx.config.height);

        // Interpolate between the last two simulation states, so the view is
        // smooth on a display refreshing faster than the tick rate.
        let alpha = 1.0;
        let camera = Camera {
            position: self.engine.interpolated_eye(alpha),
            angles: self.engine.player.view_angles,
            fov: self.engine.console.float("cl_fov"),
            aspect: gfx.config.width as f32 / gfx.config.height.max(1) as f32,
            ..Default::default()
        };

        let mut uniform = CameraUniform::from_camera(
            &camera,
            self.engine.console.float("mat_exposure"),
            self.engine.time,
        );
        uniform.set_lightmaps(self.engine.console.bool("r_lightmap"));
        uniform.set_fullbright(self.engine.console.bool("r_fullbright"));
        gfx.renderer.update_camera(&gfx.queue, &uniform);

        let mut encoder = gfx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

        {
            let depth_view = gfx.renderer.depth_view().expect("depth buffer exists");
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.02, g: 0.02, b: 0.04, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if let (Some(map), Some(level)) = (&self.map, &self.engine.level) {
                if self.engine.console.bool("r_drawworld") {
                    let visible = if self.engine.console.bool("r_novis") {
                        map.mesh.all_surfaces()
                    } else {
                        map.mesh.visible_surfaces(&level.bsp, camera.position, &camera.frustum())
                    };
                    self.stats = gfx.renderer.draw_world(
                        &mut pass,
                        &map.frame_bind_group,
                        &map.resources,
                        &map.mesh,
                        &visible,
                    );
                    self.stats.cluster = level.bsp.point_cluster(camera.position);
                }
            }
        }

        if self.console_ui.open {
            draw_console(
                gfx,
                &mut encoder,
                &view,
                &mut self.console_ui,
                &mut self.engine.console,
            );
        }

        gfx.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        self.report_speeds(real_dt);
    }

    /// Periodic `r_speeds` output.
    fn report_speeds(&mut self, real_dt: f32) {
        if !self.engine.console.bool("r_speeds") { return; }
        self.since_report += real_dt;
        if self.since_report < 1.0 { return; }
        self.since_report = 0.0;

        let s = self.stats;
        let message = format!(
            "{:.0} fps | {}/{} surfaces ({:.0}% culled) | {} tris | {} draws | cluster {}",
            1.0 / real_dt.max(1e-6),
            s.surfaces_drawn,
            s.surfaces_total,
            s.culled_fraction() * 100.0,
            s.triangles,
            s.draw_calls,
            s.cluster
        );
        self.engine.console.print(message);
    }
}

/// Run one egui frame for the console and record it into the encoder.
///
/// A free function rather than a method so it can borrow the graphics state
/// and the console at once without the whole `App` going along with it.
fn draw_console(
    gfx: &mut Gfx,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    console_ui: &mut ConsoleUi,
    console: &mut void_console::Console,
) {
    let input = gfx.egui_state.take_egui_input(&gfx.window);
    let output = gfx.egui.run(input, |ctx| {
        crate::console_ui::draw(ctx, console_ui, console);
    });
    gfx.egui_state.handle_platform_output(&gfx.window, output.platform_output);

    let triangles = gfx.egui.tessellate(output.shapes, output.pixels_per_point);
    for (id, delta) in &output.textures_delta.set {
        gfx.egui_renderer.update_texture(&gfx.device, &gfx.queue, *id, delta);
    }
    let descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [gfx.config.width, gfx.config.height],
        pixels_per_point: output.pixels_per_point,
    };
    gfx.egui_renderer.update_buffers(&gfx.device, &gfx.queue, encoder, &triangles, &descriptor);

    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("console"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    // Load, not clear: the game is behind it.
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        gfx.egui_renderer.render(&mut pass, &triangles, &descriptor);
    }

    for id in &output.textures_delta.free {
        gfx.egui_renderer.free_texture(id);
    }
}

async fn create_gfx(event_loop: &ActiveEventLoop) -> anyhow::Result<Gfx> {
    let attributes = Window::default_attributes()
        .with_title("VoidEngine")
        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
    let window = Arc::new(event_loop.create_window(attributes)?);

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let surface = instance.create_surface(window.clone())?;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| anyhow::anyhow!("no suitable GPU adapter: {e}"))?;

    log::info!("gpu: {}", adapter.get_info().name);

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("void"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        })
        .await?;

    let size = window.inner_size();
    let capabilities = surface.get_capabilities(&adapter);
    // Prefer an sRGB target so the shader's gamma step lands correctly.
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

    let mut renderer = Renderer::new(&device, format);
    renderer.ensure_depth(&device, config.width, config.height);

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

    Ok(Gfx { window, surface, device, queue, config, renderer, egui, egui_state, egui_renderer })
}

/// Map a physical key to the name bindings use.
///
/// Physical rather than logical, so WASD stays where it is on a keyboard laid
/// out differently -- which is what a player on AZERTY expects from a game.
fn key_name(code: KeyCode) -> Option<&'static str> {
    use KeyCode::*;
    Some(match code {
        KeyA => "a", KeyB => "b", KeyC => "c", KeyD => "d", KeyE => "e",
        KeyF => "f", KeyG => "g", KeyH => "h", KeyI => "i", KeyJ => "j",
        KeyK => "k", KeyL => "l", KeyM => "m", KeyN => "n", KeyO => "o",
        KeyP => "p", KeyQ => "q", KeyR => "r", KeyS => "s", KeyT => "t",
        KeyU => "u", KeyV => "v", KeyW => "w", KeyX => "x", KeyY => "y",
        KeyZ => "z",
        Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4",
        Digit5 => "5", Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9",
        Space => "space",
        ControlLeft | ControlRight => "ctrl",
        ShiftLeft | ShiftRight => "shift",
        AltLeft | AltRight => "alt",
        Enter => "enter",
        Tab => "tab",
        Backquote => "`",
        Escape => "escape",
        F1 => "f1", F2 => "f2", F3 => "f3", F4 => "f4", F5 => "f5", F6 => "f6",
        F7 => "f7", F8 => "f8", F9 => "f9", F10 => "f10", F11 => "f11", F12 => "f12",
        _ => return None,
    })
}

fn mouse_button_name(button: MouseButton) -> Option<&'static str> {
    Some(match button {
        MouseButton::Left => "mouse1",
        MouseButton::Right => "mouse2",
        MouseButton::Middle => "mouse3",
        _ => return None,
    })
}
