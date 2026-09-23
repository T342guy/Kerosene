// SPDX-License-Identifier: GPL-3.0-or-later WITH LicenseRef-Kerosene-Exception-1.0
//! Skeletal animation playback.
//!
//! A `.keromdl` carries bones, per-vertex weights and animation clips (see
//! [`kerosene_asset::model`]); this crate turns a clip and a time into the
//! matrices the renderer skins with. It is deliberately small: sampling,
//! a two-clip crossfade, and the bone palette. State machines, IK and
//! retargeting are what a game in a genre that needs them builds on top.
//!
//! Everything here is pure arithmetic on the model's data, so it runs the
//! same headless as it does with a window -- a server can ask where a hand
//! is without drawing anything.

use kerosene_asset::{Animation, Model};
use kerosene_math::{Mat4, Quat, Vec3};

/// The most bones the renderer skins with. A model with more draws its
/// extra bones at their rest pose, with a warning from whoever loads it.
pub const MAX_BONES: usize = 128;

/// One bone's local transform: translation and rotation relative to its
/// parent. No scale, as in the file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };

    pub fn to_mat4(self) -> Mat4 {
        Mat4::from_rotation_translation(self.rotation, self.translation)
    }

    /// Part way from `self` to `other`: `t` 0 is `self`, 1 is `other`.
    pub fn lerp(self, other: Transform, t: f32) -> Transform {
        Transform {
            translation: self.translation.lerp(other.translation, t),
            // Shortest way round, so a blend never spins a bone the long way.
            rotation: if self.rotation.dot(other.rotation) < 0.0 {
                self.rotation.slerp(-other.rotation, t)
            } else {
                self.rotation.slerp(other.rotation, t)
            },
        }
    }
}

/// A model's bone hierarchy and bind pose, ready to animate.
#[derive(Clone, Debug)]
pub struct Skeleton {
    /// Parent of each bone, or -1; parents always come first.
    pub parents: Vec<i32>,
    /// Each bone's rest (bind) transform, relative to its parent.
    pub rest: Vec<Transform>,
    /// Inverse of each bone's bind pose in model space: what takes a vertex
    /// from where it was skinned into the bone's own space.
    pub inverse_bind: Vec<Mat4>,
}

impl Skeleton {
    pub fn of(model: &Model) -> Skeleton {
        let parents: Vec<i32> = model.bones.iter().map(|b| b.parent).collect();
        let rest: Vec<Transform> = model
            .bones
            .iter()
            .map(|b| Transform {
                translation: Vec3::from_array(b.position),
                rotation: Quat::from_array(b.rotation).normalize(),
            })
            .collect();
        let bind = model_space(&parents, &rest);
        Skeleton {
            parents,
            inverse_bind: bind.iter().map(Mat4::inverse).collect(),
            rest,
        }
    }

    pub fn bone_count(&self) -> usize {
        self.parents.len()
    }

    /// The skinning matrices for a pose: each bone's model-space transform
    /// times its inverse bind. The rest pose gives identities.
    pub fn palette(&self, pose: &[Transform]) -> Vec<Mat4> {
        model_space(&self.parents, pose)
            .iter()
            .zip(&self.inverse_bind)
            .map(|(global, inverse)| *global * *inverse)
            .collect()
    }

    /// A bone's position in model space in a pose: where a hand is, for
    /// something to be held in it.
    pub fn bone_position(&self, pose: &[Transform], bone: usize) -> Option<Vec3> {
        model_space(&self.parents, pose)
            .get(bone)
            .map(|m| m.transform_point3(Vec3::ZERO))
    }
}

/// Each bone's transform in model space, parents first.
fn model_space(parents: &[i32], local: &[Transform]) -> Vec<Mat4> {
    let mut out: Vec<Mat4> = Vec::with_capacity(local.len());
    for (i, t) in local.iter().enumerate() {
        let m = t.to_mat4();
        let parent = parents.get(i).copied().unwrap_or(-1);
        out.push(if parent >= 0 && (parent as usize) < out.len() {
            out[parent as usize] * m
        } else {
            m
        });
    }
    out
}

