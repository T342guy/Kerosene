// SPDX-License-Identifier: LGPL-3.0-or-later OR MPL-2.0
//! Splitting console text into commands, and commands into arguments.
//!
//! Both operations have to respect quoting, because `say "hello; world"` is
//! one command with one argument and not two of anything.

/// Split a block of console text into individual command lines.
///
/// Commands separate on `;` and on newlines. A `//` starts a comment that runs
/// to the end of the line. Neither separator applies inside a quoted string.
pub fn split_commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => { in_quotes = !in_quotes; current.push(c); }
            '/' if !in_quotes && chars.peek() == Some(&'/') => {
                // Comment: discard to end of line, but the line still ends the
                // command in progress.
                for c in chars.by_ref() {
                    if c == '\n' { break; }
                }
                push_trimmed(&mut out, &mut current);
            }
            ';' | '\n' if !in_quotes => push_trimmed(&mut out, &mut current),
            _ => current.push(c),
        }
    }
    push_trimmed(&mut out, &mut current);
    out
}

fn push_trimmed(out: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() { out.push(trimmed.to_string()); }
    current.clear();
}

/// Split one command line into arguments.
///
/// Quotes group words and are stripped from the result, so
/// `hostname "The Refinery"` yields `["hostname", "The Void"]`.
pub fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_token = false;

    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                // An empty quoted string is still an argument, so remember
                // that a token was started even if no characters land in it.
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token { out.push(std::mem::take(&mut current)); has_token = false; }
            }
            c => { current.push(c); has_token = true; }
        }
    }
    if has_token { out.push(current); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_semicolons_and_newlines() {
        assert_eq!(split_commands("a 1; b 2\nc 3"), vec!["a 1", "b 2", "c 3"]);
    }

    #[test]
    fn empty_segments_are_dropped() {
        assert_eq!(split_commands(";;a;;\n\n;b;"), vec!["a", "b"]);
    }

    #[test]
    fn separators_inside_quotes_are_literal() {
        assert_eq!(split_commands(r#"say "one; two""#), vec![r#"say "one; two""#]);
    }

    #[test]
    fn comments_run_to_end_of_line() {
        assert_eq!(split_commands("a 1 // trailing\nb 2"), vec!["a 1", "b 2"]);
        assert_eq!(split_commands("// whole line\nb 2"), vec!["b 2"]);
    }

    #[test]
    fn a_url_inside_quotes_is_not_a_comment() {
        assert_eq!(
            split_commands(r#"motd "https://example.com/x""#),
            vec![r#"motd "https://example.com/x""#]
        );
    }

    #[test]
    fn tokenize_groups_quoted_words() {
        assert_eq!(tokenize(r#"hostname "The Refinery""#), vec!["hostname", "The Refinery"]);
    }

    #[test]
    fn tokenize_keeps_empty_quoted_argument() {
        assert_eq!(tokenize(r#"name """#), vec!["name", ""]);
    }

    #[test]
    fn tokenize_collapses_runs_of_whitespace() {
        assert_eq!(tokenize("  a   b\tc  "), vec!["a", "b", "c"]);
    }

    #[test]
    fn tokenize_handles_empty_input() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("   ").is_empty());
    }
}
