// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! The room: a feedback delay network that rings the way a space does.
//!
//! A real room is a sound bouncing between its walls, losing a little at
//! each one, and losing more of the highs than the lows because that is what
//! plaster and carpet do. The model here is exactly that, with eight paths
//! instead of infinitely many: eight delay lines, a matrix that scatters
//! each one's output into all the others, and a filter in each loop that
//! takes out per band what the walls would. Every knob on it is a physical
//! quantity -- how long each band takes to die away, how long before the
//! first reflection arrives -- so the compiler can fill it in from geometry
//! and a designer can read it.
//!
//! [`ReverbParams`] is the description; [`Fdn`] is the thing that runs. The
//! description is what the compiler stores per room and the engine hands
//! over per tick; the network is one per mixer, and it changes rooms by
//! sliding rather than jumping, because a jump is a click.

use crate::dsp::{Allpass, DelayLine, OnePole, next_prime};

/// The bands everything acoustic is measured in, in hertz.
///
/// Four, two octaves apart. Materials are tabulated this way, air is
/// modelled this way, and the reverb's loop filter can hold exactly this
/// many independent gains; more would be resolution nothing downstream can
/// use.
pub const BANDS_HZ: [f32; 4] = [125.0, 500.0, 2000.0, 8000.0];

/// How a space sounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbParams {
    /// How long each band takes to fall 60 dB, in seconds, per [`BANDS_HZ`].
    pub rt60: [f32; 4],
    /// Time before the first reflection reaches the ear, in seconds. Longer
    /// reads as bigger: it is the round trip to the nearest wall.
    pub predelay: f32,
    /// How quickly the echoes blur together, 0 to 1. A tiled corridor is
    /// low; a cluttered room is high.
    pub diffusion: f32,
    /// How loud the room is next to the sound itself, 0 to 1.
    pub wet: f32,
    /// Off means the network is bypassed entirely once its tail has gone.
    pub enabled: bool,
}

impl Default for ReverbParams {
    /// No room at all.
    fn default() -> Self {
        ReverbParams {
            rt60: [0.3; 4],
            predelay: 0.01,
            diffusion: 0.5,
            wet: 0.0,
            enabled: false,
        }
    }
}

impl ReverbParams {
    /// A named preset, for auditioning and for `snd_reverb_preset`.
    pub fn preset(name: &str) -> Option<ReverbParams> {
        let p = match name.trim().to_ascii_lowercase().as_str() {
            "room" => ReverbParams {
                rt60: [0.5, 0.45, 0.35, 0.2],
                predelay: 0.008,
                diffusion: 0.6,
                wet: 0.25,
                enabled: true,
            },
            "hall" => ReverbParams {
                rt60: [2.2, 2.0, 1.6, 0.9],
                predelay: 0.03,
                diffusion: 0.8,
                wet: 0.4,
                enabled: true,
            },
            "cave" => ReverbParams {
                rt60: [4.0, 3.5, 2.5, 1.2],
                predelay: 0.05,
                diffusion: 0.4,
                wet: 0.5,
                enabled: true,
            },
            "outdoor" => ReverbParams {
                rt60: [0.3, 0.25, 0.2, 0.12],
                predelay: 0.06,
                diffusion: 0.2,
                wet: 0.08,
                enabled: true,
            },
            _ => return None,
        };
        Some(p)
    }

    /// The names [`ReverbParams::preset`] knows.
    pub const PRESETS: &[&str] = &["room", "hall", "cave", "outdoor"];