/// Where a clip is at `time` seconds: every bone's local transform,
/// interpolated between the two frames either side. A looping clip wraps;
/// one that does not holds its last frame.
pub fn sample(animation: &Animation, bone_count: usize, time: f32) -> Vec<Transform> {
    let frames = animation.frame_count as usize;
    if frames == 0 || bone_count == 0 {
        return vec![Transform::IDENTITY; bone_count];
    }
    let duration = animation.duration();
    let t = if duration <= 0.0 {
        0.0
    } else if animation.looping {
        time.rem_euclid(duration)
    } else {
        time.clamp(0.0, duration)
    };
    let position = t * animation.fps;
    let a = (position.floor() as usize).min(frames - 1);
    let b = (a + 1).min(frames - 1);
    let blend = position - a as f32;
    let (fa, fb) = (
        animation.frame(a, bone_count),
        animation.frame(b, bone_count),
    );
    (0..bone_count)
        .map(|bone| {
            let key = |f: &[kerosene_asset::BoneKey]| {
                f.get(bone).map_or(Transform::IDENTITY, |k| Transform {
                    translation: Vec3::from_array(k.translation),
                    rotation: Quat::from_array(k.rotation).normalize(),
                })
            };
            key(fa).lerp(key(fb), blend)
        })
        .collect()
}

/// Blend two poses bone by bone.
pub fn blend(a: &[Transform], b: &[Transform], t: f32) -> Vec<Transform> {
    a.iter().zip(b).map(|(x, y)| x.lerp(*y, t)).collect()
}

/// What one animated model is playing: a clip since a time, and, for the
/// moment after a change, the clip it is fading out of.
///
/// Times are the caller's clock -- the engine's game time -- so state is a
/// handful of numbers that can live in entity fields and be saved, rather
/// than an object that has to be ticked.
#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    pub clip: Option<usize>,
    /// When `clip` started.
    pub started: f32,
    /// Playback speed; 1 as authored.
    pub rate: f32,
    /// The clip being faded out, when it started, and when the fade began.
    pub fading: Option<(usize, f32, f32)>,
}

/// How long a change of clip takes to cross-fade, in seconds.
pub const CROSSFADE: f32 = 0.2;

impl Playback {
    pub fn new(clip: Option<usize>, now: f32) -> Playback {
        Playback {
            clip,
            started: now,
            rate: 1.0,
            fading: None,
        }
    }

    /// Switch to another clip, fading from the current one.
    pub fn play(&mut self, clip: usize, now: f32) {
        if let Some(current) = self.clip {
            self.fading = Some((current, self.started, now));
        }
        self.clip = Some(clip);
        self.started = now;
    }

    /// Whether a clip that does not loop has reached its end.
    pub fn finished(&self, model: &Model, now: f32) -> bool {
        self.clip
            .and_then(|c| model.animations.get(c))
            .is_some_and(|a| !a.looping && (now - self.started) * self.rate >= a.duration())
    }

