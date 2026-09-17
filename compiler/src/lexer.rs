use crate::{
    diagnostic::{Diagnostic, DiagnosticCode},
    span::Span,
    token::{Token, TokenKind},
};

#[derive(Debug)]
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(source: &str) -> LexOutput {
    Lexer {
        source,
        offset: 0,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
        interpolations: Vec::new(),
    }
    .run()
}

/// How a run of string text ended.
enum Scanned {
    Closed(String),
    Interpolated(String),
}

struct Lexer<'a> {
    source: &'a str,
    offset: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
    /// Brace depth inside each open `${ ... }`. A `}` at depth zero ends the
    /// expression and returns to scanning string text.
    interpolations: Vec<usize>,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }
    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.offset += c.len_utf8();
        Some(c)
    }
    fn emit(&mut self, start: usize, kind: TokenKind) {
        self.tokens.push(Token {
            kind,
            span: Span::new(start, self.offset),
        });
    }
    fn error(&mut self, start: usize, code: DiagnosticCode, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            code,
            message: message.into(),
            span: Span::new(start, self.offset),
            help: None,
            fix: None,
        });
    }
    /// Brace depth inside each open `${ ... }`. A `}` at depth zero ends the
    /// expression and returns to scanning string text.
    fn run(mut self) -> LexOutput {
        use TokenKind::*;
        while let Some(c) = self.peek() {
            let start = self.offset;
            if c.is_ascii_whitespace() {
                self.advance();
                continue;
            }
            if self.source[start..].starts_with("//") {
                while self.peek().is_some_and(|c| c != '\n' && c != '\r') {
                    self.advance();
                }
                continue;
            }
            if c.is_ascii_alphabetic() || c == '_' {
                self.identifier(start);
                continue;
            }
            if c.is_ascii_digit() {
                self.number(start);
                continue;
            }
            if c == '"' || c == '\'' {
                self.quoted(start, c);
                continue;
            }
            self.advance();
            if c == '<' {
                if self.peek() == Some('<') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.emit(start, LessLessEqual);
                    } else {
                        self.emit(start, LessLess);
                    }
                    continue;
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.emit(start, LessEqual);
                    continue;
                }
                self.emit(start, Less);
                continue;
            }
            if c == '>' {
                if self.peek() == Some('>') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.emit(start, GreaterGreaterEqual);
                    } else {
                        self.emit(start, GreaterGreater);
                    }
                    continue;
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.emit(start, GreaterEqual);
                    continue;
                }
                self.emit(start, Greater);
                continue;
            }
            if c == '&' {
                if self.peek() == Some('&') {
                    self.advance();
                    self.emit(start, AndAnd);
                    continue;
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.emit(start, AmpersandEqual);
                    continue;
                }
                self.emit(start, Ampersand);
                continue;
            }
            if c == '|' {
                if self.peek() == Some('|') {
                    self.advance();
                    self.emit(start, OrOr);
                    continue;
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.emit(start, PipeEqual);
                    continue;
                }
                self.emit(start, Pipe);
                continue;
            }
            if c == '^' {
                if self.peek() == Some('=') {
                    self.advance();
                    self.emit(start, CaretEqual);
                    continue;
                }
                self.emit(start, Caret);
                continue;
            }
            if c == '~' {
                self.emit(start, Tilde);
                continue;
            }
            if c == '.' && self.peek() == Some('.') {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    self.emit(start, DotDotEqual);
                } else {
                    self.emit(start, DotDot);
                }
                continue;
            }
            let pair = match (c, self.peek()) {
                ('-', Some('>')) => Some(Arrow),
                ('=', Some('>')) => Some(FatArrow),
                ('=', Some('=')) => Some(EqualEqual),
                ('!', Some('=')) => Some(BangEqual),
                ('+', Some('=')) => Some(PlusEqual),
                ('-', Some('=')) => Some(MinusEqual),
                ('*', Some('=')) => Some(StarEqual),
                ('/', Some('=')) => Some(SlashEqual),
                _ => None,
            };
            if let Some(kind) = pair {
                self.advance();
                self.emit(start, kind);
                continue;
            }
            // Inside `${ ... }` braces are counted, so a struct literal in an
            // interpolation does not end it early.
            if c == '{' && !self.interpolations.is_empty() {
                *self.interpolations.last_mut().expect("open interpolation") += 1;
            }
            if c == '}' && !self.interpolations.is_empty() {
                let depth = self.interpolations.last_mut().expect("open interpolation");
                if *depth == 0 {
                    self.interpolations.pop();
                    self.resume_interpolation(start);
                    continue;
                }
                *depth -= 1;
            }
            let kind = match c {
                '(' => LeftParen,
                ')' => RightParen,
                '{' => LeftBrace,
                '}' => RightBrace,
                '[' => LeftBracket,
                ']' => RightBracket,
                ',' => Comma,
                '.' => Dot,
                ':' => Colon,
                ';' => Semicolon,
                '+' => Plus,
                '-' => Minus,
                '*' => Star,
                '/' => Slash,
                '%' => Percent,
                '=' => Equal,
                '!' => Bang,
                '?' => Question,
                _ => {
                    self.error(
                        start,
                        DiagnosticCode::InvalidCharacter,
                        format!("invalid character `{}`", c.escape_default()),
                    );
                    continue;
                }
            };
            self.emit(start, kind);
        }
        self.emit(self.source.len(), Eof);
        LexOutput {
            tokens: self.tokens,
            diagnostics: self.diagnostics,
        }
    }
    fn identifier(&mut self, start: usize) {
        use TokenKind::*;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.advance();
        }
        let text = &self.source[start..self.offset];
        let kind = match text {
            "func" => Function,
            "let" => Let,
            "var" => Var,
            "return" => Return,
            "if" => If,
            "else" => Else,
            "while" => While,
            "loop" => Loop,
            "break" => Break,
            "continue" => Continue,
            "new" => New,
            "weak" => Weak,
            "class" => Class,
            "struct" => Struct,
            "impl" => Impl,
            "interface" => Interface,
            "enum" => Enum,
            "match" => Match,
            "import" => Import,
            "const" => Const,
            "pub" => Pub,
            "for" => For,
            "in" => In,
            "static" => Static,
            "extern" => Extern,
            "unsafe" => Unsafe,
            "defer" => Defer,
            "true" => Boolean(true),
            "false" => Boolean(false),
            _ => Identifier(text.into()),
        };
        self.emit(start, kind);
    }
    fn number(&mut self, start: usize) {
        if self.source[start..].starts_with('0') {
            let next = self.source[start + 1..].chars().next();
            match next {
                Some('x' | 'X') => {
                    self.advance();
                    self.advance();
                    let digits_start = self.offset;
                    while self
                        .peek()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        self.advance();
                    }
                    let raw = &self.source[digits_start..self.offset];
                    let clean: String = raw.chars().filter(|&c| c != '_').collect();
                    if clean.is_empty() {
                        self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            "hexadecimal literal has no digits",
                        );
                        return;
                    }
                    if let Some(bad) = raw.chars().find(|&c| !c.is_ascii_hexdigit() && c != '_') {
                        self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            format!("invalid digit `{bad}` in hexadecimal literal"),
                        );
                        return;
                    }
                    match u64::from_str_radix(&clean, 16) {
                        Ok(value) => self.emit(start, TokenKind::Integer(value)),
                        Err(_) => self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            "integer literal exceeds the maximum magnitude 18446744073709551615",
                        ),
                    }
                    return;
                }
                Some('b' | 'B') => {
                    self.advance();
                    self.advance();
                    let digits_start = self.offset;
                    while self
                        .peek()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        self.advance();
                    }
                    let raw = &self.source[digits_start..self.offset];
                    let clean: String = raw.chars().filter(|&c| c != '_').collect();
                    if clean.is_empty() {
                        self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            "binary literal has no digits",
                        );
                        return;
                    }
                    if let Some(bad) = raw.chars().find(|&c| c != '0' && c != '1' && c != '_') {
                        self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            format!("invalid digit `{bad}` in binary literal"),
                        );
                        return;
                    }
                    match u64::from_str_radix(&clean, 2) {
                        Ok(value) => self.emit(start, TokenKind::Integer(value)),
                        Err(_) => self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            "integer literal exceeds the maximum magnitude 18446744073709551615",
                        ),
                    }
                    return;
                }
                Some('o' | 'O') => {
                    self.advance();
                    self.advance();
                    let digits_start = self.offset;
                    while self
                        .peek()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        self.advance();
                    }
                    let raw = &self.source[digits_start..self.offset];
                    let clean: String = raw.chars().filter(|&c| c != '_').collect();
                    if clean.is_empty() {
                        self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            "octal literal has no digits",
                        );
                        return;
                    }
                    if let Some(bad) = raw.chars().find(|&c| !('0'..='7').contains(&c) && c != '_')
                    {
                        self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            format!("invalid digit `{bad}` in octal literal"),
                        );
                        return;
                    }
                    match u64::from_str_radix(&clean, 8) {
                        Ok(value) => self.emit(start, TokenKind::Integer(value)),
                        Err(_) => self.error(
                            start,
                            DiagnosticCode::InvalidNumber,
                            "integer literal exceeds the maximum magnitude 18446744073709551615",
                        ),
                    }
                    return;
                }
                _ => {}
            }
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            self.advance();
        }
        let float = self.peek() == Some('.')
            && self.source[self.offset + 1..].starts_with(|c: char| c.is_ascii_digit());
        if float {
            self.advance();
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.advance();
            }
        }
        let text = &self.source[start..self.offset];
        let clean: String = text.chars().filter(|&c| c != '_').collect();
        if float {
            match clean.parse::<f64>() {
                Ok(value) if value.is_finite() => self.emit(start, TokenKind::Float(value)),
                _ => self.error(
                    start,
                    DiagnosticCode::InvalidNumber,
                    "float literal is outside the finite f64 range",
                ),
            }
        } else {
            match clean.parse::<u64>() {
                Ok(value) => self.emit(start, TokenKind::Integer(value)),
                Err(_) => self.error(
                    start,
                    DiagnosticCode::InvalidNumber,
                    "integer literal exceeds the maximum magnitude 18446744073709551615",
                ),
            }
        }
    }
    fn quoted(&mut self, start: usize, quote: char) {
        self.advance();
        match self.scan_text(start, quote) {
            Some(Scanned::Closed(value)) => self.finish_literal(start, quote, value),
            Some(Scanned::Interpolated(value)) => {
                self.emit(start, TokenKind::InterpolationBegin(value));
                self.interpolations.push(0);
            }
            None => {}
        }
    }
    /// Continue the string that an interpolation expression interrupted.
    fn resume_interpolation(&mut self, start: usize) {
        match self.scan_text(start, '"') {
            Some(Scanned::Closed(value)) => self.emit(start, TokenKind::InterpolationEnd(value)),
            Some(Scanned::Interpolated(value)) => {
                self.emit(start, TokenKind::InterpolationPart(value));
                self.interpolations.push(0);
            }
            None => {}
        }
    }
    /// Collect literal text up to the closing quote or the next `${`.
    /// Returns None when the literal is invalid; the diagnostic is emitted here.
    fn scan_text(&mut self, start: usize, quote: char) -> Option<Scanned> {
        let mut value = String::new();
        let mut valid = true;
        let mut closed = false;
        while let Some(c) = self.peek() {
            if c == '\n' || c == '\r' {
                break;
            }
            if c == '$' && quote == '"' && self.source[self.offset + 1..].starts_with('{') {
                self.advance();
                self.advance();
                return valid.then_some(Scanned::Interpolated(value));
            }
            self.advance();
            if c == quote {
                closed = true;
                break;
            }
            if c != '\\' {
                value.push(c);
                continue;
            }
            let escape_start = self.offset - 1;
            let Some(escaped) = self.peek() else {
                break;
            };
            if escaped == '\n' || escaped == '\r' {
                break;
            }
            self.advance();
            match escaped {
                '\\' => value.push('\\'),
                '"' => value.push('"'),
                '\'' => value.push('\''),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                '0' => value.push('\0'),
                // Without this a literal `${` could not be written.
                '$' => value.push('$'),
                _ => {
                    valid = false;
                    self.error(
                        escape_start,
                        DiagnosticCode::InvalidEscape,
                        format!("unknown escape sequence `\\{escaped}`"),
                    );
                }
            }
        }
        if !closed {
            self.error(
                start,
                DiagnosticCode::UnterminatedLiteral,
                if quote == '"' {
                    "unterminated string literal"
                } else {
                    "unterminated char literal"
                },
            );
            return None;
        }
        valid.then_some(Scanned::Closed(value))
    }
    fn finish_literal(&mut self, start: usize, quote: char, value: String) {
        if quote == '"' {
            self.emit(start, TokenKind::String(value));
        } else {
            let mut chars = value.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => self.emit(start, TokenKind::Char(c)),
                _ => self.error(
                    start,
                    DiagnosticCode::InvalidChar,
                    "char literal must contain exactly one Unicode scalar value",
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests;
