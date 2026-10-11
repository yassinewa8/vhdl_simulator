//! Splitting VHDL text into coloured pieces for the source editor.
//!
//! Unlike the compile-order scanner, which throws comments and literals
//! away, this keeps every character: the pieces cover the whole text, end
//! to end, so the editor can colour each one. It only looks at one token
//! at a time (no parsing), so it's fast and never fails, even on a file
//! that is half typed.

use std::ops::Range;

/// What a piece of text is, which decides its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// Names, operators, punctuation and spaces.
    Plain,
    /// Reserved words such as `entity`, `process`, `begin`.
    Keyword,
    /// Common type names such as `std_logic`, `unsigned`, `integer`.
    Type,
    Comment,
    /// String, character and bit-string literals: `"abc"`, `'1'`, `x"FF"`.
    Literal,
    /// Numbers: `42`, `1_000`, `16#FF#`, `1.5e-3`.
    Number,
}

/// A piece of the text: byte offsets into it, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: TokenKind,
}

/// Splits `source` into pieces covering all of it, in order. Neighbouring
/// pieces of the same kind are merged, so plain text comes in big pieces.
pub fn highlight(source: &str) -> Vec<Span> {
    let bytes = source.as_bytes();
    let mut spans: Vec<Span> = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let (end, kind) = next_token(source, i);
        // Always move forward, whatever the token rules say.
        let end = end.max(i + 1);
        push(&mut spans, i..end, kind);
        i = end;
    }
    spans
}

/// The token starting at byte `start`: where it ends and what it is.
/// All the characters that start a token are ASCII, so byte positions
/// never land inside a multi-byte character.
fn next_token(source: &str, start: usize) -> (usize, TokenKind) {
    let bytes = source.as_bytes();
    let c = bytes[start];
    let next = bytes.get(start + 1).copied();
    let previous = start.checked_sub(1).map(|p| bytes[p]);

    if c == b'-' && next == Some(b'-') {
        // Line comment: up to (not including) the line end.
        (find(bytes, start, |b| b == b'\n'), TokenKind::Comment)
    } else if c == b'/' && next == Some(b'*') {
        // Block comment (VHDL-2008), which may span lines.
        let end = source[start + 2..].find("*/").map_or(bytes.len(), |at| start + 2 + at + 2);
        (end, TokenKind::Comment)
    } else if c == b'"' {
        (string_end(bytes, start), TokenKind::Literal)
    } else if c == b'\'' && bytes.get(start + 2) == Some(&b'\'') && !previous.is_some_and(is_name_end) {
        // Character literal such as '1'. After a name or `)` a tick is an
        // attribute (`clk'event`) or qualified expression, so plain.
        (start + 3, TokenKind::Literal)
    } else if c.is_ascii_digit() && !previous.is_some_and(is_word_byte) {
        number_or_bit_string(bytes, start)
    } else if c.is_ascii_alphabetic() {
        word(source, start)
    } else if c == b'\\' {
        // Extended identifier: \any text\.
        let end = find(bytes, start + 1, |b| b == b'\\' || b == b'\n');
        let end = if bytes.get(end) == Some(&b'\\') { end + 1 } else { end };
        (end, TokenKind::Plain)
    } else {
        // Anything else is plain: take the run of characters that can't
        // start a token, so spaces and operators come as one piece.
        let end = find(bytes, start + 1, starts_token);
        (end, TokenKind::Plain)
    }
}

/// A word: a keyword, a type name, a bit string such as `x"FF"`, or a name.
fn word(source: &str, start: usize) -> (usize, TokenKind) {
    let bytes = source.as_bytes();
    let end = find(bytes, start, |b| !is_word_byte(b));
    // `x"FF"`, `b"0101"`, `o"17"`, and VHDL-2008's `ux"F"`, `sb"01"`, `d"9"`.
    if bytes.get(end) == Some(&b'"') && is_bit_string_base(&source[start..end]) {
        return (string_end(bytes, end), TokenKind::Literal);
    }
    let lower = source[start..end].to_ascii_lowercase();
    let kind = if KEYWORDS.contains(&lower.as_str()) {
        TokenKind::Keyword
    } else if TYPES.contains(&lower.as_str()) {
        TokenKind::Type
    } else {
        TokenKind::Plain
    };
    (end, kind)
}