    /// The pose at `now`: the rest pose with no clip, the clip, or the fade
    /// between the old clip and the new.
    pub fn pose(&self, model: &Model, skeleton: &Skeleton, now: f32) -> Vec<Transform> {
        let bones = skeleton.bone_count();
        let at = |clip: usize, started: f32| {
            model
                .animations
                .get(clip)
                .map(|a| sample(a, bones, (now - started) * self.rate))
        };
        let Some(current) = self.clip.and_then(|c| at(c, self.started)) else {
            return skeleton.rest.clone();
        };
        match self.fading {
            Some((old, old_started, fade_start)) if now - fade_start < CROSSFADE => {
                match at(old, old_started) {
                    Some(previous) => blend(
                        &previous,
                        &current,
                        ((now - fade_start) / CROSSFADE).clamp(0.0, 1.0),
                    ),
                    None => current,
                }
            }
            _ => current,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kerosene_asset::{Bone, BoneKey};

    /// A root and an arm 16 units up it, and a clip that turns the arm a
    /// quarter turn about Y over one second.
    fn arm() -> Model {
        let mut m = Model::new();
        let (root, arm) = (m.intern("root"), m.intern("arm"));
        m.bones.push(Bone {
            parent: -1,
            name_offset: root,
            rotation: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        });
        m.bones.push(Bone {
            parent: 0,
            name_offset: arm,
            position: [0.0, 0.0, 16.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
        let mut keys = Vec::new();
        for frame in 0..2 {
            let angle = frame as f32 * std::f32::consts::FRAC_PI_2;
            keys.push(BoneKey {
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
            });
            let q = Quat::from_rotation_y(angle);
            keys.push(BoneKey {
                translation: [0.0, 0.0, 16.0],
                rotation: q.to_array(),
            });
        }
        for (name, looping) in [("swing", true), ("once", false)] {
            m.animations.push(Animation {
                name: name.into(),
                fps: 1.0,
                frame_count: 2,
                looping,
                keys: keys.clone(),
            });
        }
        m
    }

    #[test]
    fn the_rest_pose_skins_to_identity() {
        let m = arm();
        let s = Skeleton::of(&m);
        for p in s.palette(&s.rest) {
            assert!(p.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{p}");
        }
    }

    #[test]
    fn sampling_interpolates_between_frames() {
        let m = arm();
        let half = sample(&m.animations[0], 2, 0.5);
        let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        assert!(half[1].rotation.abs_diff_eq(expected, 1e-4));
    }

    #[test]
    fn a_looping_clip_wraps_and_a_one_shot_holds() {
        let m = arm();
        let swing = sample(&m.animations[0], 2, 1.25);
        let once = sample(&m.animations[1], 2, 1.25);
        assert!(
            swing[1]
                .rotation
                .abs_diff_eq(Quat::from_rotation_y(std::f32::consts::FRAC_PI_8), 1e-4)
        );
        assert!(
            once[1]
                .rotation
                .abs_diff_eq(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), 1e-4)
        );
    }

    #[test]
    fn the_palette_moves_a_vertex_with_its_bone() {
        let m = arm();
        let s = Skeleton::of(&m);
        // A vertex at the arm's tip, 16 above the elbow.
        let tip = Vec3::new(0.0, 0.0, 32.0);
        let pose = sample(&m.animations[1], 2, 1.0);
        let moved = s.palette(&pose)[1].transform_point3(tip);
        // A quarter turn about Y takes +Z to +X, about the elbow at z = 16.
        assert!(
            moved.abs_diff_eq(Vec3::new(16.0, 0.0, 16.0), 1e-3),
            "{moved}"
        );
        assert!(
            s.bone_position(&pose, 1)
                .unwrap()
                .abs_diff_eq(Vec3::new(0.0, 0.0, 16.0), 1e-4)
        );
    }

    #[test]
    fn changing_clip_crossfades_then_settles() {
        let m = arm();
        let s = Skeleton::of(&m);
        let mut p = Playback::new(Some(1), 0.0);
        // At t = 1 "once" is at its end: a quarter turn.
        p.play(0, 1.0);
        // Straight after the change: still all "once".
        let start = p.pose(&m, &s, 1.0)[1].rotation;
        assert!(start.abs_diff_eq(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), 1e-3));
        // After the fade: all "swing", at its start.
        let after = p.pose(&m, &s, 1.0 + CROSSFADE)[1].rotation;
        let swing = sample(&m.animations[0], 2, CROSSFADE)[1].rotation;
        assert!(after.abs_diff_eq(swing, 1e-3));
    }

    #[test]
    fn a_one_shot_reports_when_it_is_done() {
        let m = arm();
        let p = Playback::new(Some(1), 10.0);
        assert!(!p.finished(&m, 10.5));
        assert!(p.finished(&m, 11.0));
        assert!(
            !Playback::new(Some(0), 0.0).finished(&m, 100.0),
            "loops never end"
        );
    }
}
