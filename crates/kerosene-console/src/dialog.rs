// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! A message box, for the failures a player would otherwise never see.
//!
//! A game started from a shortcut or a store has no terminal. When it cannot
//! start -- no graphics adapter, a crash on the first frame -- the log says
//! why, and nobody reads the log: the window never opens, and to the player
//! the game did nothing. This puts the reason in front of them.
//!
//! No toolkit is linked for it. Windows has `MessageBoxW`; macOS has
//! `osascript`; Linux desktops have `zenity` or `kdialog`, and a desktop with
//! neither gets the log and stderr, as before. Set `KEROSENE_NO_DIALOG` to
//! turn it off, for a build server or a test that expects to fail.

/// Show `message` in a blocking error box titled `title`, if there is a
/// desktop to show it on. Returns whether a box was shown.
pub fn show_error(title: &str, message: &str) -> bool {
    if std::env::var_os("KEROSENE_NO_DIALOG").is_some() {
        return false;
    }
    native(title, message)
}

#[cfg(windows)]
fn native(title: &str, message: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (title, message) = (wide(title), wide(message));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call,
    // and a null owner window is allowed.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        ) != 0
    }
}

#[cfg(target_os = "macos")]
fn native(title: &str, message: &str) -> bool {
    // Quoted for AppleScript: backslashes and double quotes escaped.
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!(
        "display alert {} message {} as critical",
        quote(title),
        quote(message)
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn native(title: &str, message: &str) -> bool {
    use std::process::{Command, Stdio};
    // No display, no desktop: a server or an ssh session.
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return false;
    }
    let attempts: [(&str, Vec<String>); 2] = [
        (
            "zenity",
            vec![
                "--error".into(),
                "--no-markup".into(),
                format!("--title={title}"),
                format!("--text={message}"),
            ],
        ),
        (
            "kdialog",
            vec![
                "--title".into(),
                title.into(),
                "--error".into(),
                message.into(),
            ],
        ),
    ];
    attempts.into_iter().any(|(program, args)| {
        Command::new(program)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    })
}

#[cfg(not(any(unix, windows)))]
fn native(_: &str, _: &str) -> bool {
    false
}