/// A number, or a bit string with a width such as `8x"FF"`.
fn number_or_bit_string(bytes: &[u8], start: usize) -> (usize, TokenKind) {
    let digits_end = find(bytes, start, |b| !(b.is_ascii_digit() || b == b'_'));
    // `8x"FF"`: width, base letters, then the string.
    let base_end = find(bytes, digits_end, |b| !b.is_ascii_alphabetic());
    if base_end > digits_end
        && bytes.get(base_end) == Some(&b'"')
        && is_bit_string_base(&String::from_utf8_lossy(&bytes[digits_end..base_end]))
    {
        return (string_end(bytes, base_end), TokenKind::Literal);
    }
    // Based (`16#FF#`), real (`1.5`) and exponent (`1.0e-3`) forms.
    let mut end = find(bytes, start, |b| !(b.is_ascii_alphanumeric() || b"_#.".contains(&b)));
    if matches!(bytes.get(end), Some(b'-' | b'+')) && matches!(bytes[end - 1], b'e' | b'E') {
        end = find(bytes, end + 1, |b| !(b.is_ascii_digit() || b == b'_'));
    }
    (end, TokenKind::Number)
}

fn is_bit_string_base(letters: &str) -> bool {
    matches!(
        letters.to_ascii_lowercase().as_str(),
        "b" | "o" | "x" | "d" | "ub" | "uo" | "ux" | "sb" | "so" | "sx"
    )
}

/// End of a string literal starting at `start` (the opening quote). `""`
/// inside stands for one quote. Strings can't span lines, so an unclosed
/// one ends at the line end.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if bytes.get(i + 1) == Some(&b'"') => i += 2,
            b'"' => return i + 1,
            b'\n' => return i,
            _ => i += 1,
        }
    }
    i
}

/// Characters that can start a non-plain token (or a word).
fn starts_token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-/\"'\\".contains(&b)
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// True if a tick after this character is an attribute, not a literal.
fn is_name_end(b: u8) -> bool {
    is_word_byte(b) || b == b')' || b == b'\\'
}

/// First position at or after `start` matching `stop`, or the end.
fn find(bytes: &[u8], start: usize, stop: impl Fn(u8) -> bool) -> usize {
    (start..bytes.len()).find(|&i| stop(bytes[i])).unwrap_or(bytes.len())
}

/// Adds a piece, merging it into the last one if they're the same kind.
fn push(spans: &mut Vec<Span>, range: Range<usize>, kind: TokenKind) {
    if let Some(last) = spans.last_mut()
        && last.kind == kind
        && last.range.end == range.start
    {
        last.range.end = range.end;
    } else {
        spans.push(Span { range, kind });
    }
}

/// VHDL-2019 reserved words (a superset of the earlier standards).
const KEYWORDS: &[&str] = &[
    "abs", "access", "after", "alias", "all", "and", "architecture", "array", "assert", "assume", "attribute",
    "begin", "block", "body", "buffer", "bus", "case", "component", "configuration", "constant", "context",
    "cover", "default", "disconnect", "downto", "else", "elsif", "end", "entity", "exit", "fairness", "file",
    "for", "force", "function", "generate", "generic", "group", "guarded", "if", "impure", "in", "inertial",
    "inout", "is", "label", "library", "linkage", "literal", "loop", "map", "mod", "nand", "new", "next", "nor",
    "not", "null", "of", "on", "open", "or", "others", "out", "package", "parameter", "port", "postponed",
    "private", "procedure", "process", "property", "protected", "pure", "range", "record", "register",
    "reject", "release", "rem", "report", "restrict", "return", "rol", "ror", "select", "sequence", "severity",
    "shared", "signal", "sla", "sll", "sra", "srl", "strong", "subtype", "then", "to", "transport", "type",
    "unaffected", "units", "until", "use", "variable", "view", "vmode", "vpkg", "vprop", "vunit", "wait",
    "when", "while", "with", "xnor", "xor",
];

