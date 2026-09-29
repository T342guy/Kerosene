// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
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

use crate::engine::{Engine, EngineConfig, report_unhandled, take_console_requests};
use crate::game::Game;
use kerosene_console::ConsoleUi;
use kerosene_entity::ModelRole;
use kerosene_math::Pose;
use kerosene_render::decals::DecalSpec;
use kerosene_render::gpu::{
    CameraUniform, GpuModel, GpuProbes, LineVertex, MAX_MODELS, MapResources, ModelInstance,
    ModelUniform, Renderer, ToneMapOperator, load_model,
};
use kerosene_render::gpu::{DecalDraw, GpuDecals};
use kerosene_render::lights::{DynamicLight, LightFrame};
use kerosene_render::ui::{PanelQuad, UiRenderer};
use kerosene_render::{Camera, FrameStats, Frustum, LightmapAtlas, WorldMesh, WorldVertex};
use kerosene_rhi::wgpu::util::DeviceExt;
use kerosene_ui::{UiInput, UiKey};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
mod draw;
mod gfx;
mod keys;
mod sections;

use gfx::*;
use keys::*;
use sections::*;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

struct App {
    engine: Engine,
    config: EngineConfig,
    gfx: Option<Gfx>,
    map: Option<LoadedMap>,
    last_frame: Instant,
    /// Whether the mouse is captured for looking around.
    mouse_captured: bool,
    stats: FrameStats,
    /// Seconds since the last `r_speeds` report.
    since_report: f32,
    console_ui: ConsoleUi,
    /// Uploaded `.kmdl` models, keyed by the name an entity refers to them by.
    model_cache: HashMap<String, Option<GpuModel>>,
    /// The probe each static prop reflects. Chosen once -- a static prop
    /// does not move -- rather than traced for every frame.
    static_probes: HashMap<kerosene_entity::EntityId, u32>,
    decal_cache: DecalCache,
    /// Whether a UI layer had the mouse and keyboard last frame, so the
    /// frame it lets go can hand the mouse back to the game.
    menu_open: bool,
    /// Whether the window has the keyboard. Out of focus, the game pauses
    /// and falls silent (`sv_pause_on_menu`, `snd_mute_losefocus`).
    focused: bool,
    /// Whether the window cannot be seen at all: minimised, or covered.
    /// Nothing is drawn then, since nobody is looking.
    occluded: bool,
    shift_held: bool,
    /// `screenshot` asked for the next frame.
    screenshot: bool,
}

/// Start the engine with a window and no game.
pub fn run(config: EngineConfig) -> anyhow::Result<()> {
    run_with(config, Box::new(()))
}

/// Start the engine with a window, running `game`.
pub fn run_with(config: EngineConfig, game: Box<dyn Game>) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    // Poll rather than Wait: a game redraws continuously.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        engine: Engine::with_game(&config, game),
        config,
        gfx: None,
        map: None,
        last_frame: Instant::now(),
        mouse_captured: false,
        stats: FrameStats::default(),
        since_report: 0.0,
        console_ui: ConsoleUi::new(),
        model_cache: HashMap::new(),
        static_probes: HashMap::new(),
        decal_cache: DecalCache::default(),
        menu_open: false,
        focused: true,
        occluded: false,
        shift_held: false,
        screenshot: false,
    };
    // A window has a frame to draw while a map loads; say which.
    app.engine.set_loading_screen(true);
    // Opening on the main menu, which the splash sits in front of for a
    // moment; a game started on a map skips both.
    if app.config.map.is_none() {
        app.engine.show_splash();
    }

    event_loop.run_app(&mut app)?;
    Ok(())
}

