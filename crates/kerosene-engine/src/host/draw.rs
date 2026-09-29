// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! One frame: what the host draws, in order.
use kerosene_rhi::wgpu;

use super::*;

/// A drawn prop: its model slot, its name, and its bone palette slot.
type Prop = (usize, String, usize);

/// What one frame draws, gathered before any pass starts.
struct FrameScene {
    camera: Camera,
    /// Brush entities: model index and the pose blended between ticks.
    brush_models: Vec<(usize, Pose)>,
    /// Each drawn prop: its model slot, its name, and its bone palette slot.
    props: Vec<Prop>,
    /// Static props grouped by model: name, first instance, instance count.
    static_ranges: Vec<(String, u32, u32)>,
    lights: Vec<DynamicLight>,
    light_frame: LightFrame,
}

impl App {
    pub(super) fn draw(&mut self, real_dt: f32) {
        // Taken out for the frame, so the passes below can borrow the rest of
        // the app while they use the GPU.
        let Some(mut gfx) = self.gfx.take() else {
            return;
        };
        self.render_frame(&mut gfx, real_dt);
        self.gfx = Some(gfx);
    }

    fn render_frame(&mut self, gfx: &mut Gfx, real_dt: f32) {
        self.apply_display_settings(gfx);
        let frame = match gfx.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                return;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => return,
            Err(_) => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // Before the targets: a change of sample count drops them, and this
        // is where they get remade at the new one.
        gfx.renderer
            .set_msaa(&gfx.device, self.engine.console.int("r_msaa").max(0) as u32);
        gfx.renderer
            .ensure_targets(&gfx.device, gfx.config.width, gfx.config.height);
        // Interpolate between the last two simulation states, so the view is
        // smooth on a display refreshing faster than the tick rate. The
        // angles come straight from the input rather than from the last tick
        // for the same reason: a mouse is read every frame, and looking
        // around at the tick rate is the stutter people notice first.
        let alpha = self.engine.interpolation_alpha();
        let (position, angles, fov) = self.engine.view_camera(alpha);
        let camera = Camera {
            position,
            angles,
            fov,
            aspect: gfx.config.width as f32 / gfx.config.height.max(1) as f32,
            ..Default::default()
        };

        let mut uniform = CameraUniform::from_camera(&camera, self.engine.time);
        uniform.set_lightmaps(self.engine.console.bool("r_lightmap"));
        uniform.set_fullbright(self.engine.console.bool("r_fullbright"));
        uniform.set_bumpmap(self.engine.console.float("r_bumpmap"));
        uniform.set_specular(self.engine.console.float("r_specular"));
        if let Some(level) = &self.engine.level {
            uniform.set_sky_color(level.sky_color);
        }
        gfx.renderer.update_camera(&gfx.queue, &uniform);
        gfx.renderer.update_display(
            &gfx.queue,
            self.engine.console.float("mat_exposure"),
            ToneMapOperator::from_index(self.engine.console.int("mat_tonemap")),
            self.engine.console.float("mat_gamma"),
        );

        let (brush_models, props) = self.gather_models(gfx, alpha);
        let static_ranges = self.upload_props(gfx, &props);
        let (lights, light_frame) = self.gather_lights(gfx, &camera);
        let scene = FrameScene {
            camera,
            brush_models,
            props,
            static_ranges,
            lights,
            light_frame,
        };
        let (line_buffer, line_count) = self.debug_lines(gfx);

        let mut encoder = gfx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        self.prepare_ui(gfx, &mut encoder, &scene.camera);
        self.shadow_passes(gfx, &mut encoder, &scene);
        self.scene_pass(gfx, &mut encoder, &scene, &line_buffer, line_count);
        self.stats.lights = scene.light_frame.drawn.len();
        self.stats.shadow_views = scene.light_frame.shadow_views.len();

        // HDR scene to the swapchain. The UI draws after, over the result,
        // so it is never tone-mapped: a console should be the colour it is.
        gfx.renderer.tonemap(&mut encoder, &view);

        // The game UI over the scene, and the console (egui) over that.
        let list = self.engine.ui.system.display_list();
        if !list.is_empty() {
            gfx.ui_renderer.draw(
                &gfx.device,
                &gfx.queue,
                &mut encoder,
                &view,
                gfx.config.format,
                "screen",
                list,
                false,
            );
        }

        if self.console_ui.open || self.engine.game().is_some_and(|g| g.wants_ui()) {
            draw_ui(
                gfx,
                &mut encoder,
                &view,
                &mut self.console_ui,
                &mut self.engine,
            );
        }

        // What `screenshot` sees is this frame, console and all, copied out
        // before it is handed to the display.
        let shot = std::mem::take(&mut self.screenshot)
            .then(|| Screenshot::copy(gfx, &mut encoder, &frame.texture))
            .flatten();
        gfx.queue.submit(std::iter::once(encoder.finish()));
        if let Some(shot) = shot {
            match shot.save(gfx, &self.engine) {
                Ok(path) => self
                    .engine
                    .console
                    .print(format!("wrote {}", path.display())),
                Err(e) => self.engine.console.error(format!("screenshot: {e}")),
            }
        }
        frame.present();

        self.report_speeds(real_dt);
    }