/// Type names worth picking out. Not reserved words, just very common.
const TYPES: &[&str] = &[
    "bit", "bit_vector", "boolean", "boolean_vector", "character", "integer", "integer_vector", "natural",
    "positive", "real", "real_vector", "severity_level", "signed", "std_logic", "std_logic_vector",
    "std_ulogic", "std_ulogic_vector", "string", "time", "time_vector", "unsigned", "line", "text",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The pieces as (text, kind), leaving out plain ones for brevity.
    fn coloured(source: &str) -> Vec<(&str, TokenKind)> {
        highlight(source)
            .into_iter()
            .filter(|span| span.kind != TokenKind::Plain)
            .map(|span| (&source[span.range], span.kind))
            .collect()
    }

    #[test]
    fn pieces_cover_the_whole_text_in_order() {
        let source = "entity top is -- the top\n  port (a : in std_logic);\nend; -- naïve ünïcode ✓\n";
        let spans = highlight(source);
        assert_eq!(spans[0].range.start, 0);
        assert_eq!(spans.last().unwrap().range.end, source.len());
        for pair in spans.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start);
            assert_ne!(pair[0].kind, pair[1].kind, "same kinds are merged");
        }
    }

    #[test]
    fn keywords_types_and_comments() {
        assert_eq!(
            coloured("ENTITY top IS -- My comment\nsignal s : Std_Logic;"),
            [
                ("ENTITY", TokenKind::Keyword),
                ("IS", TokenKind::Keyword),
                ("-- My comment", TokenKind::Comment),
                ("signal", TokenKind::Keyword),
                ("Std_Logic", TokenKind::Type),
            ]
        );
        assert_eq!(coloured("/* a\nblock */ end"), [("/* a\nblock */", TokenKind::Comment), ("end", TokenKind::Keyword)]);
        // Names containing keywords aren't keywords.
        assert!(coloured("my_signal ends portal").is_empty());
    }

    #[test]
    fn literals_and_numbers() {
        assert_eq!(
            coloured(r#"x <= "a ""quoted"" one"; y <= '1'; z <= X"FF"; w <= 8ux"F";"#),
            [
                (r#""a ""quoted"" one""#, TokenKind::Literal),
                ("'1'", TokenKind::Literal),
                (r#"X"FF""#, TokenKind::Literal),
                (r#"8ux"F""#, TokenKind::Literal),
            ]
        );
        assert_eq!(
            coloured("a := 42 + 1_000 + 16#FF# + 1.5e-3 + 10 ns;"),
            [
                ("42", TokenKind::Number),
                ("1_000", TokenKind::Number),
                ("16#FF#", TokenKind::Number),
                ("1.5e-3", TokenKind::Number),
                ("10", TokenKind::Number),
            ]
        );
    }

    #[test]
    fn ticks_after_names_are_attributes() {
        // `clk'event` is not a character literal, and neither is the
        // tick in `std_logic'('1')` (but the '1' inside is).
        assert_eq!(
            coloured("if clk'event and s = std_logic'('1') then"),
            [
                ("if", TokenKind::Keyword),
                ("and", TokenKind::Keyword),
                ("std_logic", TokenKind::Type),
                ("'1'", TokenKind::Literal),
                ("then", TokenKind::Keyword),
            ]
        );
        // Digits inside a name such as `reg2` aren't numbers.
        assert!(coloured("reg2 <= a1;").is_empty());
    }

    #[test]
    fn unfinished_text_is_fine() {
        // Typing in the editor leaves half-written tokens everywhere.
        for source in ["\"open string", "/* open comment", "x\"F", "'", "\\ext", "16#F", "1.0e-"] {
            let spans = highlight(source);
            assert_eq!(spans.last().unwrap().range.end, source.len(), "{source}");
        }
        assert!(highlight("").is_empty());
    }
}
