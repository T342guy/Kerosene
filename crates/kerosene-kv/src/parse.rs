// SPDX-License-Identifier: MPL-2.0
use super::{Entry, KeyValues};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("line {line}: unexpected '}}' with no matching '{{'")]
    UnbalancedClose { line: u32 },
    #[error("line {line}: block {name:?} was never closed")]
    UnclosedBlock { line: u32, name: String },
    #[error("line {line}: string was never closed")]
    UnterminatedString { line: u32 },
    #[error("line {line}: key {key:?} has no value")]
    DanglingKey { line: u32, key: String },
    #[error("line {line}: expected a key, found '{{'")]
    UnexpectedOpen { line: u32 },
    #[error("nesting deeper than {limit} levels (line {line})")]
    TooDeep { line: u32, limit: u32 },
}

/// Deeper than any legitimate file, shallow enough that a malformed one
/// cannot blow the stack -- the parser is recursive-descent-shaped.
const MAX_DEPTH: u32 = 64;

#[derive(Debug, PartialEq)]
enum Token {
    /// A bare word: block names, and keys in files written by hand.
    Word(String),
    /// A `"quoted string"`, with escapes already resolved.
    Quoted(String),
    Open,
    Close,
    /// A `[$condition]` suffix. Recognised so it cannot be mistaken for a
    /// value, then dropped.
    Condition,
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self { Self { src: src.as_bytes(), pos: 0, line: 1 } }

    fn skip_trivia(&mut self) {
        loop {
            while self.pos < self.src.len() {
                let c = self.src[self.pos];
                if c == b'\n' { self.line += 1; self.pos += 1; }
                else if c.is_ascii_whitespace() { self.pos += 1; }
                else { break; }
            }
            // Line comments.
            if self.src[self.pos..].starts_with(b"//") {
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' { self.pos += 1; }
                continue;
            }
            // Block comments; not in Valve's parser, but harmless to accept
            // and genuinely useful for commenting out a slab of brushes.
            if self.src[self.pos..].starts_with(b"/*") {
                self.pos += 2;
                while self.pos < self.src.len() && !self.src[self.pos..].starts_with(b"*/") {
                    if self.src[self.pos] == b'\n' { self.line += 1; }
                    self.pos += 1;
                }
                self.pos = (self.pos + 2).min(self.src.len());
                continue;
            }
            return;
        }
    }

    fn next(&mut self) -> Result<Option<Token>, ParseError> {
        self.skip_trivia();
        if self.pos >= self.src.len() { return Ok(None); }

        let c = self.src[self.pos];
        match c {
            b'{' => { self.pos += 1; Ok(Some(Token::Open)) }
            b'}' => { self.pos += 1; Ok(Some(Token::Close)) }
            b'[' => {
                while self.pos < self.src.len() && self.src[self.pos] != b']' {
                    if self.src[self.pos] == b'\n' { self.line += 1; }
                    self.pos += 1;
                }
                self.pos = (self.pos + 1).min(self.src.len());
                Ok(Some(Token::Condition))
            }
            b'"' => {
                let start_line = self.line;
                self.pos += 1;
                let mut out = String::new();
                loop {
                    if self.pos >= self.src.len() {
                        return Err(ParseError::UnterminatedString { line: start_line });
                    }
                    match self.src[self.pos] {
                        b'"' => { self.pos += 1; break; }
                        b'\\' if self.pos + 1 < self.src.len() => {
                            let esc = self.src[self.pos + 1];
                            // Only the sequences we ourselves emit are
                            // resolved. Anything else keeps its backslash, so
                            // hand-written asset paths like
                            // `materials\dev\grid` survive untouched -- Valve's
                            // parser has escapes off by default for exactly
                            // this reason.
                            let resolved = match esc {
                                b'"' => Some('"'),
                                b'\\' => Some('\\'),
                                b'n' => Some('\n'),
                                b't' => Some('\t'),
                                _ => None,
                            };
                            match resolved {
                                Some(ch) => { out.push(ch); self.pos += 2; }
                                None => { out.push('\\'); self.pos += 1; }
                            }
                        }
                        b'\n' => { self.line += 1; out.push('\n'); self.pos += 1; }
                        _ => {
                            let start = self.pos;
                            while self.pos < self.src.len()
                                && !matches!(self.src[self.pos], b'"' | b'\\' | b'\n')
                            {
                                self.pos += 1;
                            }
                            out.push_str(&String::from_utf8_lossy(&self.src[start..self.pos]));
                        }
                    }
                }
                Ok(Some(Token::Quoted(out)))
            }
            _ => {
                let start = self.pos;
                while self.pos < self.src.len() {
                    let c = self.src[self.pos];
                    if c.is_ascii_whitespace() || matches!(c, b'{' | b'}' | b'"' | b'[') { break; }
                    if self.src[self.pos..].starts_with(b"//") { break; }
                    self.pos += 1;
                }
                Ok(Some(Token::Word(
                    String::from_utf8_lossy(&self.src[start..self.pos]).into_owned(),
                )))
            }
        }
    }
}

