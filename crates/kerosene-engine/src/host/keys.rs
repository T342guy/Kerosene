// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Keys and mouse buttons: their names, and the ones the host takes itself.

use super::*;

/// A key the host acts on itself, whatever else is on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intercepted {
    ToggleConsole,
    ReleaseMouse,
}

/// What the host must do with a key before anything else sees it.
///
/// Only two keys qualify, and both for the same reason: they are how you get
/// *out* of something. A way out that can be captured by the thing you are
/// trying to leave is not a way out -- which is exactly what happened when
/// these were handled after egui, and the console kept the keys that close
/// it. The console key is deliberately not a binding either, so it still
/// works out of a config whose bindings someone has just broken.
pub fn intercepted(code: KeyCode, console_open: bool) -> Option<Intercepted> {
    match code {
        KeyCode::Backquote => Some(Intercepted::ToggleConsole),
        KeyCode::Escape if console_open => Some(Intercepted::ToggleConsole),
        KeyCode::Escape => Some(Intercepted::ReleaseMouse),
        _ => None,
    }
}

/// A key the UI does something with.
pub(super) fn ui_key(code: KeyCode, shift: bool) -> Option<UiKey> {
    Some(match code {
        KeyCode::Tab if shift => UiKey::BackTab,
        KeyCode::Tab => UiKey::Tab,
        KeyCode::Enter | KeyCode::NumpadEnter => UiKey::Enter,
        KeyCode::Space => UiKey::Space,
        KeyCode::ArrowUp => UiKey::Up,
        KeyCode::ArrowDown => UiKey::Down,
        KeyCode::ArrowLeft => UiKey::Left,
        KeyCode::ArrowRight => UiKey::Right,
        KeyCode::Backspace => UiKey::Backspace,
        _ => return None,
    })
}

/// Map a physical key to the name bindings use.
///
/// Physical rather than logical, so WASD stays where it is on a keyboard laid
/// out differently -- which is what a player on AZERTY expects from a game.
pub(super) fn key_name(code: KeyCode) -> Option<&'static str> {
    use KeyCode::*;
    Some(match code {
        KeyA => "a",
        KeyB => "b",
        KeyC => "c",
        KeyD => "d",
        KeyE => "e",
        KeyF => "f",
        KeyG => "g",
        KeyH => "h",
        KeyI => "i",
        KeyJ => "j",
        KeyK => "k",
        KeyL => "l",
        KeyM => "m",
        KeyN => "n",
        KeyO => "o",
        KeyP => "p",
        KeyQ => "q",
        KeyR => "r",
        KeyS => "s",
        KeyT => "t",
        KeyU => "u",
        KeyV => "v",
        KeyW => "w",
        KeyX => "x",
        KeyY => "y",
        KeyZ => "z",
        Digit0 => "0",
        Digit1 => "1",
        Digit2 => "2",
        Digit3 => "3",
        Digit4 => "4",
        Digit5 => "5",
        Digit6 => "6",
        Digit7 => "7",
        Digit8 => "8",
        Digit9 => "9",
        Space => "space",
        ControlLeft | ControlRight => "ctrl",
        ShiftLeft | ShiftRight => "shift",
        AltLeft | AltRight => "alt",
        Enter => "enter",
        Tab => "tab",
        Backquote => "`",
        Escape => "escape",
        F1 => "f1",
        F2 => "f2",
        F3 => "f3",
        F4 => "f4",
        F5 => "f5",
        F6 => "f6",
        F7 => "f7",
        F8 => "f8",
        F9 => "f9",
        F10 => "f10",
        F11 => "f11",
        F12 => "f12",
        // Source's names, so a config written for it binds the same keys.
        ArrowUp => "uparrow",
        ArrowDown => "downarrow",
        ArrowLeft => "leftarrow",
        ArrowRight => "rightarrow",
        Backspace => "backspace",
        Delete => "del",
        Insert => "ins",
        Home => "home",
        End => "end",
        PageUp => "pgup",
        PageDown => "pgdn",
        CapsLock => "capslock",
        Pause => "pause",
        Minus => "-",
        Equal => "=",
        BracketLeft => "[",
        BracketRight => "]",
        Backslash => "\\",
        Semicolon => "semicolon",
        Quote => "'",
        Comma => ",",
        Period => ".",
        Slash => "/",
        Numpad0 => "kp_ins",
        Numpad1 => "kp_end",
        Numpad2 => "kp_downarrow",
        Numpad3 => "kp_pgdn",
        Numpad4 => "kp_leftarrow",
        Numpad5 => "kp_5",
        Numpad6 => "kp_rightarrow",
        Numpad7 => "kp_home",
        Numpad8 => "kp_uparrow",
        Numpad9 => "kp_pgup",
        NumpadEnter => "kp_enter",
        NumpadAdd => "kp_plus",
        NumpadSubtract => "kp_minus",
        NumpadMultiply => "kp_multiply",
        NumpadDivide => "kp_slash",
        NumpadDecimal => "kp_del",
        _ => return None,
    })
}

