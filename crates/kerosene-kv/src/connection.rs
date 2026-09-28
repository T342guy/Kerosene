// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Entity I/O connections, as a keyvalue value encodes them.

use thiserror::Error;

/// Separator used when a connection's parameter contains a comma.
///
/// Source hit exactly this problem: connections are comma-delimited, and then
/// somebody needed to pass `"1,2,3"` as a parameter. The fix there and here is
/// an alternate delimiter that cannot occur in authored text.
const ALT_DELIM: char = '\x1b';

#[derive(Debug, Error)]
pub enum ParseConnectionError {
    #[error("connection {0:?} has {1} fields; expected 5 (target,input,parameter,delay,times)")]
    WrongFieldCount(String, usize),
    #[error("connection {0:?} has an unreadable delay")]
    BadDelay(String),
}

/// One entity output wired to another entity's input.
///
/// This is the mechanism that makes Source maps behave without scripting: a
/// button's `OnPressed` fires a door's `Open`, after a delay, a set number of
/// times. Chisel edits these directly, and the engine walks them at runtime.
///
/// It lives here, beside the other value encodings, because the same
/// string is written into a `.kmap` and read back out of a compiled map's
/// entity lump: the source format and the runtime share it, and neither
/// should have to link the other to read it.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    /// Output on this entity, e.g. `OnPressed`.
    pub output: String,
    /// `targetname` of the entity to fire at. May name several entities.
    pub target: String,
    /// Input to fire, e.g. `Open`.
    pub input: String,
    /// Parameter passed to the input; often empty.
    pub parameter: String,
    /// Seconds to wait before firing.
    pub delay: f32,
    /// How many times this may fire; `-1` means unlimited.
    pub times_to_fire: i32,
}

impl Connection {
    pub fn new(output: &str, target: &str, input: &str) -> Self {
        Connection {
            output: output.to_string(),
            target: target.to_string(),
            input: input.to_string(),
            parameter: String::new(),
            delay: 0.0,
            times_to_fire: -1,
        }
    }

    pub fn with_delay(mut self, delay: f32) -> Self {
        self.delay = delay;
        self
    }
    pub fn with_parameter(mut self, p: &str) -> Self {
        self.parameter = p.to_string();
        self
    }
    pub fn once(mut self) -> Self {
        self.times_to_fire = 1;
        self
    }

    pub fn is_unlimited(&self) -> bool {
        self.times_to_fire < 0
    }

    pub fn parse(output: &str, value: &str) -> Result<Connection, ParseConnectionError> {
        let delim = if value.contains(ALT_DELIM) {
            ALT_DELIM
        } else {
            ','
        };
        let parts: Vec<&str> = value.split(delim).collect();
        if parts.len() != 5 {
            return Err(ParseConnectionError::WrongFieldCount(
                value.to_string(),
                parts.len(),
            ));
        }
        Ok(Connection {
            output: output.to_string(),
            target: parts[0].trim().to_string(),
            input: parts[1].trim().to_string(),
            parameter: parts[2].to_string(),
            delay: parts[3]
                .trim()
                .parse()
                .map_err(|_| ParseConnectionError::BadDelay(value.to_string()))?,
            // A missing or unreadable count means "forever", which is the
            // forgiving reading and matches the engine default.
            times_to_fire: parts[4].trim().parse().unwrap_or(-1),
        })
    }

    /// Serialise the value half of the KeyValues pair.
    pub fn to_value(&self) -> String {
        use crate::format_float as f;
        // Only reach for the escape delimiter when a comma would be ambiguous.
        let delim = if self.parameter.contains(',') {
            ALT_DELIM
        } else {
            ','
        };
        format!(
            "{}{delim}{}{delim}{}{delim}{}{delim}{}",
            self.target,
            self.input,
            self.parameter,
            f(self.delay),
            self.times_to_fire
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connections_round_trip() {
        let c = Connection::parse("OnPressed", "door1,Open,,0.5,-1").unwrap();
        assert_eq!(c.target, "door1");
        assert_eq!(c.input, "Open");
        assert_eq!(c.delay, 0.5);
        assert!(c.is_unlimited());
        assert_eq!(c.to_value(), "door1,Open,,0.5,-1");
    }

    #[test]
    fn a_parameter_containing_a_comma_survives() {
        // The failure this prevents: a parameter with a comma in it turns one
        // connection into six fields and the wire is silently lost.
        let c = Connection::new("OnTrigger", "logic", "SetValue").with_parameter("1,2,3");
        let encoded = c.to_value();
        let back = Connection::parse("OnTrigger", &encoded).unwrap();
        assert_eq!(back.parameter, "1,2,3");
        assert_eq!(back.target, "logic");
    }

    #[test]
    fn malformed_connections_are_reported_not_guessed() {
        assert!(matches!(
            Connection::parse("OnX", "too,few,fields"),
            Err(ParseConnectionError::WrongFieldCount(_, 3))
        ));
        assert!(matches!(
            Connection::parse("OnX", "a,b,c,notanumber,-1"),
            Err(ParseConnectionError::BadDelay(_))
        ));
    }

    #[test]
    fn missing_fire_count_defaults_to_unlimited() {
        let c = Connection::parse("OnX", "a,b,c,0,").unwrap();
        assert!(c.is_unlimited());
    }
}