pub(crate) fn parse_document(text: &str) -> Result<KeyValues, ParseError> {
    let mut lexer = Lexer::new(text);
    let mut root = KeyValues::new("");
    // An explicit stack rather than recursion: a hostile file should hit
    // TooDeep, not a stack overflow.
    let mut stack: Vec<KeyValues> = Vec::new();
    let mut pending: Option<(String, u32)> = None;

    while let Some(tok) = lexer.next()? {
        match tok {
            Token::Condition => { /* platform suffix; carries no data we keep */ }

            Token::Open => {
                let (name, _) = pending.take()
                    .ok_or(ParseError::UnexpectedOpen { line: lexer.line })?;
                if stack.len() as u32 >= MAX_DEPTH {
                    return Err(ParseError::TooDeep { line: lexer.line, limit: MAX_DEPTH });
                }
                stack.push(KeyValues::new(name));
            }

            Token::Close => {
                if let Some((key, line)) = pending.take() {
                    return Err(ParseError::DanglingKey { line, key });
                }
                let done = stack.pop().ok_or(ParseError::UnbalancedClose { line: lexer.line })?;
                match stack.last_mut() {
                    Some(parent) => parent.entries.push(Entry::Block(done)),
                    None => root.entries.push(Entry::Block(done)),
                }
            }

            Token::Word(w) | Token::Quoted(w) => match pending.take() {
                // Second token in a row: this is the value of the pending key.
                Some((key, _)) => {
                    let target = stack.last_mut().unwrap_or(&mut root);
                    target.entries.push(Entry::Pair(key, w));
                }
                None => pending = Some((w, lexer.line)),
            },
        }
    }

    if let Some((key, line)) = pending {
        return Err(ParseError::DanglingKey { line, key });
    }
    if let Some(open) = stack.last() {
        return Err(ParseError::UnclosedBlock { line: lexer.line, name: open.name.clone() });
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyValues;

    #[test]
    fn unquoted_keys_and_values_work() {
        let kv = KeyValues::parse("shader { basetexture dev/grid }").unwrap();
        assert_eq!(kv.block("shader").unwrap().get("basetexture"), Some("dev/grid"));
    }

    #[test]
    fn comments_are_ignored_everywhere() {
        let src = r#"
            // leading
            a /* inline */ {
                "k" "v" // trailing
            }
        "#;
        let kv = KeyValues::parse(src).unwrap();
        assert_eq!(kv.block("a").unwrap().get("k"), Some("v"));
    }

    #[test]
    fn backslash_paths_survive_unmangled() {
        // The trap: treating '\d' as an escape would corrupt every asset path
        // written the Windows way.
        let kv = KeyValues::parse(r#"m { "tex" "materials\dev\grid" }"#).unwrap();
        assert_eq!(kv.block("m").unwrap().get("tex"), Some(r"materials\dev\grid"));
    }

    #[test]
    fn known_escapes_still_resolve() {
        let kv = KeyValues::parse(r#"m { "t" "say \"hi\"" }"#).unwrap();
        assert_eq!(kv.block("m").unwrap().get("t"), Some(r#"say "hi""#));
    }

    #[test]
    fn vmt_condition_suffix_is_not_read_as_a_value() {
        let kv = KeyValues::parse(r#"s { "$basetexture" "a" [$XBOX] "$other" "b" }"#).unwrap();
        let s = kv.block("s").unwrap();
        assert_eq!(s.get("$basetexture"), Some("a"));
        assert_eq!(s.get("$other"), Some("b"), "the [..] suffix must not shift the pairing");
    }

    #[test]
    fn errors_carry_line_numbers() {
        let err = KeyValues::parse("a\n{\n\"k\" \"v\"\n").unwrap_err();
        assert!(matches!(err, ParseError::UnclosedBlock { .. }), "{err}");
        let err = KeyValues::parse("}").unwrap_err();
        assert!(matches!(err, ParseError::UnbalancedClose { line: 1 }), "{err}");
        let err = KeyValues::parse("a {\n\"lonely\"\n}").unwrap_err();
        assert!(matches!(err, ParseError::DanglingKey { line: 2, .. }), "{err}");
    }

    #[test]
    fn deep_nesting_errors_instead_of_overflowing() {
        let src = "a {".repeat(500);
        assert!(matches!(KeyValues::parse(&src), Err(ParseError::TooDeep { .. })));
    }

    #[test]
    fn unterminated_string_is_reported() {
        assert!(matches!(
            KeyValues::parse("a { \"k\" \"oops }"),
            Err(ParseError::UnterminatedString { .. })
        ));
    }
}
