// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The handful of building blocks every effect here is made of.
//!
//! Written out rather than pulled in, for the same reason the decoders are:
//! there are three of them, they fit on a page, and a page that is ours can
//! be read when a reverb rings wrong instead of being a black box with a
//! version number.

use std::f32::consts::TAU;

/// A one-pole low-pass: `y = x + (y_prev - x) * a`.
///
/// The cheapest filter there is, and the right one for what it does here --
/// air soaking up highs with distance, a wall muffling a sound behind it --
/// because those are gentle 6 dB/octave slopes in the real world too. Its
/// output is continuous across a coefficient change, which is what lets a
/// cutoff be moved every block without a click.
#[derive(Clone, Copy, Debug)]
pub struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    pub fn new(cutoff_hz: f32, rate: f32) -> OnePole {
        OnePole {
            a: coefficient(cutoff_hz, rate),
            z: 0.0,
        }
    }

    /// Fully open: passes everything through unchanged.
    pub fn open() -> OnePole {
        OnePole { a: 0.0, z: 0.0 }
    }

    pub fn set_cutoff(&mut self, cutoff_hz: f32, rate: f32) {
        self.a = coefficient(cutoff_hz, rate);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z = x + (self.z - x) * self.a;
        self.z
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }

    /// Start from `x` rather than from silence, so a filter switched in on
    /// a sound already playing does not begin with a dip.
    pub fn prime(&mut self, x: f32) {
        self.z = x;
    }
}

/// The feedback coefficient for a one-pole at a cutoff.
///
/// Clamped to the audible band, so a cutoff of zero or of ten times the
/// sample rate gives a silent filter or an open one rather than a NaN.
pub fn coefficient(cutoff_hz: f32, rate: f32) -> f32 {
    let rate = rate.max(1.0);
    let cutoff = if cutoff_hz.is_finite() {
        cutoff_hz.clamp(1.0, rate * 0.49)
    } else {
        rate * 0.49
    };
    (-TAU * cutoff / rate).exp()
}

/// A ring of samples, read some distance behind where it was written.
///
/// Power-of-two sized so the wrap is a mask. Reads and writes are separate
/// steps because the reverb has to read a line before it writes back into it.
#[derive(Clone, Debug)]
pub struct DelayLine {
    buffer: Vec<f32>,
    mask: usize,
    /// The slot the next `write` fills.
    write: usize,
}

impl DelayLine {
    /// A line that can look back at least `max_delay` samples.
    pub fn new(max_delay: usize) -> DelayLine {
        let capacity = (max_delay + 2).next_power_of_two();
        DelayLine {
            buffer: vec![0.0; capacity],
            mask: capacity - 1,
            write: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// The sample written `delay` writes ago, where 1 is the most recent.
    ///
    /// Read before writing a frame, a delay of `d` is the sample from `d`
    /// frames back -- the natural way round for a feedback loop.
    #[inline]
    pub fn read(&self, delay: usize) -> f32 {
        debug_assert!(delay >= 1 && delay <= self.mask);
        self.buffer[self.write.wrapping_sub(delay) & self.mask]
    }

    /// Between two samples, for a delay that is not a whole number.
    #[inline]
    pub fn read_fractional(&self, delay: f32) -> f32 {
        let delay = delay.clamp(1.0, (self.mask - 1) as f32);
        let whole = delay as usize;
        let fraction = delay - whole as f32;
        let a = self.read(whole);
        let b = self.read(whole + 1);
        a + (b - a) * fraction
    }

    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buffer[self.write] = x;
        self.write = (self.write + 1) & self.mask;
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }
}

/// A Schroeder all-pass: every frequency passes at the same level, only
/// smeared in time. Four in a row turn a single click into a dense wash
/// before it ever reaches the reverb's delay lines, which is the difference
/// between a room and a flutter echo.
#[derive(Clone, Debug)]
pub struct Allpass {
    line: DelayLine,
    delay: usize,
    pub gain: f32,
}

impl Allpass {
    pub fn new(delay: usize, gain: f32) -> Allpass {
        let delay = delay.max(1);
        Allpass {
            line: DelayLine::new(delay),
            delay,
            gain,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let delayed = self.line.read(self.delay);
        let v = x + self.gain * delayed;
        self.line.write(v);
        delayed - self.gain * v
    }

    pub fn reset(&mut self) {
        self.line.reset();
    }
}

/// The smallest prime not less than `n`.
///
/// Delay lines whose lengths share a factor ring together at that period,
/// which the ear picks out as a pitch. Primes share nothing.
pub fn next_prime(n: usize) -> usize {
    let mut candidate = n.max(2);
    loop {
        if is_prime(candidate) {
            return candidate;
        }
        candidate += 1;
    }
}

fn is_prime(n: usize) -> bool {
    if n < 2 {
        return false;
    }
    if n.is_multiple_of(2) {
        return n == 2;
    }
    let mut d = 3;
    while d * d <= n {
        if n.is_multiple_of(d) {
            return false;
        }
        d += 2;
    }
    true
}

/// A gain in decibels as a factor.
pub fn db(decibels: f32) -> f32 {
    10.0f32.powf(decibels / 20.0)
}

#[cfg(test)]
mod tests;