    /// Every field brought into the range the network can take.
    pub fn clamped(mut self) -> ReverbParams {
        for t in &mut self.rt60 {
            *t = if t.is_finite() {
                t.clamp(MIN_RT60, MAX_RT60)
            } else {
                MIN_RT60
            };
        }
        self.predelay = if self.predelay.is_finite() {
            self.predelay.clamp(0.0, MAX_PREDELAY)
        } else {
            0.0
        };
        self.diffusion = if self.diffusion.is_finite() {
            self.diffusion.clamp(0.0, 1.0)
        } else {
            0.5
        };
        self.wet = if self.wet.is_finite() {
            self.wet.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self
    }
}

/// The shortest decay the network will model, in seconds.
pub const MIN_RT60: f32 = 0.05;
/// The longest. Past this the difference is inaudible and the loop gain is
/// close enough to one that float error starts to matter.
pub const MAX_RT60: f32 = 20.0;
/// The longest pre-delay, in seconds: the round trip to a wall fifty feet off.
pub const MAX_PREDELAY: f32 = 0.100;

/// How many delay lines.
pub const LINES: usize = 8;

/// Base lengths of the lines in milliseconds. Spread over a two-octave range
/// and not in any simple ratio, then each is rounded up to a prime number of
/// samples so no two share a period.
const LINE_MS: [f32; LINES] = [23.1, 29.7, 37.3, 43.9, 53.1, 61.7, 71.9, 83.3];

/// The input diffusers, in milliseconds.
const ALLPASS_MS: [f32; 4] = [4.77, 3.60, 12.73, 9.31];

/// Which way round each line gets the input. Alternating so the direct sum
/// of the lines cancels rather than piling up at DC.
const INPUT_SIGNS: [f32; LINES] = [1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0];

/// How the lines sum into each ear. Different patterns for the two, so the
/// tail is wide rather than a mono wash in the middle.
const OUT_LEFT: [f32; LINES] = [1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0];
const OUT_RIGHT: [f32; LINES] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0];

const INPUT_SCALE: f32 = 0.5;

/// How long a parameter takes to slide most of the way to a new value, in
/// seconds. Long enough that walking through a doorway is a change of room
/// rather than a switch being thrown; short enough that it is over before
/// you have crossed the next one.
const SMOOTHING_S: f32 = 0.2;

/// How long the old and new pre-delay taps overlap when the pre-delay
/// changes. Moving a tap is a pitch bend; fading between two is not.
const CROSSFADE_S: f32 = 0.2;

/// The corners of the loop filter's two shelves, in hertz. The low shelf
/// carries the 125 Hz band, the high shelf the 8 kHz one, and everything
/// between is the geometric mean of the two middle bands.
const LOW_SHELF_HZ: f32 = 250.0;
const HIGH_SHELF_HZ: f32 = 4000.0;

/// How far past its target a shelf gain is set so the band an octave past
/// the corner lands on it: a one-pole is at `1/sqrt(1 + 2^2)` there.
const SHELF_REACH: f32 = 1.118;

/// Below this the network is silent enough to switch off.
const SILENT_WET: f32 = 1e-4;

/// One line's loop filter: a gain per band, as two shelves around a middle.
///
/// `y = g_mid * x + (g_lo - g_mid) * LP_lo(x) + (g_hi - g_mid) * (x - LP_hi(x))`
///
/// At DC the low-pass passes everything and the high-pass nothing, leaving
/// `g_lo`; at the top the other way round, leaving `g_hi`; between, `g_mid`.
#[derive(Clone, Debug)]
struct ShelfPair {
    low: OnePole,
    high: OnePole,
    g_lo: f32,
    g_mid: f32,
    g_hi: f32,
}

impl ShelfPair {
    fn new(rate: f32) -> ShelfPair {
        ShelfPair {
            low: OnePole::new(LOW_SHELF_HZ, rate),
            high: OnePole::new(HIGH_SHELF_HZ, rate),
            g_lo: 0.0,
            g_mid: 0.0,
            g_hi: 0.0,
        }
    }

    /// Set the gains so a line of `length` seconds decays each band in its
    /// `rt60`: 60 dB over `rt60` seconds is `-3 * length / rt60` bels a trip.
    ///
    /// A one-pole shelf is only most of the way to its shelf gain an octave
    /// past its corner, which is where the outer bands sit, so those two are
    /// pushed a little past their targets to land on them -- and capped at
    /// the gain of the longest decay allowed, so a bright room can never
    /// push a band over unity.
    fn set(&mut self, length: f32, rt60: &[f32; 4]) {
        let g = |t: f32| 10.0f32.powf(-3.0 * length / t.max(MIN_RT60));
        let ceiling = g(MAX_RT60);
        self.g_mid = (g(rt60[1]) * g(rt60[2])).sqrt();
        self.g_lo = (self.g_mid + (g(rt60[0]) - self.g_mid) * SHELF_REACH).min(ceiling);
        self.g_hi = (self.g_mid + (g(rt60[3]) - self.g_mid) * SHELF_REACH).min(ceiling);
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let lo = self.low.process(x);
        let hi = x - self.high.process(x);
        self.g_mid * x + (self.g_lo - self.g_mid) * lo + (self.g_hi - self.g_mid) * hi
    }

