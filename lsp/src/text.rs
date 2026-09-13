//! Turning compiler spans into LSP positions, and `file:` URIs into paths.
//!
//! The compiler measures everything in bytes, and `SourceFile::location`
//! reports a column in Unicode scalars. LSP counts a character in **UTF-16
//! code units** by default, so the two agree on ASCII, agree on `é`, and
//! disagree on anything outside the Basic Multilingual Plane — an emoji is one
//! scalar and two UTF-16 units. Converting here rather than reusing
//! `location()` is what keeps a diagnostic under the right characters.

/// A zero-based LSP position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    /// UTF-16 code units from the start of the line.
    pub character: usize,
}

/// Converts byte offsets in one source text into LSP positions.
pub struct Positions {
    /// Byte offset where each line starts.
    line_starts: Vec<usize>,
    text: String,
}

impl Positions {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(offset, _)| offset + 1));
        Self { line_starts, text }
    }

    /// The position of a byte offset. An offset past the end clamps to the end
    /// of the text, and one that lands inside a character rounds down to its
    /// start, so a bad span can never panic or produce a position a client
    /// would reject.
    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let start = self.line_starts[line];
        // Round down to a character boundary before slicing.
        let mut offset = offset;
        while offset > start && !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let character = self.text[start..offset].chars().map(char::len_utf16).sum();
        Position { line, character }
    }
}

impl Positions {
    /// The byte offset of a position. A line or a character past the end
    /// clamps, so a client that counted differently cannot make this panic or
    /// slice a string in the middle of a character.
    pub fn offset(&self, position: Position) -> usize {
        let Some(&start) = self.line_starts.get(position.line) else {
            return self.text.len();
        };
        let end = self
            .line_starts
            .get(position.line + 1)
            .map_or(self.text.len(), |&next| next);
        let mut offset = start;
        let mut units = 0;
        for character in self.text[start..end].chars() {
            if units >= position.character {
                break;
            }
            units += character.len_utf16();
            offset += character.len_utf8();
        }
        offset.min(end)
    }
}

/// Decode a `file:` URI into a filesystem path.
///
/// Only `file:` is accepted: a language server that followed any other scheme
/// would be reading from somewhere the client never meant to open.
pub fn uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // `file:///path` has an empty authority; anything else is a remote host
    // this server has no business reading.
    let path = match rest.find('/') {
        Some(0) => rest,
        _ => return None,
    };
    Some(percent_decode(path))
}

/// Encode a filesystem path as a `file:` URI.
pub fn path_to_uri(path: &str) -> String {
    let mut out = String::from("file://");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(value) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    // A URI that does not decode to UTF-8 is not a path this server can open;
    // lossy conversion would invent a filename that does not exist.
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

#[cfg(test)]
mod tests;
