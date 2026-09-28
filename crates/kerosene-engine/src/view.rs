// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What the camera does besides follow the player's eye.
//!
//! Recoil kicks it, an explosion shakes it, a scope narrows it, a cutscene
//! or a death camera takes it somewhere else entirely. All of it is the
//! game's to ask for and the engine's to apply, in one place, so the
//! renderer draws one answer and every effect composes with the others.
//!
//! None of it moves the player or their aim: a punch is on the view only,
//! and a shot still goes where the crosshair was. [`Engine::set_view_angles`]
//! is the exception, and says so.

use crate::engine::Engine;
use kerosene_math::{Angles, Vec3};

/// A camera placed by the game rather than at the player's eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraOverride {
    /// Where it is.
    pub position: Vec3,
    /// Which way it looks.
    pub angles: Angles,
    /// Its field of view, or `None` for the player's.
    pub fov: Option<f32>,
}

/// One shake, fading out over its time.
#[derive(Clone, Copy, Debug)]
struct Shake {
    /// Peak swing, in degrees.
    amplitude: f32,
    /// Swings per second.
    frequency: f32,
    /// Game time it started and how long it lasts.
    start: f32,
    duration: f32,
}

/// The camera's state beyond the player's eye.
#[derive(Clone, Debug, Default)]
pub struct ViewEffects {
    /// A kick added to the view, easing back to nothing.
    punch: Angles,
    shakes: Vec<Shake>,
    /// The field of view being blended to, if the game set one, and how
    /// fast: degrees per second.
    fov_target: Option<(f32, f32)>,
    /// The field of view the blend has reached, or `None` for the player's.
    fov: Option<f32>,
    camera: Option<CameraOverride>,
}

/// How quickly a punch returns, per second: most of the way in a quarter
/// of a second, which reads as a kick rather than a drift.
const PUNCH_RETURN: f32 = 12.0;

impl ViewEffects {
    /// Move the effects on by one tick.
    pub(crate) fn tick(&mut self, now: f32, dt: f32, player_fov: f32) {
        let keep = (-PUNCH_RETURN * dt).exp();
        self.punch = Angles::new(
            self.punch.pitch * keep,
            self.punch.yaw * keep,
            self.punch.roll * keep,
        );
        if self.punch.pitch.abs() + self.punch.yaw.abs() + self.punch.roll.abs() < 0.001 {
            self.punch = Angles::ZERO;
        }
        self.shakes.retain(|s| now < s.start + s.duration);

        let current = self.fov.unwrap_or(player_fov);
        match self.fov_target {
            Some((target, rate)) => {
                let step = rate * dt;
                let next = if (target - current).abs() <= step || rate <= 0.0 {
                    target
                } else {
                    current + step * (target - current).signum()
                };
                self.fov = Some(next);
            }
            None => {
                // Back to the player's own, at the rate it left.
                self.fov = None;
            }
        }
    }

    /// The shake's offset at game time `now`.
    fn shake_at(&self, now: f32) -> Angles {
        let mut out = Angles::ZERO;
        for (i, s) in self.shakes.iter().enumerate() {
            let t = now - s.start;
            let fade = (1.0 - t / s.duration.max(0.001)).clamp(0.0, 1.0);
            let a = s.amplitude * fade;
            let w = std::f32::consts::TAU * s.frequency;
            // Two axes out of step with each other, and each shake out of
            // step with the last, so it wanders instead of nodding.
            let phase = i as f32 * 1.7;
            out.pitch += a * (w * t + phase).sin();
            out.yaw += a * 0.7 * (w * 1.3 * t + phase + 0.9).sin();
        }
        out
    }
}

impl Engine {
    /// Kick the view by `angles` (pitch up is negative), easing back over a
    /// fraction of a second: recoil, a hit, a landing. Kicks add.
    pub fn view_punch(&mut self, angles: Angles) {
        let p = &mut self.view.punch;
        *p = Angles::new(
            p.pitch + angles.pitch,
            p.yaw + angles.yaw,
            p.roll + angles.roll,
        );
    }

    /// Shake the view by up to `amplitude` degrees, `frequency` times a
    /// second, fading out over `seconds`: an explosion, a quake. Shakes add.
    pub fn screen_shake(&mut self, amplitude: f32, frequency: f32, seconds: f32) {
        if amplitude <= 0.0 || seconds <= 0.0 {
            return;
        }
        self.view.shakes.push(Shake {
            amplitude,
            frequency: frequency.max(0.1),
            start: self.time,
            duration: seconds,
        });
    }

    /// Narrow or widen the view to `fov` degrees, reaching it after
    /// `seconds` (zero for at once): a scope, a sprint. `None` goes back to
    /// the player's own `cl_fov`.
    pub fn set_fov_override(&mut self, fov: Option<f32>, seconds: f32) {
        match fov {
            Some(fov) => {
                let fov = fov.clamp(1.0, 170.0);
                let from = self.view.fov.unwrap_or(self.console.float("cl_fov"));
                let rate = match seconds > 0.0 {
                    true => (fov - from).abs() / seconds,
                    false => f32::INFINITY,
                };
                self.view.fov_target = Some((fov, rate));
                if seconds <= 0.0 {
                    self.view.fov = Some(fov);
                }
            }
            None => {
                self.view.fov_target = None;
                self.view.fov = None;
            }
        }
    }

    /// Point the player somewhere: after a teleport, at the start of a
    /// scene. Unlike the other effects this is where they are aiming, not
    /// only what is drawn, and the mouse carries on from it.
    pub fn set_view_angles(&mut self, angles: Angles) {
        let angles = angles.clamped_view();
        self.player.view_angles = angles;
        self.input.view_angles = angles;
        self.view_forced = true;
    }

