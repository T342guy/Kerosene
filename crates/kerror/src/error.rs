// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! The error types, and the stable string ids they are counted and logged by.

use std::fmt;

/// How bad an error is, and so what the engine does about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Worth a log line and a counter; the frame carries on.
    Warning,
    /// Something failed and was recovered from, or can be retried.
    Recoverable,
    /// The engine cannot continue; [`crate::abort`] shuts it down.
    Fatal,
}

/// Anything kerror can name, count and rank.
pub trait KError: std::error::Error {
    /// The stable id: `kengine…` for the engine, `kgameErr_…` for a game.
    /// What counters, logs and the console key on; unlike the message, it
    /// does not change with the details.
    fn id(&self) -> String;

    fn severity(&self) -> Severity;
}

/// A failure inside the engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("render: {0}")]
    Render(String),
    #[error("audio: {0}")]
    Audio(String),
    #[error("asset: {0}")]
    Asset(String),
    #[error("file system: {0}")]
    Vfs(String),
    #[error("config: {0}")]
    Config(String),
    #[error("script: {0}")]
    Script(String),
    #[error("platform: {0}")]
    Platform(String),
    #[error("thread: {0}")]
    Thread(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl KError for EngineError {
    fn id(&self) -> String {
        let name = match self {
            Self::Render(_) => "RenderError",
            Self::Audio(_) => "AudioError",
            Self::Asset(_) => "AssetError",
            Self::Vfs(_) => "VfsError",
            Self::Config(_) => "ConfigError",
            Self::Script(_) => "ScriptError",
            Self::Platform(_) => "PlatformError",
            Self::Thread(_) => "ThreadError",
            Self::Io(_) => "IoError",
            Self::Other(_) => "Error",
        };
        format!("kengine{name}")
    }

    fn severity(&self) -> Severity {
        match self {
            Self::Render(_) | Self::Platform(_) | Self::Thread(_) => Severity::Fatal,
            _ => Severity::Recoverable,
        }
    }
}

/// A failure raised by game code, named by the game.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{name}: {message}")]
pub struct GameError {
    pub name: String,
    pub message: String,
    pub severity: Severity,
}

impl GameError {
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
            severity: Severity::Recoverable,
        }
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
}

impl KError for GameError {
    fn id(&self) -> String {
        format!("kgameErr_{}", self.name)
    }

    fn severity(&self) -> Severity {
        self.severity
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Warning => "warning",
            Self::Recoverable => "recoverable",
            Self::Fatal => "fatal",
        })
    }
}

/// `Result` with [`EngineError`] as the error.
pub type Result<T, E = EngineError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_ids_carry_the_kengine_prefix() {
        assert_eq!(EngineError::Render("x".into()).id(), "kengineRenderError");
        assert_eq!(EngineError::Other("x".into()).id(), "kengineError");
    }

    #[test]
    fn game_ids_carry_the_game_prefix() {
        assert_eq!(GameError::new("Jam", "stuck").id(), "kgameErr_Jam");
    }

    #[test]
    fn severity_defaults_and_overrides() {
        assert_eq!(EngineError::Render("x".into()).severity(), Severity::Fatal);
        let e = GameError::new("A", "b");
        assert_eq!(e.severity(), Severity::Recoverable);
        assert_eq!(e.with_severity(Severity::Fatal).severity(), Severity::Fatal);
    }

    #[test]
    fn io_errors_convert_and_display() {
        let e: EngineError = std::io::Error::other("disk").into();
        assert_eq!(e.to_string(), "io: disk");
    }
}
