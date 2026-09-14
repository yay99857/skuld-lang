use crate::span::{SourceFile, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    InvalidCharacter,
    InvalidNumber,
    UnterminatedLiteral,
    InvalidEscape,
    InvalidChar,
    ExpectedSyntax,
    ExpectedDeclaration,
    UnsupportedSyntax,
    InvalidAssignmentTarget,
    SyntaxLimit,
    UnknownName,
    DuplicateDeclaration,
    UnknownType,
    TypeMismatch,
    InvalidValueType,
    InvalidOperator,
    IntegerRange,
    ArgumentCount,
    NotCallable,
    MissingReturn,
    InvalidEntrypoint,
    UnsupportedFeature,
    ImmutableAssignment,
    InvalidAssignment,
    JumpOutsideLoop,
    MissingField,
    NonExhaustiveMatch,
    MisplacedImport,
    InvalidModulePath,
    UnknownModule,
    ImportCycle,
    PrivateName,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCharacter => "E0001",
            Self::InvalidNumber => "E0002",
            Self::UnterminatedLiteral => "E0003",
            Self::InvalidEscape => "E0004",
            Self::InvalidChar => "E0005",
            Self::ExpectedSyntax => "E1001",
            Self::ExpectedDeclaration => "E1002",
            Self::UnsupportedSyntax => "E1003",
            Self::InvalidAssignmentTarget => "E1004",
            Self::SyntaxLimit => "E1005",
            Self::UnknownName => "E0201",
            Self::DuplicateDeclaration => "E0202",
            Self::UnknownType => "E0101",
            Self::TypeMismatch => "E0102",
            Self::InvalidValueType => "E0103",
            Self::InvalidOperator => "E0104",
            Self::IntegerRange => "E0105",
            Self::ArgumentCount => "E0106",
            Self::NotCallable => "E0107",
            Self::MissingReturn => "E0108",
            Self::InvalidEntrypoint => "E0109",
            Self::UnsupportedFeature => "E0110",
            Self::ImmutableAssignment => "E0203",
            Self::InvalidAssignment => "E0204",
            Self::JumpOutsideLoop => "E0111",
            Self::MissingField => "E0112",
            Self::NonExhaustiveMatch => "E0113",
            Self::MisplacedImport => "E1006",
            Self::InvalidModulePath => "E1007",
            Self::UnknownModule => "E0205",
            Self::ImportCycle => "E0206",
            Self::PrivateName => "E0207",
        }
    }
}

/// An edit that makes a diagnostic go away, written by the code that reported
/// it.
///
/// The `help` line next to it is prose for a person; this is for a tool. A fix
/// only exists where the compiler knows the whole edit — the exact bytes to
/// replace and what to put there — so applying it is never a guess. Anything
/// the compiler can only describe stays prose.
///
/// The span is half-open and lives in the same file as the diagnostic's own,
/// so an empty span is an insertion at that point and an empty replacement is
/// a deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// What applying it does, as an editor would offer it: "add `unsafe`".
    pub title: String,
    pub span: Span,
    pub replacement: String,
}

impl Fix {
    pub fn new(title: impl Into<String>, span: Span, replacement: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            span,
            replacement: replacement.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
    /// An edit a tool may apply without asking anything else, when there is
    /// one. Most diagnostics have none, and it is boxed because a `Diagnostic`
    /// travels in a `Result` through the whole parser: the rare case does not
    /// get to widen every return.
    pub fix: Option<Box<Fix>>,
}

impl Diagnostic {
    /// Attach guidance to a diagnostic built by a helper that has none.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
    /// Attach the edit that resolves it.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(Box::new(fix));
        self
    }
    pub fn render(&self, source: &SourceFile) -> String {
        let (line, column) = source.location(self.span.start);
        let text = source.line(line).unwrap_or("");
        // Expand each tab to four spaces in both source and marker alignment.
        let prefix: String = text.chars().take(column - 1).collect();
        let indent = prefix.replace('\t', "    ").chars().count();
        let (end_line, end_column) = source.location(self.span.end.max(self.span.start));
        let width = if end_line == line {
            end_column.saturating_sub(column)
        } else {
            text.chars().count().saturating_sub(column - 1)
        };
        let marked: String = text.chars().skip(column - 1).take(width).collect();
        let width = marked.replace('\t', "    ").chars().count().max(1);
        let gutter = line.to_string().len();
        let mut output = format!(
            "error[{}]: {}\n\n  --> {}:{line}:{column}\n{:gutter$} |\n{line} | {}\n{:gutter$} | {}{}\n",
            self.code.as_str(),
            self.message,
            source.name,
            "",
            text.replace('\t', "    "),
            "",
            " ".repeat(indent),
            "^".repeat(width)
        );
        if let Some(help) = &self.help {
            output.push_str(&format!("help: {help}\n"));
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_location_and_marker() {
        let source = SourceFile::new("main.skuld", "func main() {\n    @\n}");
        let diagnostic = Diagnostic {
            code: DiagnosticCode::InvalidCharacter,
            message: "invalid character `@`".into(),
            span: Span::new(18, 19),
            help: Some("remove it".into()),
            fix: None,
        };
        assert_eq!(
            diagnostic.render(&source),
            "error[E0001]: invalid character `@`\n\n  --> main.skuld:2:5\n  |\n2 |     @\n  |     ^\nhelp: remove it\n"
        );
    }
}