    /// `r_fullscreen` and `r_vsync`, applied the frame they change.
    fn apply_display_settings(&self, gfx: &mut Gfx) {
        // `r_fullscreen`, the same way.
        let fullscreen = self.engine.console.int("r_fullscreen").clamp(0, 2);
        if gfx.fullscreen != Some(fullscreen) {
            gfx.fullscreen = Some(fullscreen);
            gfx.window
                .set_fullscreen(fullscreen_mode(&gfx.window, fullscreen));
        }

        // `r_vsync`, applied the frame it changes: an options menu's switch
        // takes effect without a restart.
        let present_mode = kerosene_rhi::present_mode(self.engine.console.bool("r_vsync"));
        if gfx.config.present_mode != present_mode {
            gfx.config.present_mode = present_mode;
            gfx.surface.configure(&gfx.device, &gfx.config);
        }
    }

    /// Brush and prop poses and bone palettes for this frame, uploaded.
    /// Returns the brush models and the props to draw.
    fn gather_models(&mut self, gfx: &mut Gfx, alpha: f32) -> (Vec<(usize, Pose)>, Vec<Prop>) {
        // Where each brush entity has got to, blended between the last two
        // ticks so a door or rotating brush sweeps smoothly instead of
        // snapping into place each time it thinks. Collision still traces
        // against the raw current-tick pose; only the drawn position lags by
        // up to one tick's worth of motion, same as the camera does.
        let brush_models = self.engine.interpolated_brush_model_poses(alpha);
        let mut poses = vec![ModelUniform::default(); MAX_MODELS];
        let mut next_slot = 1usize;
        for (model, pose) in &brush_models {
            if *model < MAX_MODELS {
                poses[*model] = ModelUniform::from(*pose);
            }
            next_slot = next_slot.max(model + 1);
        }

        // Physics props each take a model slot and draw where their body is,
        // blended from where it was last tick so motion stays smooth between
        // the 64Hz simulation and a faster display.
        // Each drawn model: its model slot, its name, and its bone palette
        // slot -- 0, the identity, for anything that is not animated.
        let mut props: Vec<Prop> = Vec::new();
        for entity in self.engine.entities.iter() {
            if self.engine.entities.registry.model_role(&entity.classname)
                != Some(ModelRole::Physics)
            {
                continue;
            }
            let Some(name) = entity.fields.text("model") else {
                continue;
            };
            if next_slot >= MAX_MODELS {
                break;
            }
            let (prev_origin, prev_angles) = self
                .engine
                .physics
                .previous_pose(entity.id)
                .unwrap_or((entity.origin, entity.angles));
            let origin = prev_origin.lerp(entity.origin, alpha);
            let angles = prev_angles.slerp(entity.angles, alpha);
            // The probe it reflects is the nearest one its centre can see,
            // chosen the way a world face chooses: by line of sight first.
            let probe = self
                .engine
                .level
                .as_ref()
                .map_or(kerosene_render::NO_PROBE, |level| {
                    kerosene_render::probe_for(&level.bsp, origin)
                });
            poses[next_slot] = ModelUniform::with_probe(Pose::new(origin, angles), probe);
            props.push((next_slot, name.to_string(), 0));
            next_slot += 1;
        }

        // Animated props: a model slot each, and a palette of their pose at
        // the moment being drawn, between ticks like everything else.
        let now = self.engine.render_time(alpha);
        let mut palettes = Vec::new();
        let engine = &mut self.engine;
        for entity in engine.entities.iter() {
            if engine.entities.registry.model_role(&entity.classname) != Some(ModelRole::Animated) {
                continue;
            }
            let Some(name) = entity.fields.text("model") else {
                continue;
            };
            if next_slot >= MAX_MODELS {
                break;
            }
            let probe = match (&engine.level, self.static_probes.get(&entity.id)) {
                (_, Some(&p)) => p,
                (Some(level), None) => {
                    let p = kerosene_render::probe_for(&level.bsp, entity.origin);
                    self.static_probes.insert(entity.id, p);
                    p
                }
                (None, None) => kerosene_render::NO_PROBE,
            };
            poses[next_slot] =
                ModelUniform::with_probe(Pose::new(entity.origin, entity.angles), probe);
            let bones =
                match engine
                    .animations
                    .palette(&engine.vfs, &engine.entities, entity.id, now)
                {
                    Some(palette) if palettes.len() < kerosene_render::gpu::MAX_SKINNED => {
                        palettes.push(palette);
                        palettes.len()
                    }
                    _ => 0,
                };
            props.push((next_slot, name.to_string(), bones));
            next_slot += 1;
        }
        gfx.renderer.update_palettes(&gfx.queue, &palettes);
        gfx.renderer.update_model_uniforms(&gfx.queue, &poses);
        (brush_models, props)
    }

