//! Selection ranges: what "expand the selection" should reach next.
//!
//! The chain is built from the same place folding's is — the token stream —
//! for the same reason: expanding a selection is something a reader does while
//! editing, which is while the text does not parse. Each step contains the one
//! before it, which is what the protocol requires of the chain.

use skuld_compiler::{span::Span, token::TokenKind};

/// The ranges to expand through at a position, innermost first. The last is
/// always the whole document, so expanding never runs out before it should.
pub fn chain(source: &str, offset: usize) -> Vec<Span> {
    let offset = offset.min(source.len());
    let mut steps = Vec::new();

    // The word under the cursor, which is what a first expansion selects.
    if let Some(word) = crate::query::word_at(source, offset) {
        steps.push(Span::new(word.start, word.end));
    }

    let lexed = skuld_compiler::lex(source);
    // A string is two steps: what it holds, then the literal with its quotes.
    for token in &lexed.tokens {
        let holds = token.span.start < offset && offset < token.span.end;
        // An interpolated string arrives in pieces, each carrying its own
        // quote or brace, so only a plain literal is stepped through here.
        if holds && matches!(token.kind, TokenKind::String(_) | TokenKind::Char(_)) {
            steps.push(Span::new(token.span.start + 1, token.span.end - 1));
            steps.push(token.span);
        }
    }

    // Every bracket pair the offset sits inside, innermost first: first what
    // the pair holds, then the pair with its brackets.
    let mut open: Vec<usize> = Vec::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for token in &lexed.tokens {
        match token.kind {
            TokenKind::LeftBrace | TokenKind::LeftBracket | TokenKind::LeftParen => {
                open.push(token.span.start)
            }
            TokenKind::RightBrace | TokenKind::RightBracket | TokenKind::RightParen => {
                if let Some(start) = open.pop() {
                    pairs.push((start, token.span.end));
                }
            }
            _ => {}
        }
    }
    pairs.retain(|&(start, end)| start < offset && offset < end);
    pairs.sort_by_key(|&(start, end)| end - start);
    for (start, end) in pairs {
        steps.push(Span::new(start + 1, end - 1));
        steps.push(Span::new(start, end));
    }

    steps.push(Span::new(0, source.len()));

    // Keep only the steps that actually grow: a word that fills its
    // parentheses would otherwise be offered twice, and a client walking the
    // chain would appear to stall.
    let mut chain: Vec<Span> = Vec::new();
    for step in steps {
        if step.start > offset || step.end < offset {
            continue;
        }
        match chain.last() {
            Some(last) if step.start >= last.start && step.end <= last.end => {}
            _ => chain.push(step),
        }
    }
    chain
}

#[cfg(test)]
mod tests;
