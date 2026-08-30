// SPDX-License-Identifier: LGPL-3.0-or-later
//! Timbre's window.
//!
//! The compiler is a handful of decisions -- how loud, which encoding, where
//! it loops, whether it is mono -- and every one of them is a decision you
//! make better by seeing and hearing the result than by reading a number.
//! A gain of 0.8 means nothing on a command line; the same 0.8 with the
//! waveform redrawn under it, and the clipped samples marked in red, means
//! something immediately.
//!
//! So the window is not a wrapper around the flags. It shows the samples that
//! will actually be written, plays them through the same mixer the engine
//! uses, and writes its settings to the file `timbre build` reads -- which is
//! what stops the two from ever disagreeing about what a build is.

use anyhow::Result;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use timbre::build::Script;
use timbre::Options;
use void_audio::compiled::{Encoding, Loop};
use void_audio::wav::Sound;
use void_audio::{Mixer, SoundHandle, SoundParams};

/// How many columns the waveform is reduced to before drawing.
///
/// A sound is hundreds of thousands of samples and a panel is hundreds of
/// pixels, so something has to summarise. Min and max per column rather than
/// an average: an average of a symmetrical waveform is zero, and the picture
/// it draws is a flat line through the middle of a loud sound.
const WAVE_COLUMNS: usize = 900;

/// One sound found in the tree.
struct Entry {
    /// Absolute, for reading and writing.
    path: PathBuf,
    /// Relative to the sound root, with forward slashes -- the name the build
    /// script keys on and the name worth showing.
    name: String,
    options: Options,
    format: timbre::decode::Format,
    loaded: Option<Loaded>,
    /// What went wrong, if it will not decode at all.
    error: Option<String>,
    compiled: bool,
}

/// A decoded source, and the summary drawn from it.
struct Loaded {
    source: Sound,
    /// The samples as they would be written, options applied.
    prepared: Arc<Sound>,
    /// Per-column min and max of `prepared`, in [-1, 1].
    envelope: Vec<(f32, f32)>,
    peak: f32,
    /// The loop the source itself declares, if any.
    source_loop: Option<Loop>,
}

/// What is currently playing, and when it started.
struct Playing {
    handle: SoundHandle,
    started: Instant,
    rate: u32,
    frames: usize,
}

pub struct Timbre {
    root: PathBuf,
    sound_root: PathBuf,
    script: Script,
    entries: Vec<Entry>,
    selected: Option<usize>,
    filter: String,
    status: String,
    mixer: Option<Arc<Mutex<Mixer>>>,
    /// Kept alive for as long as the window is: dropping it stops the stream.
    _device: Option<void_audio::device::AudioDevice>,
    audio_status: String,
    playing: Option<Playing>,
}

impl Timbre {
    pub fn open(content: &Path) -> Result<Timbre> {
        let sound_root = content.join("sound");
        let script = Script::load_beside(&sound_root)?;

        let (mixer, device, audio_status) = match void_audio::device::AudioDevice::open() {
            Ok(device) => {
                let mixer = Arc::clone(device.mixer());
                let status = format!("{} at {} Hz", device.name(), device.sample_rate());
                (Some(mixer), Some(device), status)
            }
            // A machine with no sound card can still compile sounds; it just
            // cannot audition them, and saying so once is the whole handling.
            Err(e) => (None, None, format!("no audio device: {e}")),
        };

        let mut timbre = Timbre {
            root: content.to_path_buf(),
            sound_root,
            script,
            entries: Vec::new(),
            selected: None,
            filter: String::new(),
            status: String::new(),
            mixer,
            _device: device,
            audio_status,
            playing: None,
        };
        timbre.rescan();
        Ok(timbre)
    }