    /// Group the static props by model, upload their instances, and load any
    /// prop model not seen yet. Returns each static model's instance range.
    fn upload_props(&mut self, gfx: &mut Gfx, props: &[Prop]) -> Vec<(String, u32, u32)> {
        // Static props, grouped by model so every copy of one model draws in
        // a single instanced call: a room of forty identical crates is one
        // draw per mesh, not forty.
        let mut static_groups: Vec<(String, Vec<ModelInstance>)> = Vec::new();
        if let Some(level) = &self.engine.level {
            for entity in self.engine.entities.iter() {
                if self.engine.entities.registry.model_role(&entity.classname)
                    != Some(ModelRole::Static)
                {
                    continue;
                }
                let Some(name) = entity.fields.text("model") else {
                    continue;
                };
                let probe = *self
                    .static_probes
                    .entry(entity.id)
                    .or_insert_with(|| kerosene_render::probe_for(&level.bsp, entity.origin));
                let instance = ModelInstance::new(Pose::new(entity.origin, entity.angles), probe);
                match static_groups.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, list)) => list.push(instance),
                    None => static_groups.push((name.into_owned(), vec![instance])),
                }
            }
        }
        let mut static_instances = Vec::new();
        let mut static_ranges: Vec<(String, u32, u32)> = Vec::new();
        for (name, list) in static_groups {
            static_ranges.push((name, static_instances.len() as u32, list.len() as u32));
            static_instances.extend(list);
        }
        gfx.renderer
            .update_instances(&gfx.device, &gfx.queue, &static_instances);

        // Upload any prop model we have not seen yet, once. A failed load is
        // cached as `None` so the warning is not repeated every frame.
        let static_names = static_ranges.iter().map(|(n, _, _)| n);
        for name in props.iter().map(|(_, n, _)| n).chain(static_names) {
            if self.model_cache.contains_key(name) {
                continue;
            }
            let model = load_model(
                &gfx.device,
                &gfx.queue,
                &gfx.renderer,
                &self.engine.vfs,
                name,
            );
            if model.is_none() {
                self.engine.console.warn(format!("missing model: {name}"));
            }
            self.model_cache.insert(name.clone(), model);
        }
        static_ranges
    }

    /// The frame's dynamic lights, binned into clusters with shadow layers.
    fn gather_lights(&self, gfx: &mut Gfx, camera: &Camera) -> (Vec<DynamicLight>, LightFrame) {
        // Dynamic lights: the switched-on light_dynamics, and the flashlight
        // from where the camera is -- the interpolated eye, so it does not
        // lag the view -- binned into clusters and given shadow layers.
        let mut lights = Vec::new();
        if self.engine.console.bool("r_dynamic") && self.engine.level.is_some() {
            lights = crate::lights::entity_lights(&self.engine.entities);
            if self.engine.console.bool("cl_flashlight") {
                lights.push(crate::lights::flashlight(camera.position, camera.angles));
            }
            if !self.engine.console.bool("r_shadows") {
                for light in &mut lights {
                    light.shadows = false;
                }
            }
        }
        let light_frame = LightFrame::build(&lights, camera, gfx.config.width, gfx.config.height);
        gfx.renderer.update_lights(&gfx.queue, &light_frame);
        (lights, light_frame)
    }

    /// Debug overlays as a vertex buffer: the prop boxes when `phys_debug` is
    /// on, and the acoustic rooms and occlusion traces when
    /// `snd_acoustics_debug` is 2.
    fn debug_lines(&mut self, gfx: &mut Gfx) -> (Option<wgpu::Buffer>, u32) {
        let now = self.engine.time;
        let mut debug_lines = self.engine.debug_draw.take(now);
        if self.engine.console.int("phys_debug") >= 1 {
            debug_lines.extend(self.engine.physics.debug_lines());
        }
        if self.engine.console.int("r_stream_debug") >= 1
            && let Some(level) = &self.engine.level
        {
            debug_lines.extend(section_debug_lines(&level.streaming));
        }
        if self.engine.console.int("snd_acoustics_debug") >= 2
            && let Some(level) = &self.engine.level
        {
            debug_lines.extend(crate::acoustics::debug_lines(
                &level.bsp,
                self.engine.player.movement.eye_position(),
                self.engine.audio.tracked_voices(),
            ));
        }
        let line_vertices: Vec<LineVertex> = if !debug_lines.is_empty() {
            debug_lines
                .iter()
                .flat_map(|l| {
                    [
                        LineVertex {
                            position: l.a.to_array(),
                            color: l.color,
                        },
                        LineVertex {
                            position: l.b.to_array(),
                            color: l.color,
                        },
                    ]
                })
                .collect()
        } else {
            Vec::new()
        };
        let line_count = line_vertices.len() as u32;
        let line_buffer = (!line_vertices.is_empty()).then(|| {
            gfx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("debug lines"),
                    contents: bytemuck::cast_slice(&line_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        (line_buffer, line_count)
    }

    /// The UI's side of the frame, before any pass that uses it: glyphs
    /// and images it asked for, world panels drawn into their textures,
    /// and decals cut out of whatever geometry is loaded.
    fn prepare_ui(&mut self, gfx: &mut Gfx, encoder: &mut wgpu::CommandEncoder, camera: &Camera) {
        let ui = &mut self.engine.ui;
        let atlas = &mut ui.system.fonts.atlas;
        if std::mem::take(&mut atlas.dirty) {
            gfx.ui_renderer.upload_atlas(&gfx.queue, &atlas.pixels);
        }
        if std::mem::take(&mut ui.images_stale) {
            gfx.ui_renderer.forget_images();
        }
        gfx.ui_renderer.sync_images(
            &gfx.device,
            &gfx.queue,
            &self.engine.vfs,
            &mut ui.system.images,
        );
        let mut panel_quads = Vec::new();
        for p in &ui.panels {
            if let Some((list, revision)) = ui.system.panel_display(&p.name) {
                gfx.ui_renderer.render_panel(
                    &gfx.device,
                    &gfx.queue,
                    encoder,
                    &p.name,
                    list,
                    revision,
                );
                panel_quads.push(PanelQuad {
                    name: p.name.clone(),
                    corners: p.corners,
                    brightness: p.brightness,
                });
            }
        }
        gfx.ui_renderer
            .retain_panels(|name| ui.panels.iter().any(|p| p.name == name));
        gfx.ui_renderer.prepare_world_panels(
            &gfx.device,
            &gfx.queue,
            camera.view_projection(),
            gfx.renderer.msaa_samples(),
            &panel_quads,
        );
        if let Some(map) = &self.map {
            cut_decals(&mut self.decal_cache, map, &ui.decals);
            let mut draws = Vec::new();
            for decal in &ui.decals.list {
                for (section, vertices) in self.decal_cache.cut.get(&decal.id).into_iter().flatten()
                {
                    draws.push(DecalDraw {
                        section: *section,
                        material: &decal.material,
                        vertices,
                    });
                }
            }
            let revision = ui.decals.revision.wrapping_add(map.generation << 32);
            gfx.decals.prepare(
                &gfx.device,
                &gfx.queue,
                &gfx.renderer,
                &self.engine.vfs,
                revision,
                &draws,
            );
            for missing in std::mem::take(&mut gfx.decals.missing) {
                self.engine
                    .console
                    .warn(format!("decal material {missing} would not load"));
            }
        }
    }

    /// Shadow maps: each layer is the world from one light's point of view,
    /// culled by that light's own PVS and frustum -- the same leaf walk the
    /// camera uses, from somewhere else.
    fn shadow_passes(&self, gfx: &mut Gfx, encoder: &mut wgpu::CommandEncoder, scene: &FrameScene) {
        let FrameScene {
            brush_models,
            props,
            static_ranges,
            lights,
            light_frame,
            ..
        } = scene;
        if let (Some(map), Some(level), Some(world)) = (
            &self.map,
            &self.engine.level,
            self.map.as_ref().and_then(LoadedMap::world),
        ) && self.engine.console.bool("r_drawworld")
        {
            for (layer, (view, light)) in light_frame.shadow_views.iter().enumerate() {
                let frustum = Frustum::from_view_projection(*view);
                let origin = lights[*light].origin;
                let mut pass = gfx.renderer.begin_shadow_pass(encoder, layer);
                for (_, section) in map.loaded() {
                    let surfaces = section.mesh.visible_surfaces(&level.bsp, origin, &frustum);
                    gfx.renderer.draw_world_shadow(
                        &mut pass,
                        layer,
                        &section.resources,
                        &section.mesh,
                        &surfaces,
                        0,
                    );
                }
                for (model, pose) in brush_models {
                    if !world.mesh.model_is_visible(*model, *pose, &frustum) {
                        continue;
                    }
                    if let Some(surfaces) = world.mesh.model_surfaces.get(*model) {
                        gfx.renderer.draw_world_shadow(
                            &mut pass,
                            layer,
                            &world.resources,
                            &world.mesh,
                            surfaces,
                            *model,
                        );
                    }
                }
                for (slot, name, bones) in props {
                    if let Some(model) = self.model_cache.get(name).and_then(Option::as_ref) {
                        gfx.renderer
                            .draw_studio_shadow(&mut pass, layer, model, *slot, *bones);
                    }
                }
                for (name, first, count) in static_ranges {
                    if let Some(model) = self.model_cache.get(name).and_then(Option::as_ref) {
                        gfx.renderer
                            .draw_studio_shadow_instances(&mut pass, layer, model, *first, *count);
                    }
                }
            }
        }
    }

    /// The HDR scene pass: world, decals, brush models, props, translucent
    /// surfaces, world panels and debug lines.
    fn scene_pass(
        &mut self,
        gfx: &mut Gfx,
        encoder: &mut wgpu::CommandEncoder,
        scene: &FrameScene,
        line_buffer: &Option<wgpu::Buffer>,
        line_count: u32,
    ) {
        let FrameScene {
            camera,
            brush_models,
            props,
            static_ranges,
            ..
        } = scene;
        {
            let mut pass = gfx.renderer.begin_scene_pass(
                encoder,
                wgpu::Color {
                    r: 0.02,
                    g: 0.02,
                    b: 0.04,
                    a: 1.0,
                },
            );

            if let (Some(map), Some(level), Some(world)) = (
                &self.map,
                &self.engine.level,
                self.map.as_ref().and_then(LoadedMap::world),
            ) && self.engine.console.bool("r_drawworld")
            {
                let novis = self.engine.console.bool("r_novis");
                let frustum = camera.frustum();
                // Every resident section, each with its own lightmap atlas
                // and so its own frame bind group.
                self.stats = FrameStats::default();
                // Named groups so a RenderDoc capture reads as the frame's
                // structure rather than a list of anonymous draws.
                pass.push_debug_group("world");
                // Kept for the translucent pass, after everything solid.
                let mut section_visible = Vec::new();
                for (_, section) in map.loaded() {
                    let visible = if novis {
                        // Every surface, models included: `r_novis` means
                        // "cull nothing", and the models are drawn below.
                        section.mesh.world_surfaces()
                    } else {
                        section
                            .mesh
                            .visible_surfaces(&level.bsp, camera.position, &frustum)
                    };
                    let drawn = gfx.renderer.draw_world(
                        &mut pass,
                        &section.frame_bind_group,
                        &section.resources,
                        &section.mesh,
                        &visible,
                    );
                    self.stats.draw_calls += drawn.draw_calls;
                    self.stats.triangles += drawn.triangles;
                    self.stats.surfaces_drawn += drawn.surfaces_drawn;
                    self.stats.surfaces_total += drawn.surfaces_total;
                    self.stats.cluster = drawn.cluster;
                    section_visible.push(visible);
                }
                pass.pop_debug_group();

                // Decals, on the surfaces they were cut from, with each
                // section's own lightmap.
                pass.push_debug_group("decals");
                for (index, section) in map.loaded() {
                    let drawn = gfx.decals.draw_section(
                        &gfx.renderer,
                        &mut pass,
                        &section.frame_bind_group,
                        index,
                    );
                    self.stats.draw_calls += drawn.draw_calls;
                    self.stats.triangles += drawn.triangles;
                }
                pass.pop_debug_group();

                // Then the brush entities, each where it has got to.
                // They are not in the world's PVS -- their leaves are
                // their own -- so a leaf walk cannot find them, which is
                // why every door in every map used to be invisible.
                pass.push_debug_group("brush models");
                for (model, pose) in brush_models {
                    if !novis && !world.mesh.model_is_visible(*model, *pose, &frustum) {
                        continue;
                    }
                    let drawn = gfx.renderer.draw_model(
                        &mut pass,
                        &world.frame_bind_group,
                        &world.resources,
                        &world.mesh,
                        *model,
                    );
                    self.stats.draw_calls += drawn.draw_calls;
                    self.stats.triangles += drawn.triangles;
                    self.stats.surfaces_drawn += drawn.surfaces_drawn;
                }
                pass.pop_debug_group();

                // Physics props, at the pose in their model slot.
                pass.push_debug_group("props");
                for (slot, name, bones) in props {
                    if let Some(model) = self.model_cache.get(name).and_then(Option::as_ref) {
                        let drawn = gfx.renderer.draw_studio_model(
                            &mut pass,
                            &world.frame_bind_group,
                            model,
                            *slot,
                            *bones,
                        );
                        self.stats.draw_calls += drawn.draw_calls;
                        self.stats.triangles += drawn.triangles;
                    }
                }
                pass.pop_debug_group();

                // Static props, one instanced draw per model.
                pass.push_debug_group("static props");
                for (name, first, count) in static_ranges {
                    if let Some(model) = self.model_cache.get(name).and_then(Option::as_ref) {
                        let drawn = gfx.renderer.draw_studio_instances(
                            &mut pass,
                            &world.frame_bind_group,
                            model,
                            *first,
                            *count,
                        );
                        self.stats.draw_calls += drawn.draw_calls;
                        self.stats.triangles += drawn.triangles;
                    }
                }
                pass.pop_debug_group();

                // Glass, water and the rest of `$translucent`, over everything
                // solid and back to front, so what is behind it shows.
                pass.push_debug_group("translucent");
                for ((_, section), visible) in map.loaded().zip(&section_visible) {
                    let drawn = gfx.renderer.draw_world_translucent(
                        &mut pass,
                        &section.frame_bind_group,
                        &section.resources,
                        &section.mesh,
                        visible,
                        camera.position,
                    );
                    self.stats.draw_calls += drawn.draw_calls;
                    self.stats.triangles += drawn.triangles;
                }
                for (model, pose) in brush_models {
                    if !novis && !world.mesh.model_is_visible(*model, *pose, &frustum) {
                        continue;
                    }
                    let drawn = gfx.renderer.draw_model_translucent(
                        &mut pass,
                        &world.frame_bind_group,
                        &world.resources,
                        &world.mesh,
                        *model,
                        camera.position,
                    );
                    self.stats.draw_calls += drawn.draw_calls;
                    self.stats.triangles += drawn.triangles;
                }
                pass.pop_debug_group();

                pass.push_debug_group("world panels");
                gfx.ui_renderer.draw_world_panels(&mut pass);
                pass.pop_debug_group();

                // The physics debug overlay, drawn last so it sits on top.
                if let Some(buffer) = &line_buffer {
                    pass.push_debug_group("debug lines");
                    gfx.renderer
                        .draw_lines(&mut pass, &world.frame_bind_group, buffer, line_count);
                    pass.pop_debug_group();
                }

                self.stats.cluster = level.bsp.point_cluster(camera.position);
            }
        }
    }

    /// Periodic `r_speeds` output.
    pub(super) fn report_speeds(&mut self, real_dt: f32) {
        if !self.engine.console.bool("r_speeds") {
            return;
        }
        self.since_report += real_dt;
        if self.since_report < 1.0 {
            return;
        }
        self.since_report = 0.0;

        let s = self.stats;
        let message = format!(
            "{:.0} fps | {}/{} surfaces ({:.0}% culled) | {} tris | {} draws | {} lights, {} shadow views | cluster {}",
            1.0 / real_dt.max(1e-6),
            s.surfaces_drawn,
            s.surfaces_total,
            s.culled_fraction() * 100.0,
            s.triangles,
            s.draw_calls,
            s.lights,
            s.shadow_views,
            s.cluster
        );
        self.engine.console.print(message);
    }
}

/// The UI layer: the game's, then the console over it.
///
/// One egui frame for both, so the console can still be dropped over a
/// game menu and the two never fight for the pointer.
pub(super) fn draw_ui(
    gfx: &mut Gfx,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    console_ui: &mut ConsoleUi,
    engine: &mut Engine,
) {
    let input = gfx.egui_state.take_egui_input(&gfx.window);
    let output = gfx.egui.run(input, |ctx| {
        engine.with_game_mut(|game, engine| {
            if game.wants_ui() {
                game.ui(engine, ctx);
            }
        });
        if console_ui.open {
            crate::console_ui::draw(ctx, console_ui, &mut engine.console);
        }
    });
    gfx.egui_state
        .handle_platform_output(&gfx.window, output.platform_output);

    let triangles = gfx.egui.tessellate(output.shapes, output.pixels_per_point);
    for (id, delta) in &output.textures_delta.set {
        gfx.egui_renderer
            .update_texture(&gfx.device, &gfx.queue, *id, delta);
    }
    let descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [gfx.config.width, gfx.config.height],
        pixels_per_point: output.pixels_per_point,
    };
    gfx.egui_renderer
        .update_buffers(&gfx.device, &gfx.queue, encoder, &triangles, &descriptor);

    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    // Load, not clear: the game is behind it.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
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
