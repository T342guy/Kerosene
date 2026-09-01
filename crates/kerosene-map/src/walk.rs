// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! How a face participates in the NPC walkmap.
//!
//! A walkmap is a compiled answer to "where can NPCs go", built from the flat
//! walkable faces of the world. Every face carries one of these rules, chosen
//! in Chisel, and the compiler folds them together: `allow` lets a flat face
//! into the walkmap, `deny` keeps it out, `avoid` lets it in but marks it as
//! a place NPCs would rather not cross, and `always` forces it in even if it
//! is not flat.

use std::fmt;

/// A face's rule in the compiled walkmap.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash, PartialOrd, Ord)]
pub enum WalkmapRule {
    /// Walkable if the face is flat enough. The default for ordinary floors.
    #[default]
    Allow,
    /// Never walkable, even if flat.
    Deny,
    /// Walkable, but NPCs prefer to go around rather than through.
    Avoid,
    /// Always walkable, even if the face is not flat.
    Always,
}

impl WalkmapRule {
    /// Parse the spelling used in a `.keromap`. Unknown words fall back to
    /// [`WalkmapRule::Allow`], because a face that says nothing is a floor.
    pub fn parse(s: &str) -> WalkmapRule {
        match s.trim().to_ascii_lowercase().as_str() {
            "deny" => WalkmapRule::Deny,
            "avoid" => WalkmapRule::Avoid,
            "always" => WalkmapRule::Always,
            _ => WalkmapRule::Allow,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            WalkmapRule::Allow => "allow",
            WalkmapRule::Deny => "deny",
            WalkmapRule::Avoid => "avoid",
            WalkmapRule::Always => "always",
        }
    }

    /// Every rule, in the order the editor lists them.
    pub fn all() -> [WalkmapRule; 4] {
        [WalkmapRule::Allow, WalkmapRule::Deny, WalkmapRule::Avoid, WalkmapRule::Always]
    }

    /// A sentence for a tooltip.
    pub fn describe(self) -> &'static str {
        match self {
            WalkmapRule::Allow => "walkable if the face is flat (the default)",
            WalkmapRule::Deny => "never walkable, even if flat",
            WalkmapRule::Avoid => "walkable, but NPCs prefer to go around",
            WalkmapRule::Always => "always walkable, even if the face is not flat",
        }
    }
}

impl fmt::Display for WalkmapRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_insensitively() {
        assert_eq!(WalkmapRule::parse("deny"), WalkmapRule::Deny);
        assert_eq!(WalkmapRule::parse("AVOID"), WalkmapRule::Avoid);
        assert_eq!(WalkmapRule::parse("Always"), WalkmapRule::Always);
        assert_eq!(WalkmapRule::parse("allow"), WalkmapRule::Allow);
    }

    #[test]
    fn unknown_words_are_allow() {
        assert_eq!(WalkmapRule::parse(""), WalkmapRule::Allow);
        assert_eq!(WalkmapRule::parse("  "), WalkmapRule::Allow);
        assert_eq!(WalkmapRule::parse("sometimes"), WalkmapRule::Allow);
    }

    #[test]
    fn round_trips_through_text() {
        for rule in WalkmapRule::all() {
            assert_eq!(WalkmapRule::parse(rule.as_str()), rule);
            assert_eq!(rule.to_string(), rule.as_str());
        }
    }

    #[test]
    fn every_rule_says_what_it_does() {
        for rule in WalkmapRule::all() {
            assert!(!rule.describe().is_empty());
        }
    }
}
