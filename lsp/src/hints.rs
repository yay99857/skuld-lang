//! Inlay hints: the type an inference left unwritten.
//!
//! Skuld types every function signature explicitly and infers every local, so
//! the one type a reader cannot see is a binding's. `let socket = net.connect(...)`
//! says nothing about what `socket` is, and going to find out means a hover,
//! or a jump into another module. A hint puts the checker's answer where the
//! annotation would have been.
//!
//! Only a binding with no written annotation gets one: repeating `: int` next
//! to a `let count: int = 0` would be noise, not information.

use crate::complete::type_name;
use skuld_compiler::{
    module::FileId, resolver::SymbolKind, type_checker::TypedProgram, types::Type,
};

/// The entry file of a checked program, which is the document being edited.
const ENTRY: FileId = FileId(0);

/// One hint, and the byte offset in the document it is drawn at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// Immediately after the name, which is where the annotation would go.
    pub offset: usize,
    pub label: String,
}

/// The inferred type of every unannotated binding in the document, in source
/// order.
pub fn type_hints(source: &str, typed: &TypedProgram) -> Vec<Hint> {
    let resolution = typed.resolution();
    let mut hints = Vec::new();
    for (&(file, start), &symbol) in &resolution.declarations {
        if file != ENTRY {
            continue;
        }
        let Some(info) = resolution.symbols.get(symbol.0) else {
            continue;
        };
        // A parameter and a function are written with their types already;
        // only a binding is inferred.
        if !matches!(info.kind, SymbolKind::Variable(_)) {
            continue;
        }
        let end = start + info.name.len();
        if source.get(start..end) != Some(info.name.as_str()) || is_annotated(source, end) {
            continue;
        }
        let ty = typed.symbol_type(symbol);
        // `Type::Error` is recovery, and a successful check has none; a
        // binding whose type is void is a checker error too. Neither is worth
        // drawing.
        if matches!(ty, Type::Error | Type::Void) {
            continue;
        }
        hints.push(Hint {
            offset: end,
            label: format!(": {}", type_name(typed, ty)),
        });
    }
    hints.sort_unstable_by_key(|hint| hint.offset);
    hints
}

/// Whether a written type follows the name at `end`.
///
/// This reads the text rather than the syntax tree, and it is sound because of
/// where a binding name can appear: a `:` may follow one only in
/// `let name: Type = ...`. Every other binder closes with something else —
/// `if let Some(name)` and a match arm's payload with `)`, `for name in` with
/// a keyword, `let name = value else reason { ... }` with a block — and a
/// match arm's own `:` comes after the pattern's closing parenthesis, never
/// after the name inside it.
fn is_annotated(source: &str, end: usize) -> bool {
    source[end..]
        .trim_start_matches([' ', '\t'])
        .starts_with(':')
}

#[cfg(test)]
mod tests;