    fn reset(&mut self) {
        self.low.reset();
        self.high.reset();
    }
}

#[derive(Clone, Debug)]
struct Line {
    delay: DelayLine,
    length: usize,
    filter: ShelfPair,
}

/// The network.
pub struct Fdn {
    rate: f32,
    lines: Vec<Line>,
    diffusers: Vec<Allpass>,
    predelay: DelayLine,
    /// Pre-delay taps in samples: the one being faded out, the one being
    /// faded in, and how far along the fade is (1 when done).
    tap_from: f32,
    tap_to: f32,
    fade: f32,
    fade_step: f32,
    /// What the game asked for, and where the network actually is.
    target: ReverbParams,
    current: ReverbParams,
    /// Whether there is anything in the lines worth computing. Off, the
    /// network costs nothing and adds nothing -- bit for bit.
    active: bool,
}

impl Fdn {
    pub fn new(rate: u32) -> Fdn {
        let rate = rate.max(1) as f32;
        let lines = LINE_MS
            .iter()
            .map(|ms| {
                let length = next_prime((ms * 1e-3 * rate).round() as usize);
                Line {
                    delay: DelayLine::new(length),
                    length,
                    filter: ShelfPair::new(rate),
                }
            })
            .collect();
        let diffusers = ALLPASS_MS
            .iter()
            .map(|ms| Allpass::new((ms * 1e-3 * rate).round() as usize, 0.5))
            .collect();
        let mut fdn = Fdn {
            rate,
            lines,
            diffusers,
            predelay: DelayLine::new((MAX_PREDELAY * rate) as usize + 2),
            tap_from: 1.0,
            tap_to: 1.0,
            fade: 1.0,
            fade_step: 1.0 / (CROSSFADE_S * rate),
            target: ReverbParams::default(),
            current: ReverbParams::default(),
            active: false,
        };
        fdn.snap();
        fdn
    }

    pub fn params(&self) -> &ReverbParams {
        &self.target
    }