    /// Take the camera away from the player's eye -- a cutscene, a death
    /// camera, a security monitor the player is looking through -- or with
    /// `None`, give it back. The player still moves as usual unless the game
    /// stops them.
    pub fn set_camera(&mut self, camera: Option<CameraOverride>) {
        self.view.camera = camera;
    }

    /// The camera the game has placed, if any.
    pub fn camera_override(&self) -> Option<CameraOverride> {
        self.view.camera
    }

    /// Where the camera is this frame, which way it looks, and its field of
    /// view, `alpha` of the way from the last tick to the next: the player's
    /// eye with every effect applied, or the game's camera.
    pub fn view_camera(&self, alpha: f32) -> (Vec3, Angles, f32) {
        let player_fov = self.console.float("cl_fov");
        let fov = self.view.fov.unwrap_or(player_fov);
        if let Some(camera) = self.view.camera {
            return (camera.position, camera.angles, camera.fov.unwrap_or(fov));
        }
        let base = self.input.state().view_angles.clamped_view();
        let shake = self.view.shake_at(self.render_time(alpha));
        let p = self.view.punch;
        let angles = Angles::new(
            base.pitch + p.pitch + shake.pitch,
            base.yaw + p.yaw + shake.yaw,
            base.roll + p.roll + shake.roll,
        );
        (self.interpolated_eye(alpha), angles, fov)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineConfig;
    use crate::input::InputState;

    fn ticks(engine: &mut Engine, n: usize) {
        for _ in 0..n {
            engine.tick(1.0 / 64.0, &engine.input.state());
        }
    }

    #[test]
    fn a_punch_kicks_the_view_and_eases_back() {
        let mut engine = Engine::new(&EngineConfig::default());
        engine.view_punch(Angles::new(-4.0, 0.0, 0.0));
        let (_, kicked, _) = engine.view_camera(0.0);
        assert!(kicked.pitch < -3.9);
        ticks(&mut engine, 32);
        let (_, after, _) = engine.view_camera(0.0);
        assert!(
            after.pitch.abs() < 0.05,
            "all but back after half a second: {after:?}"
        );
        ticks(&mut engine, 64);
        assert_eq!(engine.view_camera(0.0).1.pitch, 0.0, "and then exactly");
        assert_eq!(engine.player.view_angles.pitch, 0.0, "the aim never moved");
    }

    #[test]
    fn a_zoom_blends_to_its_fov_and_back() {
        let mut engine = Engine::new(&EngineConfig::default());
        let player_fov = engine.console.float("cl_fov");
        engine.set_fov_override(Some(30.0), 0.25);
        ticks(&mut engine, 4);
        let (_, _, part) = engine.view_camera(0.0);
        assert!(part < player_fov && part > 30.0, "{part}");
        ticks(&mut engine, 32);
        assert_eq!(engine.view_camera(0.0).2, 30.0);
        engine.set_fov_override(None, 0.0);
        assert_eq!(engine.view_camera(0.0).2, player_fov);
    }

    #[test]
    fn set_view_angles_sticks_through_ticks() {
        let mut engine = Engine::new(&EngineConfig::default());
        engine.set_view_angles(Angles::new(10.0, 135.0, 0.0));
        ticks(&mut engine, 4);
        assert_eq!(engine.player.view_angles.yaw, 135.0);
        assert_eq!(engine.view_camera(0.0).1.yaw, 135.0);
        // A tick fed input from elsewhere takes that input's angles, as it
        // always has; the host feeds it the engine's own.
        engine.tick(1.0 / 64.0, &InputState::default());
        assert_eq!(engine.player.view_angles.yaw, 0.0);
    }

    #[test]
    fn a_camera_override_replaces_the_eye_and_a_shake_fades() {
        let mut engine = Engine::new(&EngineConfig::default());
        let shot = CameraOverride {
            position: Vec3::new(1.0, 2.0, 3.0),
            angles: Angles::new(0.0, 90.0, 0.0),
            fov: Some(50.0),
        };
        engine.set_camera(Some(shot));
        assert_eq!(engine.view_camera(0.5), (shot.position, shot.angles, 50.0));
        engine.set_camera(None);

        engine.screen_shake(5.0, 10.0, 0.5);
        ticks(&mut engine, 3);
        let (_, shaking, _) = engine.view_camera(0.0);
        assert!(shaking.pitch != 0.0 || shaking.yaw != 0.0);
        ticks(&mut engine, 64);
        assert_eq!(
            engine.view_camera(0.0).1,
            Angles::ZERO,
            "gone after its time"
        );
    }

    #[test]
    fn a_new_map_ends_the_last_ones_shakes() {
        let mut engine = Engine::new(&EngineConfig::default());
        engine.load_map(crate::base::FALLBACK_MAP).unwrap();
        ticks(&mut engine, 64);
        engine.screen_shake(5.0, 10.0, 2.0);
        engine.view_punch(Angles::new(-4.0, 0.0, 0.0));
        // The clock goes back to the start with the new map.
        engine.load_map(crate::base::FALLBACK_MAP).unwrap();
        ticks(&mut engine, 1);
        assert_eq!(
            engine.view_camera(0.0).1.pitch,
            engine.player.view_angles.pitch
        );
        assert!(engine.view.shakes.is_empty() && engine.view.punch == Angles::ZERO);
    }

    #[test]
    fn a_frame_keeps_a_facing_the_engine_set_mid_frame() {
        let mut engine = Engine::new(&EngineConfig::default());
        // What the host read before the frame: looking along +x.
        let stale = InputState::default();
        engine.set_view_angles(Angles::new(0.0, 90.0, 0.0));
        // A frame long enough for several ticks, all fed the stale input.
        engine.frame(0.1, &stale);
        assert_eq!(engine.player.view_angles.yaw, 90.0);
    }
}