pub(super) fn mouse_button_name(button: MouseButton) -> Option<&'static str> {
    Some(match button {
        MouseButton::Left => "mouse1",
        MouseButton::Right => "mouse2",
        MouseButton::Middle => "mouse3",
        MouseButton::Back => "mouse4",
        MouseButton::Forward => "mouse5",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_have_sources_names_and_no_two_share_one() {
        use KeyCode::*;
        assert_eq!(key_name(ArrowUp), Some("uparrow"));
        assert_eq!(key_name(Numpad8), Some("kp_uparrow"));
        assert_eq!(key_name(Semicolon), Some("semicolon"));
        assert_eq!(mouse_button_name(MouseButton::Back), Some("mouse4"));
        let codes = [
            KeyA,
            Digit0,
            ArrowUp,
            ArrowDown,
            ArrowLeft,
            ArrowRight,
            Backspace,
            Delete,
            Insert,
            Home,
            End,
            PageUp,
            PageDown,
            CapsLock,
            Pause,
            Minus,
            Equal,
            BracketLeft,
            BracketRight,
            Backslash,
            Semicolon,
            Quote,
            Comma,
            Period,
            Slash,
            Numpad0,
            Numpad1,
            Numpad2,
            Numpad3,
            Numpad4,
            Numpad5,
            Numpad6,
            Numpad7,
            Numpad8,
            Numpad9,
            NumpadEnter,
            NumpadAdd,
            NumpadSubtract,
            NumpadMultiply,
            NumpadDivide,
            NumpadDecimal,
        ];
        let names: std::collections::HashSet<_> =
            codes.iter().filter_map(|&c| key_name(c)).collect();
        assert_eq!(names.len(), codes.len());
    }

    #[test]
    fn the_console_key_is_intercepted_whether_the_console_is_open_or_not() {
        // Both directions, and before egui sees it. Handled afterwards, a
        // focused text field claims every keystroke: the console kept the key
        // that closes it, and the backtick that opened it was typed into the
        // prompt so that every command after it began with a `.
        assert_eq!(
            intercepted(KeyCode::Backquote, false),
            Some(Intercepted::ToggleConsole)
        );
        assert_eq!(
            intercepted(KeyCode::Backquote, true),
            Some(Intercepted::ToggleConsole)
        );
    }

    #[test]
    fn escape_closes_the_console_when_it_is_open_and_frees_the_mouse_when_it_is_not() {
        assert_eq!(
            intercepted(KeyCode::Escape, true),
            Some(Intercepted::ToggleConsole)
        );
        assert_eq!(
            intercepted(KeyCode::Escape, false),
            Some(Intercepted::ReleaseMouse)
        );
    }

    #[test]
    fn nothing_else_is_taken_from_the_console() {
        // Everything the console needs must reach it, or typing into it is
        // full of holes that are maddening to find.
        for code in [
            KeyCode::KeyW,
            KeyCode::KeyN,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Backspace,
            KeyCode::Space,
            KeyCode::Digit1,
            KeyCode::Semicolon,
        ] {
            assert_eq!(
                intercepted(code, true),
                None,
                "{code:?} must reach the console"
            );
        }
    }
}
