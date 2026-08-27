// SPDX-License-Identifier: LGPL-3.0-or-later
//! Input: key bindings and the move commands they produce.
//!
//! Bindings map a key to a *console command string*, exactly as Source does.
//! `bind w +forward` and typing `+forward` at the console are the same thing,
//! which is what makes key binds scriptable and configs portable.
//!
//! Commands starting with `+` are held: pressing the key runs `+forward` and
//! releasing it runs `-forward`. Everything else fires once on press.

use std::collections::HashMap;
use void_console::Console;
use void_math::Angles;

/// The movement and view state one tick consumes.
#[derive(Clone, Copy, Debug, Default)]
pub struct InputState {
    /// Forward/back in `[-1, 1]`.
    pub forward: f32,
    /// Right/left in `[-1, 1]`.
    pub side: f32,
    /// Up/down, for swimming and noclip.
    pub up: f32,
    pub jump: bool,
    pub duck: bool,
    pub attack: bool,
    pub use_key: bool,
    pub view_angles: Angles,
}

/// Which held actions are currently active.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct HeldActions {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub duck: bool,
    pub attack: bool,
    pub use_key: bool,
    pub speed: bool,
}

impl HeldActions {
    fn slot(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "forward" => &mut self.forward,
            "back" => &mut self.back,
            "moveleft" => &mut self.left,
            "moveright" => &mut self.right,
            "jump" => &mut self.jump,
            "duck" => &mut self.duck,
            "attack" => &mut self.attack,
            "use" => &mut self.use_key,
            "speed" => &mut self.speed,
            _ => return None,
        })
    }

    /// Turn held actions into movement axes.
    ///
    /// Opposing keys cancel rather than one winning, which is what makes
    /// counter-strafing work: tapping the opposite direction stops you dead
    /// instead of turning you around.
    pub fn to_input(self, view_angles: Angles) -> InputState {
        InputState {
            forward: (self.forward as i32 - self.back as i32) as f32,
            side: (self.right as i32 - self.left as i32) as f32,
            up: 0.0,
            jump: self.jump,
            duck: self.duck,
            attack: self.attack,
            use_key: self.use_key,
            view_angles,
        }
    }
}

/// Key bindings and the state they produce.
pub struct InputSystem {
    /// Key name to the command it runs.
    bindings: HashMap<String, String>,
    pub held: HeldActions,
    pub view_angles: Angles,
    /// Mouse movement not yet applied, in raw counts.
    pending_mouse: (f32, f32),
}

impl Default for InputSystem {
    fn default() -> Self { Self::new() }
}

impl InputSystem {
    pub fn new() -> Self {
        let mut system = InputSystem {
            bindings: HashMap::new(),
            held: HeldActions::default(),
            view_angles: Angles::ZERO,
            pending_mouse: (0.0, 0.0),
        };
        system.apply_default_bindings();
        system
    }

    /// The bindings a fresh install starts with.
    fn apply_default_bindings(&mut self) {
        for (key, command) in [
            ("w", "+forward"),
            ("s", "+back"),
            ("a", "+moveleft"),
            ("d", "+moveright"),
            ("space", "+jump"),
            ("ctrl", "+duck"),
            ("shift", "+speed"),
            ("mouse1", "+attack"),
            ("e", "+use"),
            ("`", "toggleconsole"),
            ("escape", "cancelselect"),
        ] {
            self.bind(key, command);
        }
    }

    pub fn bind(&mut self, key: &str, command: &str) {
        self.bindings.insert(key.to_lowercase(), command.to_string());
    }

    pub fn unbind(&mut self, key: &str) { self.bindings.remove(&key.to_lowercase()); }

    pub fn binding(&self, key: &str) -> Option<&str> {
        self.bindings.get(&key.to_lowercase()).map(|s| s.as_str())
    }

    pub fn bindings(&self) -> impl Iterator<Item = (&String, &String)> { self.bindings.iter() }

    /// Handle a key going down or up.
    ///
    /// Returns the console command to run, if the binding is a one-shot rather
    /// than a held action.
    pub fn key_event(&mut self, key: &str, pressed: bool) -> Option<String> {
        let command = self.bindings.get(&key.to_lowercase())?.clone();

        if let Some(action) = command.strip_prefix('+') {
            if let Some(slot) = self.held.slot(action) {
                *slot = pressed;
                return None;
            }
            // A `+command` with no movement slot still runs as a command, so a
            // game can add its own held actions.
            return Some(format!("{}{action}", if pressed { '+' } else { '-' }));
        }

        // One-shots fire on press only; firing on release too would double
        // every `impulse` and `toggleconsole`.
        pressed.then_some(command)
    }

    /// Record raw mouse movement, to be applied on the next update.
    pub fn mouse_moved(&mut self, dx: f32, dy: f32) {
        self.pending_mouse.0 += dx;
        self.pending_mouse.1 += dy;
    }

    /// Apply accumulated mouse movement to the view angles.
    ///
    /// Sensitivity is applied to raw counts rather than to a per-second rate,
    /// deliberately: aiming should depend on how far the mouse moved, not on
    /// how long it took to move it. Scaling by frame time here is a classic
    /// mistake that makes aim feel different at different frame rates.
    pub fn update_view(&mut self, console: &Console) {
        let (dx, dy) = std::mem::take(&mut self.pending_mouse);
        if dx == 0.0 && dy == 0.0 { return; }

        let sensitivity = console.float("sensitivity").max(0.001);
        let yaw_scale = console.float("m_yaw");
        let pitch_scale = console.float("m_pitch");
        let invert = if console.bool("m_invert") { -1.0 } else { 1.0 };

        self.view_angles.yaw -= dx * yaw_scale * sensitivity;
        self.view_angles.pitch += dy * pitch_scale * sensitivity * invert;
        self.view_angles = self.view_angles.clamped_view();
    }

