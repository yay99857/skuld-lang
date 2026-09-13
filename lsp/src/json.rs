//! The smallest JSON that speaks LSP, written here rather than taken from a
//! crate so the project keeps its no-dependency rule.
//!
//! This is not a general JSON library and does not try to be one. It reads and
//! writes what a language client sends, rejects the rest, and is deliberately
//! strict: a malformed message is a protocol error worth reporting, never
//! something to guess at.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A JSON value. Objects keep their keys sorted rather than in source order:
/// nothing in LSP depends on member order, and a map makes lookup honest.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members.get(key),
            _ => None,
        }
    }

    /// Follow a chain of object keys, e.g. `params.textDocument.uri`.
    pub fn path(&self, keys: &[&str]) -> Option<&Json> {
        keys.iter().try_fold(self, |value, key| value.get(key))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    /// An integer, which is the only numeric shape LSP uses for ids. The
    /// server passes an id through untouched, so nothing outside the tests
    /// reads one back; it is gated rather than kept as an unused accessor.
    #[cfg(test)]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Number(value) if value.fract() == 0.0 => Some(*value as i64),
            _ => None,
        }
    }

    pub fn object(members: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
        Json::Object(
            members
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    pub fn string(text: impl Into<String>) -> Json {
        Json::String(text.into())
    }

    pub fn number(value: impl Into<f64>) -> Json {
        Json::Number(value.into())
    }
}

// --- Writing ---------------------------------------------------------------

impl Json {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(value) => {
                // An integer is written without a fractional part: a client
                // reading `id` as an integer must not be handed `1.0`.
                if value.fract() == 0.0 && value.is_finite() && value.abs() < 1e15 {
                    let _ = write!(out, "{}", *value as i64);
                } else if value.is_finite() {
                    let _ = write!(out, "{value}");
                } else {
                    // JSON has no infinity or NaN; null is the only honest
                    // encoding, and no LSP field this crate writes can be one.
                    out.push_str("null");
                }
            }
            Json::String(text) => write_string(text, out),
            Json::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(members) => {
                out.push('{');
                for (index, (key, value)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            // Every other control character needs the \u form, or the message
            // is invalid JSON that a client will reject as a framing error.
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// --- Reading ---------------------------------------------------------------

/// How deep a nested value may go before parsing gives up. A client never
/// sends anything close; a hostile or corrupt message could, and recursion
/// that deep would abort the process rather than return an error.
const MAX_DEPTH: usize = 128;

pub fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        offset: 0,
        depth: 0,
    };
    parser.skip_whitespace();
    let value = parser.value()?;
    parser.skip_whitespace();
    if parser.offset != parser.bytes.len() {
        return Err(format!("trailing input at byte {}", parser.offset));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    offset: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.offset += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.offset += 1;
            Ok(())
        } else {
            Err(format!(
                "expected `{}` at byte {}",
                byte as char, self.offset
            ))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.offset..].starts_with(word.as_bytes()) {
            self.offset += word.len();
            Ok(value)
        } else {
            Err(format!("invalid literal at byte {}", self.offset))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err("value nested too deeply".to_string());
        }
        let value = match self.peek() {
            None => Err(format!("unexpected end of input at byte {}", self.offset)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(byte) => Err(format!(
                "unexpected `{}` at byte {}",
                byte as char, self.offset
            )),
        };
        self.depth -= 1;
        value
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.offset += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.offset += 1,
                Some(b']') => {
                    self.offset += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected `,` or `]` at byte {}", self.offset)),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut members = BTreeMap::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.offset += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.value()?;
            members.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.offset += 1,
                Some(b'}') => {
                    self.offset += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(format!("expected `,` or `}}` at byte {}", self.offset)),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.offset;
        if self.peek() == Some(b'-') {
            self.offset += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.offset += 1;
        }
        if self.peek() == Some(b'.') {
            self.offset += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.offset += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.offset += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.offset += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.offset += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.offset])
            .map_err(|_| format!("invalid number at byte {start}"))?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| format!("invalid number `{text}` at byte {start}"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| "unterminated string".to_string())?;
            match byte {
                b'"' => {
                    self.offset += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.offset += 1;
                    let escape = self
                        .peek()
                        .ok_or_else(|| "unterminated escape".to_string())?;
                    self.offset += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        other => {
                            return Err(format!(
                                "unknown escape `\\{}` at byte {}",
                                other as char,
                                self.offset - 1
                            ));
                        }
                    }
                }
                // Control characters are not allowed unescaped in a JSON string.
                byte if byte < 0x20 => {
                    return Err(format!("control character at byte {}", self.offset));
                }
                _ => {
                    // Copy one whole UTF-8 sequence, so a multi-byte character
                    // is never split into invalid pieces.
                    let rest = std::str::from_utf8(&self.bytes[self.offset..])
                        .map_err(|_| format!("invalid UTF-8 at byte {}", self.offset))?;
                    let c = rest
                        .chars()
                        .next()
                        .ok_or_else(|| "unterminated string".to_string())?;
                    self.offset += c.len_utf8();
                    out.push(c);
                }
            }
        }
    }

    /// A `\uXXXX` escape, joining a surrogate pair when one is present. A lone
    /// surrogate cannot be a `char`, so it becomes the replacement character
    /// rather than failing the whole message over one bad name.
    fn unicode_escape(&mut self) -> Result<char, String> {
        let high = self.hex4()?;
        if !(0xD800..0xDC00).contains(&high) {
            return Ok(char::from_u32(high).unwrap_or('\u{fffd}'));
        }
        if self.bytes[self.offset..].starts_with(b"\\u") {
            let mark = self.offset;
            self.offset += 2;
            let low = self.hex4()?;
            if (0xDC00..0xE000).contains(&low) {
                let combined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                return Ok(char::from_u32(combined).unwrap_or('\u{fffd}'));
            }
            // Not a low surrogate after all: leave it for the next iteration.
            self.offset = mark;
        }
        Ok('\u{fffd}')
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.offset + 4;
        let digits = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "truncated \\u escape".to_string())?;
        let text = std::str::from_utf8(digits).map_err(|_| "invalid \\u escape".to_string())?;
        let value =
            u32::from_str_radix(text, 16).map_err(|_| format!("invalid \\u escape `{text}`"))?;
        self.offset = end;
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