    /// Where the network actually is, part way through a slide.
    pub fn current(&self) -> &ReverbParams {
        &self.current
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The lengths of the lines in samples, for the test that checks they
    /// share no factor.
    pub fn line_lengths(&self) -> Vec<usize> {
        self.lines.iter().map(|l| l.length).collect()
    }

    /// Ask for a room. The network slides there over the next fraction of a
    /// second; if it is silent it goes straight there, since there is
    /// nothing to slide from.
    pub fn set_params(&mut self, params: ReverbParams) {
        self.target = params.clamped();
        if !self.active {
            self.snap();
        }
    }

    /// Jump to the target, and bring the taps and filters with it.
    fn snap(&mut self) {
        self.current = self.target;
        if !self.target.enabled {
            self.current.wet = 0.0;
        }
        self.tap_to = self.tap_samples(self.target.predelay);
        self.tap_from = self.tap_to;
        self.fade = 1.0;
        self.update_filters();
    }

    fn tap_samples(&self, predelay: f32) -> f32 {
        // Plus one: the tap is read after this frame's write, and a delay
        // of one is the sample just written.
        predelay.clamp(0.0, MAX_PREDELAY) * self.rate + 1.0
    }

    fn update_filters(&mut self) {
        let rate = self.rate;
        for line in &mut self.lines {
            line.filter
                .set(line.length as f32 / rate, &self.current.rt60);
        }
        let gain = 0.4 + 0.35 * self.current.diffusion;
        for d in &mut self.diffusers {
            d.gain = gain;
        }
    }

    /// Forget everything in the lines.
    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.delay.reset();
            line.filter.reset();
        }
        for d in &mut self.diffusers {
            d.reset();
        }
        self.predelay.reset();
        self.active = false;
        self.snap();
    }

    /// Slide the parameters one block's worth towards the target. Returns
    /// the wet level at the start of the block, so the caller can ramp it
    /// per sample.
    fn advance(&mut self, frames: usize) -> f32 {
        let k = 1.0 - (-(frames as f32) / self.rate / SMOOTHING_S).exp();
        let wet_start = self.current.wet;
        let wet_target = if self.target.enabled {
            self.target.wet
        } else {
            0.0
        };
        self.current.wet += (wet_target - self.current.wet) * k;
        // Decay times slide geometrically: a second added to a short tail
        // is a different room, added to a long one it is nothing.
        for (c, t) in self.current.rt60.iter_mut().zip(self.target.rt60) {
            let lc = c.ln();
            *c = (lc + (t.ln() - lc) * k).exp();
        }
        self.current.diffusion += (self.target.diffusion - self.current.diffusion) * k;
        self.current.enabled = self.target.enabled;

        let tap = self.tap_samples(self.target.predelay);
        if (tap - self.tap_to).abs() > 0.5 {
            self.tap_from = self.tap_to;
            self.tap_to = tap;
            self.fade = 0.0;
            self.current.predelay = self.target.predelay;
        }
        self.update_filters();
        wet_start
    }

    /// Run the room over one block: a mono send in, wet stereo *added* to
    /// the interleaved buffer.
    ///
    /// Adds rather than replaces because the dry mix is already there and
    /// this is the room on top of it. When the room is off and has died
    /// away, the buffer is not touched at all.
    pub fn process(&mut self, send: &[f32], out: &mut [f32]) {
        let frames = send.len().min(out.len() / 2);
        if frames == 0 {
            return;
        }
        if !self.active {
            if !self.target.enabled || self.target.wet <= 0.0 {
                return;
            }
            self.active = true;
            self.snap();
        }

        let wet_start = self.advance(frames);
        let wet_end = self.current.wet;
        let wet_step = (wet_end - wet_start) / frames as f32;
        let out_scale = 1.0 / (LINES as f32).sqrt();
        let householder = 2.0 / LINES as f32;
        let mut wet = wet_start;
        let mut delayed = [0.0f32; LINES];

        for (frame, &x) in send.iter().enumerate().take(frames) {
            // Pre-delay, crossfading between taps when it moved.
            self.predelay.write(x);
            let x = if self.fade >= 1.0 {
                self.predelay.read_fractional(self.tap_to)
            } else {
                let a = self.predelay.read_fractional(self.tap_from);
                let b = self.predelay.read_fractional(self.tap_to);
                self.fade = (self.fade + self.fade_step).min(1.0);
                a + (b - a) * self.fade
            };

            // Smear it before it reaches the lines.
            let mut diffused = x * INPUT_SCALE;
            for d in &mut self.diffusers {
                diffused = d.process(diffused);
            }

            // Read every line through its loop filter, then scatter the
            // lot back in with a Householder reflection: each line gets its
            // own output minus a share of the sum. Orthogonal, so the loop
            // gain is exactly what the filters say and nothing else.
            let mut sum = 0.0;
            for (i, line) in self.lines.iter_mut().enumerate() {
                delayed[i] = line.filter.process(line.delay.read(line.length));
                sum += delayed[i];
            }
            let mut left = 0.0;
            let mut right = 0.0;
            for (i, line) in self.lines.iter_mut().enumerate() {
                let d = delayed[i];
                line.delay
                    .write(INPUT_SIGNS[i] * diffused + d - householder * sum);
                left += OUT_LEFT[i] * d;
                right += OUT_RIGHT[i] * d;
            }

            wet += wet_step;
            out[frame * 2] += left * out_scale * wet;
            out[frame * 2 + 1] += right * out_scale * wet;
        }

        let wanted = self.target.enabled && self.target.wet > 0.0;
        if !wanted && self.current.wet < SILENT_WET {
            self.reset();
        }
    }
}

#[cfg(test)]
mod tests;
