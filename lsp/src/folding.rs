//! Folding ranges: which runs of lines an editor may collapse.
//!
//! These come from the token stream, not from the syntax tree, and that is a
//! choice rather than a shortcut. Folding is a textual idea — a reader folds a
//! brace, not a `StatementKind` — and the lexer answers for text the parser
//! rejects, which is the text an editor usually holds. It also means a lambda
//! body folds like any other block, with nothing written here about lambdas.

use crate::text::Positions;
use skuld_compiler::token::TokenKind;

/// A range of lines that may be collapsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fold {
    pub start_line: usize,
    pub end_line: usize,
    /// `imports` and `comment` are the two kinds the protocol names that Skuld
    /// has; a block is a plain region and carries none.
    pub kind: Option<&'static str>,
}

/// Every foldable run in the document, in source order.
pub fn folds(source: &str, positions: &Positions) -> Vec<Fold> {
    let mut folds = brace_folds(source, positions);
    folds.extend(import_folds(source, positions));
    folds.extend(comment_folds(source));
    folds.sort_by_key(|fold| (fold.start_line, fold.end_line));
    folds
}

/// A fold per balanced pair of braces or brackets that spans more than a line.
fn brace_folds(source: &str, positions: &Positions) -> Vec<Fold> {
    let mut open: Vec<usize> = Vec::new();
    let mut folds = Vec::new();
    for token in skuld_compiler::lex(source).tokens {
        match token.kind {
            TokenKind::LeftBrace | TokenKind::LeftBracket => open.push(token.span.start),
            TokenKind::RightBrace | TokenKind::RightBracket => {
                // A close with nothing open is unbalanced text, which the
                // editor holds all the time; there is simply nothing to fold.
                // A `[` closed by a `}` is treated as a pair for the same
                // reason: this is a fold, not a parse.
                if let Some(start) = open.pop() {
                    let start_line = positions.position(start).line;
                    let end_line = positions.position(token.span.start).line;
                    if end_line > start_line {
                        folds.push(Fold {
                            start_line,
                            end_line,
                            kind: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    folds
}

/// One fold over a run of `import` lines. They precede every declaration and a
/// program that uses several modules opens with a block nobody reads twice.
fn import_folds(source: &str, positions: &Positions) -> Vec<Fold> {
    let lines: Vec<usize> = skuld_compiler::lex(source)
        .tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Import)
        .map(|token| positions.position(token.span.start).line)
        .collect();
    runs(&lines, Some("imports"))
}

/// One fold per run of whole-line comments. A `//` that follows code on its
/// line is not one: folding it would hide the code with it.
fn comment_folds(source: &str) -> Vec<Fold> {
    let lines: Vec<usize> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("//"))
        .map(|(index, _)| index)
        .collect();
    runs(&lines, Some("comment"))
}

/// Group consecutive lines into folds, keeping only runs worth folding.
fn runs(lines: &[usize], kind: Option<&'static str>) -> Vec<Fold> {
    let mut folds = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let start = lines[index];
        let mut end = start;
        while index + 1 < lines.len() && lines[index + 1] == end + 1 {
            index += 1;
            end = lines[index];
        }
        // A single line collapses to itself, which is not a fold.
        if end > start {
            folds.push(Fold {
                start_line: start,
                end_line: end,
                kind,
            });
        }
        index += 1;
    }
    folds
}

#[cfg(test)]
mod tests;
