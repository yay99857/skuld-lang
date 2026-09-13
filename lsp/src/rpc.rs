//! The LSP wire format: JSON-RPC messages framed by HTTP-like headers over
//! stdio.
//!
//! A message is `Content-Length: <n>\r\n\r\n` followed by exactly `n` bytes of
//! UTF-8 JSON. `Content-Type` may appear and is ignored; nothing else is.
//! The length is counted in bytes, not characters, which is why the body is
//! read from a byte stream and validated as UTF-8 afterwards.

use crate::json::{self, Json};
use std::io::{BufRead, Write};

/// Why reading a message stopped.
#[derive(Debug)]
pub enum ReadError {
    /// The peer closed the stream between messages, which is an ordinary end
    /// and not a failure.
    Closed,
    /// The stream is still open but what arrived is not a message. The server
    /// cannot resynchronise a byte stream it has lost its place in.
    Malformed(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Closed => write!(out, "the client closed the connection"),
            ReadError::Malformed(reason) => write!(out, "{reason}"),
        }
    }
}

/// Read one framed message. `Ok(None)` is never returned: a clean end of
/// stream is [`ReadError::Closed`], so a caller cannot confuse "no more
/// messages" with "an empty message".
pub fn read(input: &mut impl BufRead) -> Result<Json, ReadError> {
    let mut length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let read = input
            .read_line(&mut line)
            .map_err(|error| ReadError::Malformed(format!("cannot read a header: {error}")))?;
        if read == 0 {
            return Err(ReadError::Closed);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(ReadError::Malformed(format!("malformed header `{header}`")));
        };
        // Header names are case-insensitive; some clients send `content-length`.
        if name.trim().eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse().map_err(|_| {
                ReadError::Malformed(format!(
                    "`Content-Length: {}` is not a length",
                    value.trim()
                ))
            })?);
        }
    }

    let Some(length) = length else {
        return Err(ReadError::Malformed(
            "a message arrived with no `Content-Length` header".to_string(),
        ));
    };

    let mut body = vec![0u8; length];
    // `BufRead` implies `Read`, so `read_exact` is available here; a short
    // read is the truncation it reports, not a partial message to salvage.
    std::io::Read::read_exact(input, &mut body).map_err(|error| {
        ReadError::Malformed(format!("cannot read a {length}-byte body: {error}"))
    })?;
    let text = String::from_utf8(body)
        .map_err(|_| ReadError::Malformed("the message body is not UTF-8".to_string()))?;
    json::parse(&text).map_err(|reason| ReadError::Malformed(format!("invalid JSON: {reason}")))
}

/// Write one framed message and flush it. Flushing matters: a client waits on
/// a response that is still sitting in this process's buffer.
pub fn write(output: &mut impl Write, message: &Json) -> std::io::Result<()> {
    let body = message.to_text();
    write!(output, "Content-Length: {}\r\n\r\n", body.len())?;
    output.write_all(body.as_bytes())?;
    output.flush()
}

#[cfg(test)]
mod tests;
