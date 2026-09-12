/// A half-open byte range in one UTF-8 source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// Owns source text and its line index. Spans remain independent of filenames.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(offset, _)| offset + 1));
        Self {
            name: name.into(),
            text,
            line_starts,
        }
    }

    /// Returns one-based line and Unicode scalar column; clamps invalid offsets.
    pub fn location(&self, offset: usize) -> (usize, usize) {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        (
            line + 1,
            self.text[self.line_starts[line]..offset].chars().count() + 1,
        )
    }

    pub fn line(&self, number: usize) -> Option<&str> {
        let index = number.checked_sub(1)?;
        let start = *self.line_starts.get(index)?;
        let end = self
            .line_starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.text.len());
        Some(self.text[start..end].trim_end_matches(['\r', '\n']))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexes_unicode_crlf_and_eof() {
        let source = SourceFile::new("test", "éx\r\nlast\n");
        assert_eq!(source.location(2), (1, 2));
        assert_eq!(source.location(5), (2, 1));
        assert_eq!(source.location(source.text.len()), (3, 1));
        assert_eq!(source.line(1), Some("éx"));
        assert_eq!(source.line(0), None);
        assert_eq!(source.line(4), None);
    }
}
