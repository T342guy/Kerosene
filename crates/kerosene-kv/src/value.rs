// SPDX-License-Identifier: MPL-2.0
//! Typed conversions for KeyValues strings.
//!
//! Map and material files write numbers, flags and vectors as text, in a
//! handful of shapes that all have to be accepted: `"64"`, `"0 64 128"`,
//! `"[1 .5 .25]"` (materials), `"(0 0 64)"` (brush plane points).

use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseValueError {
    #[error("{value:?} is not a valid {expected}")]
    Malformed { value: String, expected: &'static str },
}

fn malformed(value: &str, expected: &'static str) -> ParseValueError {
    ParseValueError::Malformed { value: value.to_string(), expected }
}

/// A value readable out of a KeyValues string.
pub trait FromKvValue: Sized {
    fn from_kv(s: &str) -> Result<Self, ParseValueError>;
}

/// A value writable into a KeyValues string.
pub trait ToKvValue {
    fn to_kv(&self) -> String;
}

macro_rules! numeric {
    ($($t:ty => $name:literal),* $(,)?) => {$(
        impl FromKvValue for $t {
            fn from_kv(s: &str) -> Result<Self, ParseValueError> {
                let t = s.trim();
                t.parse::<$t>()
                    // Map files are full of integers written as "64.000000",
                    // so an integer parse falls back to a float parse.
                    .or_else(|_| t.parse::<f64>().map(|f| f as $t))
                    .map_err(|_| malformed(s, $name))
            }
        }
        impl ToKvValue for $t {
            fn to_kv(&self) -> String { self.to_string() }
        }
    )*};
}

numeric!(i32 => "integer", u32 => "integer", i64 => "integer", u64 => "integer", usize => "integer");

impl FromKvValue for f32 {
    fn from_kv(s: &str) -> Result<Self, ParseValueError> {
        s.trim().parse::<f32>().map_err(|_| malformed(s, "number"))
    }
}

impl ToKvValue for f32 {
    fn to_kv(&self) -> String { format_float(*self) }
}

impl FromKvValue for bool {
    fn from_kv(s: &str) -> Result<Self, ParseValueError> {
        match s.trim() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" | "" => Ok(false),
            // Anything numeric and non-zero counts as set, which is how the
            // engine has always read spawnflag-ish keys.
            other => other.parse::<f64>().map(|v| v != 0.0).map_err(|_| malformed(s, "boolean")),
        }
    }
}

impl ToKvValue for bool {
    fn to_kv(&self) -> String { if *self { "1".into() } else { "0".into() } }
}

impl FromKvValue for String {
    fn from_kv(s: &str) -> Result<Self, ParseValueError> { Ok(s.to_string()) }
}

impl ToKvValue for String {
    fn to_kv(&self) -> String { self.clone() }
}

impl ToKvValue for &str {
    fn to_kv(&self) -> String { (*self).to_string() }
}

/// Three floats, however the file chose to punctuate them.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Vec3Value(pub [f32; 3]);

impl Vec3Value {
    pub fn x(&self) -> f32 { self.0[0] }
    pub fn y(&self) -> f32 { self.0[1] }
    pub fn z(&self) -> f32 { self.0[2] }
    pub fn to_array(self) -> [f32; 3] { self.0 }
}

impl FromKvValue for Vec3Value {
    fn from_kv(s: &str) -> Result<Self, ParseValueError> {
        let cleaned: String = s
            .chars()
            .map(|c| if matches!(c, '[' | ']' | '(' | ')' | ',') { ' ' } else { c })
            .collect();
        let parts: Vec<&str> = cleaned.split_whitespace().collect();
        if parts.len() != 3 { return Err(malformed(s, "3-component vector")); }
        let mut out = [0.0f32; 3];
        for (i, p) in parts.iter().enumerate() {
            out[i] = p.parse().map_err(|_| malformed(s, "3-component vector"))?;
        }
        Ok(Vec3Value(out))
    }
}

impl ToKvValue for Vec3Value {
    fn to_kv(&self) -> String {
        format!("{} {} {}", format_float(self.0[0]), format_float(self.0[1]), format_float(self.0[2]))
    }
}

impl fmt::Display for Vec3Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.to_kv()) }
}

impl FromKvValue for [f32; 4] {
    fn from_kv(s: &str) -> Result<Self, ParseValueError> {
        let cleaned: String = s
            .chars()
            .map(|c| if matches!(c, '[' | ']' | '(' | ')' | ',') { ' ' } else { c })
            .collect();
        let parts: Vec<&str> = cleaned.split_whitespace().collect();
        if parts.len() != 4 { return Err(malformed(s, "4-component vector")); }
        let mut out = [0.0f32; 4];
        for (i, p) in parts.iter().enumerate() {
            out[i] = p.parse().map_err(|_| malformed(s, "4-component vector"))?;
        }
        Ok(out)
    }
}

/// Print a float tersely but losslessly enough for a map file.
///
/// `64.0` should read as `64`, not `64.000000`: map files are diffed by hand
/// and reviewed in pull requests like any other source.
/// Re-exported so the text formats and the editor cannot disagree about how a
/// coordinate is written. The implementation is in `kerosene-math`, next to the
/// numbers it formats.
pub use kerosene_math::format_float;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_accept_every_punctuation_style() {
        for src in ["0 64 128", "[0 64 128]", "(0 64 128)", "0, 64, 128", "  0   64 128 "] {
            assert_eq!(Vec3Value::from_kv(src).unwrap().0, [0.0, 64.0, 128.0], "{src}");
        }
    }

    #[test]
    fn wrong_component_count_is_rejected() {
        assert!(Vec3Value::from_kv("0 64").is_err());
        assert!(Vec3Value::from_kv("0 64 128 255").is_err());
    }

    #[test]
    fn integers_tolerate_float_spelling() {
        assert_eq!(i32::from_kv("64.000000").unwrap(), 64);
        assert_eq!(i32::from_kv("-3").unwrap(), -3);
        assert!(i32::from_kv("banana").is_err());
    }

    #[test]
    fn booleans_take_the_usual_spellings() {
        for t in ["1", "true", "yes", "on", "2"] { assert!(bool::from_kv(t).unwrap(), "{t}"); }
        for f in ["0", "false", "no", "off", ""] { assert!(!bool::from_kv(f).unwrap(), "{f}"); }
    }

    #[test]
    fn floats_print_without_noise() {
        assert_eq!(format_float(64.0), "64");
        assert_eq!(format_float(-0.5), "-0.5");
        assert_eq!(format_float(0.25), "0.25");
    }
}