    /// Find every source sound under the tree and note its state.
    fn rescan(&mut self) {
        let previous = self.selected.and_then(|i| self.entries.get(i)).map(|e| e.name.clone());
        self.entries = timbre::sources(&self.sound_root)
            .into_iter()
            .map(|path| {
                let name = path
                    .strip_prefix(&self.sound_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let options = self.script.options_for(&path, &self.sound_root);
                let compiled = timbre::output_for(&path).is_file();
                let format =
                    timbre::decode::Format::of(&path).unwrap_or(timbre::decode::Format::Wav);
                Entry { path, name, format, options, loaded: None, error: None, compiled }
            })
            .collect();

        self.selected = previous
            .and_then(|name| self.entries.iter().position(|e| e.name == name))
            .or_else(|| (!self.entries.is_empty()).then_some(0));
        if self.entries.is_empty() {
            self.status = format!(
                "no {} files under {}",
                timbre::SOURCE_EXTENSIONS.join(", ."),
                self.sound_root.display()
            );
        }
    }

    /// Decode a sound and build its picture, once.
    fn load(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else { return };
        if entry.loaded.is_some() || entry.error.is_some() {
            return;
        }
        let path = entry.path.clone();
        let options = entry.options;

        let result = std::fs::read(&path)
            .map_err(|e| format!("{e}"))
            .and_then(|bytes| {
                let read = timbre::decode::any(&path, &bytes).map_err(|e| format!("{e:#}"))?;
                Ok((read.sound, read.looping, read.format))
            });

        let Some(entry) = self.entries.get_mut(index) else { return };
        match result {
            Ok((source, source_loop, format)) => {
                entry.format = format;
                entry.loaded = Some(Loaded::build(source, source_loop, &options));
            }
            Err(e) => entry.error = Some(e),
        }
    }

    /// Rebuild the prepared samples after a setting changed.
    fn refresh(&mut self, index: usize) {
        let Some(entry) = self.entries.get_mut(index) else { return };
        let options = entry.options;
        if let Some(loaded) = entry.loaded.take() {
            entry.loaded = Some(Loaded::build(loaded.source, loaded.source_loop, &options));
        }
    }

    /// Record a setting change and write the script.
    fn settings_changed(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else { return };
        let (name, options) = (entry.name.clone(), entry.options);
        self.script.set(&name, options);
        self.refresh(index);
        match self.script.save() {
            Ok(()) => self.status = format!("{name}: settings saved"),
            Err(e) => self.status = format!("could not save settings: {e:#}"),
        }
    }

    fn play(&mut self, index: usize) {
        self.stop();
        let Some(mixer) = self.mixer.clone() else {
            self.status = self.audio_status.clone();
            return;
        };
        self.load(index);
        let Some(entry) = self.entries.get(index) else { return };
        let Some(loaded) = &entry.loaded else { return };

        let sound = Arc::clone(&loaded.prepared);
        let frames = sound.frames();
        let rate = sound.sample_rate;
        // Centred on the listener with no attenuation: this is an audition,
        // not a placement, and hearing it quieter than it is would be a lie
        // about the gain being set.
        let params = SoundParams { volume: 1.0, ..SoundParams::default() };
        let handle = mixer.lock().map(|mut m| m.play(sound, params));
        match handle {
            Ok(handle) => {
                self.playing = Some(Playing { handle, started: Instant::now(), rate, frames });
            }
            Err(_) => self.status = "the mixer is wedged".into(),
        }
    }

    fn stop(&mut self) {
        if let (Some(playing), Some(mixer)) = (self.playing.take(), self.mixer.clone())
            && let Ok(mut mixer) = mixer.lock()
        {
            mixer.stop(playing.handle);
        }
    }

    /// Where playback has got to, in frames, if anything is playing.
    ///
    /// From the wall clock rather than from the mixer: the mixer's cursor is
    /// behind a lock held by the audio thread, and a playhead that is a
    /// buffer's length out is a playhead nobody can tell is wrong.
    fn playhead(&self) -> Option<usize> {
        let playing = self.playing.as_ref()?;
        let elapsed = playing.started.elapsed().as_secs_f32();
        let frame = (elapsed * playing.rate as f32) as usize;
        (frame < playing.frames).then_some(frame)
    }

    fn compile_one(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else { return };
        let (path, options, name) = (entry.path.clone(), entry.options, entry.name.clone());
        let output = timbre::output_for(&path);
        match timbre::compile(&path, &output, &options) {
            Ok(done) => {
                self.status = format!("{done}");
                if let Some(entry) = self.entries.get_mut(index) {
                    entry.compiled = true;
                }
            }
            Err(e) => self.status = format!("{name}: {e:#}"),
        }
    }

    fn compile_all(&mut self) {
        match timbre::build_sounds(&self.root, true) {
            Ok(batch) => {
                self.status = format!("{batch}");
                for entry in &mut self.entries {
                    entry.compiled = timbre::output_for(&entry.path).is_file();
                }
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }
}

impl Loaded {
    fn build(source: Sound, source_loop: Option<Loop>, options: &Options) -> Loaded {
        let prepared = timbre::prepare(&source, options);
        let peak = timbre::peak_of(&prepared);
        let envelope = envelope_of(&prepared, WAVE_COLUMNS);
        Loaded { source, prepared: Arc::new(prepared), envelope, peak, source_loop }
    }
}

/// Reduce a sound to per-column extremes, mixed down to one trace.
fn envelope_of(sound: &Sound, columns: usize) -> Vec<(f32, f32)> {
    let frames = sound.frames();
    if frames == 0 || columns == 0 {
        return Vec::new();
    }
    let channels = sound.channels.max(1) as usize;
    let per = frames.div_ceil(columns).max(1);

    (0..frames.div_ceil(per))
        .map(|column| {
            let first = column * per;
            let last = (first + per).min(frames);
            let mut low = f32::MAX;
            let mut high = f32::MIN;
            for frame in first..last {
                for channel in 0..channels {
                    let s = sound.samples[frame * channels + channel];
                    low = low.min(s);
                    high = high.max(s);
                }
            }
            (low, high)
        })
        .collect()
}

impl void_ui::App for Timbre {
    fn window_title(&self) -> String {
        match self.selected.and_then(|i| self.entries.get(i)) {
            Some(entry) => format!("{} -- Timbre", entry.name),
            None => "Timbre -- VoidEngine sound compiler".into(),
        }
    }

    /// Only while something is playing: the playhead and the meter move, and
    /// nothing else in this window ever does on its own.
    fn wants_continuous_redraw(&self) -> bool {
        self.playing.is_some()
    }

    fn ui(&mut self, ctx: &egui::Context) {
        // Playback finishing is not an event anyone sends us, so it is noticed
        // here rather than waited for.
        if self.playing.is_some() && self.playhead().is_none() {
            self.stop();
        }

        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Timbre");
                ui.separator();
                if ui.button("rescan").clicked() {
                    self.rescan();
                }
                if ui.button("compile all").clicked() {
                    self.compile_all();
                }
                ui.separator();
                ui.label(format!("{} sound(s)", self.entries.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(&self.audio_status).weak());
                });
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(if self.status.is_empty() { "ready" } else { &self.status });
            });
        });

        self.file_list(ctx);
        self.detail(ctx);
    }
}