impl App {
    fn game_wants_ui(&self) -> bool {
        self.engine.game().is_some_and(|g| g.wants_ui())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match pollster::block_on(create_gfx(event_loop, &self.config)) {
            Ok(gfx) => {
                self.gfx = Some(gfx);
                self.last_frame = Instant::now();
            }
            Err(e) => {
                log::error!("could not start the renderer: {e}");
                // The window never opened, so without this the player saw
                // the game do nothing at all.
                kerosene_console::dialog::show_error(
                    &self.config.title,
                    &format!(
                        "{} could not start its renderer:\n\n{e}\n\n\
                         Check that the graphics driver is installed and up to date, \
                         or try another renderer in engine.kcfg.",
                        self.config.title
                    ),
                );
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // The keys that open and close the console are handled here, before
        // anything else sees them, and are never passed on.
        //
        // Afterwards would be too late. A focused text field makes egui claim
        // every keystroke, so the console swallowed the very keys that close
        // it -- and the backtick that opened it was typed *into* the prompt,
        // so every command after it began with a character that made it
        // unknown. One ordering mistake, and the console appeared both to
        // have no commands and to be impossible to leave.
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && let PhysicalKey::Code(code) = event.physical_key
            && let Some(action) = self.intercept(code)
        {
            // On the press, not the release, and not on auto-repeat: a held
            // key must not toggle the console sixty times a second.
            if event.state == ElementState::Pressed && !event.repeat {
                match action {
                    Intercepted::ToggleConsole => self.toggle_console(),
                    Intercepted::ReleaseMouse => self.escape(),
                }
            }
            return;
        }

        // egui gets first refusal on everything else while the console is
        // open, and the game sees nothing: a console you cannot type an `n`
        // into without walking forward is not a console. The game's own UI
        // gets the same only while the mouse is free: a HUD drawn during
        // play must never be able to eat a movement key.
        let game_ui = !self.mouse_captured && self.game_wants_ui();
        if (self.console_ui.open || game_ui)
            && let Some(gfx) = &mut self.gfx
        {
            let response = gfx.egui_state.on_window_event(&gfx.window, &event);
            let swallow = response.consumed
                && !matches!(
                    event,
                    WindowEvent::RedrawRequested | WindowEvent::CloseRequested
                );
            if swallow {
                return;
            }
        }

        if let WindowEvent::ModifiersChanged(modifiers) = &event {
            self.shift_held = modifiers.state().shift_key();
        }

        // A UI layer that wants input -- a menu -- is modal: the pointer and
        // keys are its, and none reaches a binding. The console, above, still
        // comes first.
        if !self.console_ui.open && self.engine.ui_wants_input() && self.ui_window_event(&event) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.config.width = size.width.max(1);
                    gfx.config.height = size.height.max(1);
                    gfx.surface.configure(&gfx.device, &gfx.config);
                    gfx.renderer
                        .ensure_targets(&gfx.device, gfx.config.width, gfx.config.height);
                }
            }

            WindowEvent::Occluded(occluded) => self.occluded = occluded,

            WindowEvent::Focused(focused) => {
                self.focused = focused;
                // Releasing held keys on focus loss stops the player running
                // forever after an alt-tab.
                if !focused {
                    self.engine.input.release_all();
                    self.set_mouse_capture(false);
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if event.repeat {
                    return;
                }

                // Held movement keys must not stay held while the console
                // has the keyboard, or the player walks the whole time it is
                // open.
                if self.console_ui.open {
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key
                    && let Some(name) = key_name(code)
                    && let Some(command) = self.engine.input.key_event(name, pressed)
                {
                    self.engine.console.execute_user(&command);
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if self.console_ui.open {
                    return;
                }
                if button == MouseButton::Left
                    && state == ElementState::Pressed
                    && !self.mouse_captured
                {
                    // Clicking the window takes the mouse, the way every game
                    // does; escape gives it back.
                    self.set_mouse_capture(true);
                    return;
                }
                if let Some(name) = mouse_button_name(button) {
                    let pressed = state == ElementState::Pressed;
                    if let Some(command) = self.engine.input.key_event(name, pressed) {
                        self.engine.console.execute_user(&command);
                    }
                }
            }

            // The wheel is two keys, `mwheelup` and `mwheeldown`, pressed and
            // let go at once for each notch -- so a `+` binding on one is a tap.
            WindowEvent::MouseWheel { delta, .. } => {
                if self.console_ui.open || !self.mouse_captured {
                    return;
                }
                let notches = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    // A touchpad's pixels: about one notch per 40.
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
                let key = if notches > 0.0 {
                    "mwheelup"
                } else {
                    "mwheeldown"
                };
                for _ in 0..(notches.abs().round() as usize).clamp(1, 8) {
                    for pressed in [true, false] {
                        if let Some(command) = self.engine.input.key_event(key, pressed) {
                            self.engine.console.execute_user(&command);
                        }
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
        if let DeviceEvent::MouseMotion { delta } = event
            && self.mouse_captured
        {
            self.engine
                .input
                .mouse_moved(delta.0 as f32, delta.1 as f32);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.engine.shutdown();
        // Whatever was set this session -- a sensitivity, a field of view,
        // a rebound key -- is written where the next start reads it back.
        // Done here rather than on `quit` alone so that closing the window
        // keeps the settings too.
        match self.engine.write_config() {
            Ok(path) => log::info!("wrote {}", path.display()),
            Err(e) => log::warn!("could not write config.cfg: {e}"),
        }
        if let Err(e) = self.engine.save_console_history() {
            log::warn!("could not keep the console's history: {e}");
        }
    }
}

impl App {
    fn intercept(&self, code: KeyCode) -> Option<Intercepted> {
        intercepted(code, self.console_ui.open)
    }

    /// Open or close the console, and hand the mouse and keyboard over.
    fn toggle_console(&mut self) {
        self.console_ui.toggle();
        if self.console_ui.open {
            // Whatever was held stays held forever otherwise.
            self.engine.input.release_all();
            self.set_mouse_capture(false);
            self.console_ui.greet(&mut self.engine.console);
        }
    }

    /// Escape, with the console closed: the pause menu if there is one,
    /// else just the mouse back.
    fn escape(&mut self) {
        if !self.engine.toggle_pause_menu() {
            self.set_mouse_capture(false);
        }
        self.sync_menu();
    }

    /// Hand the mouse to the UI when a menu opens and back to the game when
    /// it closes, however it closed: Escape, a Resume button, a script.
    fn sync_menu(&mut self) {
        let wants = self.engine.ui_wants_input();
        if wants == self.menu_open {
            return;
        }
        self.menu_open = wants;
        if wants {
            self.engine.input.release_all();
            self.set_mouse_capture(false);
        } else if self.engine.level.is_some() && !self.console_ui.open {
            self.set_mouse_capture(true);
        }
    }

    /// Give a window event to the UI. `true` if it is the UI's.
    fn ui_window_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.engine.ui_input(UiInput::PointerMove {
                    x: position.x as f32,
                    y: position.y as f32,
                });
                true
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if *button == MouseButton::Left {
                    self.engine.ui_input(UiInput::PointerButton {
                        down: *state == ElementState::Pressed,
                    });
                }
                true
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return true;
                }
                let key = match event.physical_key {
                    PhysicalKey::Code(code) => ui_key(code, self.shift_held),
                    _ => None,
                };
                match key {
                    Some(key) => {
                        self.engine.ui_input(UiInput::Key(key));
                    }
                    None => {
                        if let Some(text) = event
                            .text
                            .as_ref()
                            .filter(|t| !t.chars().any(char::is_control))
                        {
                            self.engine.ui_input(UiInput::Text(text.to_string()));
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn set_mouse_capture(&mut self, capture: bool) {
        let Some(gfx) = &self.gfx else { return };
        self.mouse_captured = capture;
        let mode = if capture {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
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

        self.engine.input.update_view(&self.engine.console);
        let input_state = self.engine.input.state();

        // The world stops while something else has the player: the console,
        // or another window. The pause menu and the store's overlay are the
        // engine's to see, and it does.
        let background = !self.focused || self.occluded;
        self.engine
            .set_host_paused(self.console_ui.open || background);
        self.engine.set_background(background);

        self.engine.frame(real_dt, &input_state);
        // Whatever the engine did not claim is the host's: opening the
        // console is the obvious one, and `toggleconsole` being a command
        // rather than only a key is what lets it be bound somewhere else.
        let unhandled = take_console_requests(&mut self.engine);
        let mut leftover = Vec::new();
        for (kind, payload) in unhandled {
            match kind.as_str() {
                kerosene_console::requests::TOGGLE_CONSOLE => self.toggle_console(),
                crate::engine::SCREENSHOT => self.screenshot = true,
                _ => leftover.push((kind, payload)),
            }
        }
        report_unhandled(&mut self.engine, leftover);

        // The store, every frame rather than every tick: the overlay's
        // callbacks must keep flowing while the game is paused under it.
        self.engine.platform_frame(real_dt);

        // The UI, after the ticks it shows the result of.
        if let Some(gfx) = &self.gfx {
            let viewport = (gfx.config.width, gfx.config.height);
            self.engine.ui_frame(real_dt, viewport);
        }
        self.engine.ui.host_actions.clear();
        self.sync_menu();

        if self.engine.should_quit {
            event_loop.exit();
            return;
        }

        // Rebuild GPU resources when a map loads -- by generation, not by
        // name, so `map x` typed after recompiling x shows the new geometry
        // rather than colliding with it while drawing the old.
        let current = self
            .engine
            .level
            .as_ref()
            .map(|_| self.engine.load_generation());
        let loaded = self.map.as_ref().map(|m| m.generation);
        if current != loaded {
            self.rebuild_map();
            // The view is the host's -- it comes from the mouse -- so a new
            // map's spawn facing, a saved game's, or the one a level change
            // carried across, is taken here or never seen.
            self.engine.input.view_angles = self.engine.player.view_angles;
        }
        self.stream_sections();

        if self.occluded {
            // Nothing to draw into that anyone can see. A short sleep, so
            // a minimised game is not a spinning core.
            std::thread::sleep(std::time::Duration::from_millis(20));
        } else {
            self.draw(real_dt);
        }

        // `fps_max`: the loop polls, so without this a display without
        // vsync spins a core drawing frames nobody can see. Slept after
        // the draw, measured from the frame's start, so the cap is a cap
        // on the whole frame rather than on the idle part of it.
        let cap = self.engine.console.float("fps_max");
        if cap > 0.0 {
            let budget = std::time::Duration::from_secs_f32(1.0 / cap);
            let spent = now.elapsed();
            if spent < budget {
                std::thread::sleep(budget - spent);
            }
        }
    }

    fn rebuild_map(&mut self) {
        let (Some(gfx), Some(level)) = (&self.gfx, &self.engine.level) else {
            self.map = None;
            return;
        };

        // Models are per map: a `.kmdl` that failed to load, and was then
        // compiled, gets its retry on the next load rather than on restart.
        self.model_cache.clear();
        self.static_probes.clear();
        let bsp = &level.bsp;
        let probes = GpuProbes::upload(&gfx.device, &gfx.queue, bsp.cubemaps.as_ref());
        // The world section, with the map. Baked at unit exposure:
        // `mat_exposure` is applied by the tone-map pass, per frame, so it is
        // live and is counted once. Folding it in here as well squared it,
        // and froze half of it at load time.
        let atlas = LightmapAtlas::build_for(bsp, 1.0, |f| bsp.face_section(f) == 0);
        let mesh = WorldMesh::build_for(bsp, &atlas, |f| bsp.face_section(f) == 0);
        let summary = format!(
            "{} surfaces, {} triangles, {} materials, lightmap atlas {:.0}% full",
            mesh.surfaces.len(),
            mesh.triangle_count(),
            mesh.materials.len(),
            atlas.occupancy() * 100.0
        );
        let world = upload_section(gfx, &self.engine.vfs, mesh, &atlas, &probes);

        for missing in &world.resources.missing_materials {
            self.engine
                .console
                .warn(format!("missing material: {missing}"));
        }
        self.engine.console.print(summary);
        if atlas.overflowed > 0 {
            self.engine.console.warn(format!(
                "{} faces did not fit the lightmap atlas and draw unlit; \
                 raise their lightmap scale in Chisel or split the map",
                atlas.overflowed
            ));
        }

        let count = bsp.section_count();
        let mut sections: Vec<Option<SectionGpu>> = (0..count).map(|_| None).collect();
        sections[0] = Some(world);
        let (tx, rx) = std::sync::mpsc::channel();
        self.map = Some(LoadedMap {
            generation: self.engine.load_generation(),
            sections,
            building: std::collections::HashSet::new(),
            tx,
            rx,
            probes,
        });
    }

    /// Build, upload and drop streamed sections as the engine's decision
    /// changes. Building happens on a worker thread from the shared BSP;
    /// uploading happens here, where the device is.
    fn stream_sections(&mut self) {
        let (Some(gfx), Some(map), Some(level)) =
            (&self.gfx, self.map.as_mut(), &self.engine.level)
        else {
            return;
        };
        let streaming = &level.streaming;
        if streaming.is_static() {
            return;
        }

        // Start what is wanted and not yet under way.
        for section in 1..map.sections.len() {
            let wanted = streaming.state(section) == crate::streaming::SectionState::Wanted;
            if wanted && map.sections[section].is_none() && !map.building.contains(&section) {
                map.building.insert(section);
                let bsp = Arc::clone(&level.bsp);
                let tx = map.tx.clone();
                let generation = map.generation;
                std::thread::spawn(move || {
                    let keep = |f: usize| bsp.face_section(f) as usize == section;
                    let atlas = LightmapAtlas::build_for(&bsp, 1.0, keep);
                    let mesh = WorldMesh::build_for(&bsp, &atlas, keep);
                    // The receiver is gone when the map has been replaced;
                    // the work is simply discarded.
                    let _ = tx.send(BuiltSection {
                        generation,
                        section,
                        atlas,
                        mesh,
                    });
                });
            }
        }

        // Drop what is no longer resident.
        for section in 1..map.sections.len() {
            if streaming.state(section) == crate::streaming::SectionState::Unloaded
                && map.sections[section].is_some()
            {
                map.sections[section] = None;
                self.engine
                    .console
                    .developer(format!("section {section} unloaded"));
            }
        }

        // Take delivery of what the workers finished.
        let mut arrived = Vec::new();
        while let Ok(built) = map.rx.try_recv() {
            map.building.remove(&built.section);
            if built.generation != map.generation {
                continue;
            }
            if streaming.state(built.section) != crate::streaming::SectionState::Wanted {
                continue;
            }
            self.engine.console.developer(format!(
                "section {} loaded: {} surfaces, {} triangles",
                built.section,
                built.mesh.surfaces.len(),
                built.mesh.triangle_count()
            ));
            let gpu = upload_section(gfx, &self.engine.vfs, built.mesh, &built.atlas, &map.probes);
            map.sections[built.section] = Some(gpu);
            arrived.push(built.section);
        }
        for section in arrived {
            self.engine.section_loaded(section);
        }
    }
}