    pub fn state(&self) -> InputState { self.held.to_input(self.view_angles) }

    /// Clear every held key.
    ///
    /// Called when focus is lost: without it, alt-tabbing while holding W
    /// leaves the player running forever.
    pub fn release_all(&mut self) {
        self.held = HeldActions::default();
        self.pending_mouse = (0.0, 0.0);
    }

    /// Serialise bindings as console commands, for `config.cfg`.
    pub fn to_config(&self) -> String {
        let mut lines: Vec<String> = self
            .bindings
            .iter()
            .map(|(key, command)| format!("bind \"{key}\" \"{command}\""))
            .collect();
        lines.sort();
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_bindings_set_and_clear_their_action() {
        let mut input = InputSystem::new();
        assert!(input.key_event("w", true).is_none(), "held keys run no command");
        assert!(input.held.forward);
        input.key_event("w", false);
        assert!(!input.held.forward);
    }

    #[test]
    fn one_shot_bindings_fire_on_press_only() {
        let mut input = InputSystem::new();
        input.bind("f5", "save");
        assert_eq!(input.key_event("f5", true).as_deref(), Some("save"));
        assert_eq!(input.key_event("f5", false), None, "releasing must not fire it again");
    }

    #[test]
    fn opposing_keys_cancel() {
        // Counter-strafing depends on this: tapping the opposite direction
        // should stop you, not turn you around.
        let mut input = InputSystem::new();
        input.key_event("w", true);
        input.key_event("s", true);
        assert_eq!(input.state().forward, 0.0);
    }

    #[test]
    fn movement_axes_map_to_the_right_directions() {
        let mut input = InputSystem::new();
        input.key_event("w", true);
        assert_eq!(input.state().forward, 1.0);
        input.key_event("w", false);
        input.key_event("s", true);
        assert_eq!(input.state().forward, -1.0);

        input.key_event("s", false);
        input.key_event("d", true);
        assert_eq!(input.state().side, 1.0);
        input.key_event("d", false);
        input.key_event("a", true);
        assert_eq!(input.state().side, -1.0);
    }

    #[test]
    fn unbound_keys_do_nothing() {
        let mut input = InputSystem::new();
        assert_eq!(input.key_event("f13", true), None);
    }

    #[test]
    fn rebinding_replaces_the_old_command() {
        let mut input = InputSystem::new();
        input.bind("w", "+back");
        input.key_event("w", true);
        assert!(input.held.back && !input.held.forward);
    }

    #[test]
    fn mouse_movement_turns_the_view() {
        let mut console = Console::new();
        console.register_cvar("sensitivity", "3", void_console::ConVarFlags::NONE, "");
        console.register_cvar("m_yaw", "0.022", void_console::ConVarFlags::NONE, "");
        console.register_cvar("m_pitch", "0.022", void_console::ConVarFlags::NONE, "");
        console.register_cvar("m_invert", "0", void_console::ConVarFlags::NONE, "");

        let mut input = InputSystem::new();
        input.mouse_moved(100.0, 0.0);
        input.update_view(&console);
        // Moving the mouse right turns the view right, which is a decreasing
        // yaw in a +Y-is-left world.
        assert!(input.view_angles.yaw < 0.0, "yaw {}", input.view_angles.yaw);

        // 1000 counts at 0.022 deg/count and sensitivity 3 is 66 degrees, so
        // it takes rather more than that to reach the limit.
        input.mouse_moved(0.0, 1000.0);
        input.update_view(&console);
        assert!((input.view_angles.pitch - 66.0).abs() < 0.01, "{}", input.view_angles.pitch);

        input.mouse_moved(0.0, 2000.0);
        input.update_view(&console);
        assert_eq!(input.view_angles.pitch, 89.0, "pitch must clamp at the neck's limit");
    }

    #[test]
    fn mouse_input_is_consumed_once() {
        let mut console = Console::new();
        console.register_cvar("sensitivity", "3", void_console::ConVarFlags::NONE, "");
        console.register_cvar("m_yaw", "0.022", void_console::ConVarFlags::NONE, "");
        console.register_cvar("m_pitch", "0.022", void_console::ConVarFlags::NONE, "");
        console.register_cvar("m_invert", "0", void_console::ConVarFlags::NONE, "");

        let mut input = InputSystem::new();
        input.mouse_moved(50.0, 0.0);
        input.update_view(&console);
        let after_first = input.view_angles.yaw;
        input.update_view(&console);
        assert_eq!(input.view_angles.yaw, after_first, "the same movement must not apply twice");
    }

    #[test]
    fn losing_focus_releases_everything() {
        // Otherwise alt-tabbing while holding W leaves the player running.
        let mut input = InputSystem::new();
        input.key_event("w", true);
        input.key_event("space", true);
        input.release_all();
        assert_eq!(input.held, HeldActions::default());
    }

    #[test]
    fn bindings_round_trip_through_a_config() {
        let input = InputSystem::new();
        let config = input.to_config();
        assert!(config.contains(r#"bind "w" "+forward""#), "{config}");
        assert!(config.lines().count() >= 10);
    }

    #[test]
    fn key_names_are_case_insensitive() {
        let mut input = InputSystem::new();
        input.key_event("W", true);
        assert!(input.held.forward);
    }
}
