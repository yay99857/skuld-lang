//! Semantic tokens: highlighting that knows what a name is.
//!
//! `editors/nvim/syntax/skuld.vim` mirrors the lexer, so it colours keywords,
//! literals and comments correctly and can do no better than convention on a
//! name: a capitalised word reads as a type because types are usually
//! capitalised. The checker knows instead of guessing, and this is that
//! knowledge as the protocol's token stream.
//!
//! Only names are emitted. Keywords, strings, numbers and comments stay with
//! the editor's own highlighting, which a client layers underneath these — a
//! server that restated them would win nothing and drift from the lexer.

use crate::query::{self, Target};
use skuld_compiler::{
    module::FileId, resolver::SymbolKind, token::TokenKind, type_checker::TypedProgram, types::Type,
};

/// The entry file of a checked program, which is the document being edited.
const ENTRY: FileId = FileId(0);

/// The token types this server emits, in the order a client is told about
/// them: the numbers on the wire are indexes into this list.
pub const TYPES: [&str; 10] = [
    "namespace",
    "class",
    "struct",
    "enum",
    "interface",
    "function",
    "method",
    "property",
    "parameter",
    "variable",
];

/// The modifiers, as a bit per entry in this order.
pub const MODIFIERS: [&str; 3] = ["declaration", "readonly", "defaultLibrary"];

const NAMESPACE: u32 = 0;
const CLASS: u32 = 1;
const STRUCT: u32 = 2;
const ENUM: u32 = 3;
const INTERFACE: u32 = 4;
const FUNCTION: u32 = 5;
const METHOD: u32 = 6;
const PROPERTY: u32 = 7;
const PARAMETER: u32 = 8;
const VARIABLE: u32 = 9;

const DECLARATION: u32 = 1 << 0;
const READONLY: u32 = 1 << 1;
const DEFAULT_LIBRARY: u32 = 1 << 2;

/// One classified name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: u32,
    pub modifiers: u32,
}

/// Every name in the document the last good check can classify, in source
/// order. A name it cannot is left out rather than guessed at: the editor's
/// own highlighting is underneath, and a wrong colour is worse than the
/// ordinary one.
pub fn tokens(source: &str, typed: &TypedProgram) -> Vec<Token> {
    let resolution = typed.resolution();
    let interfaces: Vec<&str> = typed
        .program()
        .files
        .get(ENTRY.0)
        .map(|file| {
            file.program
                .interfaces
                .iter()
                .map(|declaration| declaration.name.text.as_str())
                .collect()
        })
        .unwrap_or_default();

    let mut tokens = Vec::new();
    for token in skuld_compiler::lex(source).tokens {
        let TokenKind::Identifier(text) = &token.kind else {
            continue;
        };
        let start = token.span.start;
        // A resolved name is the common case and the certain one.
        if let Some(&symbol) = resolution
            .declarations
            .get(&(ENTRY, start))
            .or_else(|| resolution.references.get(&(ENTRY, start)))
        {
            let Some(info) = resolution.symbols.get(symbol.0) else {
                continue;
            };
            let declaring = resolution.declarations.contains_key(&(ENTRY, start));
            let (kind, mut modifiers) = match info.kind {
                SymbolKind::Function => (FUNCTION, 0),
                // A parameter and a `let` cannot be assigned again; a `var`
                // can, and that is the distinction worth seeing.
                SymbolKind::Parameter => (PARAMETER, READONLY),
                SymbolKind::Variable(skuld_compiler::ast::Mutability::Immutable) => {
                    (VARIABLE, READONLY)
                }
                SymbolKind::Variable(skuld_compiler::ast::Mutability::Mutable) => (VARIABLE, 0),
                SymbolKind::Module(_) => (NAMESPACE, 0),
                SymbolKind::Enum => (ENUM, 0),
                SymbolKind::Constant => (VARIABLE, READONLY),
                SymbolKind::Builtin(_) => (FUNCTION, DEFAULT_LIBRARY),
            };
            if declaring {
                modifiers |= DECLARATION;
            }
            tokens.push(Token {
                start,
                end: token.span.end,
                kind,
                modifiers,
            });
            continue;
        }
        // A type name resolves to no symbol: it lives in the checker's own
        // namespace, which is why it is looked up by what was written.
        if let Some(info) = typed.structs().iter().find(|info| &info.name == text) {
            tokens.push(Token {
                start,
                end: token.span.end,
                kind: if info.reference { CLASS } else { STRUCT },
                modifiers: 0,
            });
            continue;
        }
        if typed.enums().iter().any(|info| &info.name == text) {
            tokens.push(Token {
                start,
                end: token.span.end,
                kind: ENUM,
                modifiers: 0,
            });
            continue;
        }
        if interfaces.contains(&text.as_str()) {
            tokens.push(Token {
                start,
                end: token.span.end,
                kind: INTERFACE,
                modifiers: 0,
            });
            continue;
        }
        // A field or a method is reached through its receiver's type, which is
        // the one classification that needs the text around the name.
        if let Some(kind) = member_kind(source, start, typed) {
            tokens.push(Token {
                start,
                end: token.span.end,
                kind,
                modifiers: 0,
            });
        }
    }
    tokens.sort_unstable_by_key(|token| token.start);
    tokens
}

/// Whether a name reached through a `.` is a field or a method.
fn member_kind(source: &str, start: usize, typed: &TypedProgram) -> Option<u32> {
    let Some(Target::Member { receiver, word }) = query::target_at(source, start, typed) else {
        return None;
    };
    let Type::Struct(id) = receiver else {
        // A builtin member — `len`, `push`, `upgrade` — is always a method.
        return Some(METHOD);
    };
    let info = typed.structs().get(id.0)?;
    if info.fields.iter().any(|field| field.name == word.text) {
        return Some(PROPERTY);
    }
    info.methods
        .iter()
        .any(|method| method.name == word.text)
        .then_some(METHOD)
}

#[cfg(test)]
mod tests;