impl Timbre {
    fn file_list(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("files")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("search")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                ui.separator();

                let filter = self.filter.to_lowercase();
                let mut choose = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, entry) in self.entries.iter().enumerate() {
                        if !filter.is_empty() && !entry.name.to_lowercase().contains(&filter) {
                            continue;
                        }
                        let selected = self.selected == Some(i);
                        // The dot says whether a compiled form exists at all,
                        // which is the one thing worth seeing without clicking.
                        let mark = if entry.error.is_some() {
                            "!"
                        } else if entry.compiled {
                            "\u{2022}"
                        } else {
                            "\u{25e6}"
                        };
                        let label = format!("{mark}  {}", entry.name);
                        if ui.selectable_label(selected, label).clicked() {
                            choose = Some(i);
                        }
                    }
                });
                if let Some(i) = choose {
                    self.selected = Some(i);
                    self.load(i);
                }
            });
    }

    fn detail(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(index) = self.selected else {
                ui.centered_and_justified(|ui| {
                    ui.label("No sound selected.");
                });
                return;
            };
            self.load(index);

            let Some(entry) = self.entries.get(index) else { return };
            let name = entry.name.clone();
            ui.heading(&name);

            if let Some(error) = entry.error.clone() {
                ui.colored_label(Color32::from_rgb(220, 110, 100), error);
                return;
            }
            let Some(loaded) = &entry.loaded else { return };

            let (channels, rate, frames, peak) = (
                loaded.prepared.channels,
                loaded.prepared.sample_rate,
                loaded.prepared.frames(),
                loaded.peak,
            );
            let source_format = entry.format;
            ui.label(
                egui::RichText::new(format!(
                    "{}  \u{2022}  {:.2}s  \u{2022}  {rate} Hz  \u{2022}  {channels} channel{}  \u{2022}  {frames} frames",
                    source_format.name().to_uppercase(),
                    loaded.prepared.duration(),
                    if channels == 1 { "" } else { "s" },
                ))
                .weak(),
            );
            if source_format.is_lossy() {
                ui.colored_label(
                    Color32::from_rgb(228, 186, 92),
                    "already lossy: compiling this further compounds the artifacts rather than \
                     cancelling them. A lossless source makes a better build.",
                );
            }

            ui.add_space(6.0);
            self.waveform(ui, index);
            ui.add_space(6.0);
            self.meters(ui, index, peak);
            ui.add_space(10.0);
            self.controls(ui, index, channels);
        });
    }

    /// The waveform, the loop region, the playhead and the clipping.
    fn waveform(&mut self, ui: &mut egui::Ui, index: usize) {
        let height = 150.0;
        let (response, painter) =
            ui.allocate_painter(Vec2::new(ui.available_width(), height), Sense::click());
        let rect = response.rect;
        painter.rect_filled(rect, 2.0, Color32::from_rgb(18, 22, 26));

        let Some(entry) = self.entries.get(index) else { return };
        let Some(loaded) = &entry.loaded else { return };
        if loaded.envelope.is_empty() {
            return;
        }

        let mid = rect.center().y;
        let half = rect.height() * 0.5 - 4.0;
        let columns = loaded.envelope.len() as f32;

        // The loop region, behind the wave, so it reads as a place rather than
        // as a line drawn over it.
        let region = entry.options.looping.or(loaded.source_loop).unwrap_or_default();
        if !region.is_empty() {
            let frames = loaded.prepared.frames().max(1) as f32;
            let x0 = rect.left() + rect.width() * (region.start as f32 / frames);
            let x1 = rect.left() + rect.width() * (region.end as f32 / frames);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(x1, rect.bottom())),
                0.0,
                Color32::from_rgba_unmultiplied(90, 140, 200, 28),
            );
        }

        painter.line_segment(
            [Pos2::new(rect.left(), mid), Pos2::new(rect.right(), mid)],
            Stroke::new(1.0, Color32::from_rgb(40, 48, 56)),
        );

        for (i, &(low, high)) in loaded.envelope.iter().enumerate() {
            let x = rect.left() + rect.width() * (i as f32 / columns);
            // Anything that reached full scale is drawn in red, because that
            // is what a gain slider needs to tell you and a number cannot.
            let clipped = high >= 0.999 || low <= -0.999;
            let colour = if clipped {
                Color32::from_rgb(226, 96, 88)
            } else {
                Color32::from_rgb(120, 190, 150)
            };
            painter.line_segment(
                [
                    Pos2::new(x, mid - high.clamp(-1.0, 1.0) * half),
                    Pos2::new(x, mid - low.clamp(-1.0, 1.0) * half),
                ],
                Stroke::new(1.0, colour),
            );
        }

        if let Some(frame) = self.playhead() {
            let at = frame as f32 / loaded.prepared.frames().max(1) as f32;
            let x = rect.left() + rect.width() * at;
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.5, Color32::from_rgb(240, 220, 130)),
            );
        }

        painter.rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, Color32::from_rgb(48, 56, 64)),
            StrokeKind::Inside,
        );
    }

    /// Peak of the whole sound, and the level under the playhead.
    fn meters(&mut self, ui: &mut egui::Ui, index: usize, peak: f32) {
        let live = self.level_now(index);
        ui.horizontal(|ui| {
            ui.label("peak");
            meter_bar(ui, peak, 190.0);
            ui.label(format!("{:.2}  ({})", peak, decibels(peak)));
            ui.separator();
            ui.label("level");
            meter_bar(ui, live, 190.0);
        });
        if peak >= 0.999 {
            ui.colored_label(
                Color32::from_rgb(226, 96, 88),
                "clipping: samples are hitting full scale and will distort",
            );
        }
    }

    /// The loudest sample near the playhead, or nothing when stopped.
    fn level_now(&self, index: usize) -> f32 {
        let Some(frame) = self.playhead() else { return 0.0 };
        let Some(entry) = self.entries.get(index) else { return 0.0 };
        let Some(loaded) = &entry.loaded else { return 0.0 };
        // A twentieth of a second either side: short enough to follow a
        // transient, long enough not to flicker at the frame rate.
        let window = (loaded.prepared.sample_rate / 20).max(1) as usize;
        let channels = loaded.prepared.channels.max(1) as usize;
        let first = frame.saturating_sub(window) * channels;
        let last = ((frame + window) * channels).min(loaded.prepared.samples.len());
        loaded.prepared.samples[first.min(last)..last]
            .iter()
            .fold(0.0f32, |a, s| a.max(s.abs()))
    }

    fn controls(&mut self, ui: &mut egui::Ui, index: usize, channels: u16) {
        let Some(entry) = self.entries.get_mut(index) else { return };
        let mut changed = false;
        let mut options = entry.options;

        ui.horizontal(|ui| {
            let playing = self.playing.is_some();
            if ui.button(if playing { "\u{25a0} stop" } else { "\u{25b6} play" }).clicked() {
                if playing { self.stop() } else { self.play(index) }
            }
            ui.separator();
            if ui.button("compile").clicked() {
                self.compile_one(index);
            }
        });

        ui.add_space(8.0);
        egui::Grid::new("options").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
            ui.label("gain");
            // In decibels, because that is the unit gain is thought in, while
            // the value stored stays a plain multiplier.
            let mut db = decibels_value(options.gain);
            if ui
                .add(egui::Slider::new(&mut db, -24.0..=12.0).suffix(" dB").fixed_decimals(1))
                .changed()
            {
                options.gain = 10f32.powf(db / 20.0);
                changed = true;
            }
            ui.end_row();

            ui.label("encoding");
            ui.horizontal(|ui| {
                changed |= ui
                    .radio_value(&mut options.encoding, Encoding::Adpcm, "ADPCM")
                    .on_hover_text("A quarter the size. Close to transparent on impacts and speech.")
                    .changed();
                changed |= ui
                    .radio_value(&mut options.encoding, Encoding::Pcm16, "PCM 16")
                    .on_hover_text("Every bit kept. For quiet, exposed material where ADPCM is audible.")
                    .changed();
            });
            ui.end_row();

            ui.label("channels");
            ui.horizontal(|ui| {
                if channels == 1 && !options.mono {
                    ui.label(egui::RichText::new("mono already").weak());
                } else {
                    changed |= ui
                        .checkbox(&mut options.mono, "fold to mono")
                        .on_hover_text(
                            "A stereo sound cannot be placed in the world: there is one pan and \
                             two channels already carrying their own image.",
                        )
                        .changed();
                }
            });
            ui.end_row();
        });

        if let Some(entry) = self.entries.get_mut(index)
            && changed
        {
            entry.options = options;
        }
        if changed {
            self.settings_changed(index);
        }
    }
}

/// A bar from 0 to full scale, turning red where it would clip.
fn meter_bar(ui: &mut egui::Ui, level: f32, width: f32) {
    let (response, painter) = ui.allocate_painter(Vec2::new(width, 14.0), Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, 2.0, Color32::from_rgb(24, 28, 33));

    let level = level.clamp(0.0, 1.0);
    if level > 0.0 {
        let filled = Rect::from_min_size(rect.min, Vec2::new(rect.width() * level, rect.height()));
        let colour = if level >= 0.999 {
            Color32::from_rgb(226, 96, 88)
        } else if level > 0.9 {
            Color32::from_rgb(228, 186, 92)
        } else {
            Color32::from_rgb(120, 190, 150)
        };
        painter.rect_filled(filled, 2.0, colour);
    }
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, Color32::from_rgb(48, 56, 64)),
        StrokeKind::Inside,
    );
}

/// A linear amplitude as decibels, for display.
fn decibels(level: f32) -> String {
    if level <= 0.0 {
        return "-inf dB".to_string();
    }
    format!("{:.1} dB", decibels_value(level))
}

fn decibels_value(level: f32) -> f32 {
    if level <= 0.0 { -96.0 } else { 20.0 * level.log10() }
}

#[cfg(test)]
mod tests;
