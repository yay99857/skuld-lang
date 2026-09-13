//! Finding every use of a name, and changing all of them at once.
//!
//! Both questions read the resolver's tables the other way round from
//! `query`: there a position asks for its symbol, here a symbol asks for its
//! positions. The tables already record a use by file and byte offset, so a
//! reference list is a filter over them rather than a new pass.
//!
//! A rename is the same list with an edit attached, plus the part that makes
//! it safe: nothing here decides that an edit is correct because the names
//! look right. The server rechecks the edited program and compares where
//! every name resolves, and this module supplies the comparison.

use crate::query::{self, Target, Word};
use skuld_compiler::{
    module::FileId,
    resolver::{SymbolId, SymbolKind},
    span::Span,
    type_checker::TypedProgram,
};
use std::collections::BTreeMap;

/// A name as it is written somewhere in a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub file: FileId,
    pub span: Span,
}

/// Why a position cannot be renamed. A refusal is an answer, not a failure:
/// the client shows it to the user instead of applying a half-correct edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The cursor is not resting on a name the last good check knows.
    NotAName,
    /// A prelude binding belongs to the language, and no edit to one file can
    /// move it.
    Prelude,
    /// An import qualifier is the last segment of a directory path, so
    /// renaming it would mean renaming a directory.
    Qualifier,
    /// A struct, class or enum name. Type names live in the checker's own
    /// namespace and their uses in annotations are not in the resolver's use
    /// table, so a rename here would leave the annotations behind.
    Type,
    /// A field or a method, which is reached through its receiver's type
    /// rather than through a name the resolver tracked.
    Member,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Self::NotAName => "there is no name to rename at this position",
            Self::Prelude => "a prelude binding belongs to the language and cannot be renamed",
            Self::Qualifier => {
                "an import qualifier is the module's directory name; rename the directory instead"
            }
            Self::Type => "renaming a type is not supported yet",
            Self::Member => "renaming a field or a method is not supported yet",
        }
    }
}

/// The symbol a position names, when it is one this milestone can move.
pub fn nameable(
    source: &str,
    offset: usize,
    typed: &TypedProgram,
) -> Result<(SymbolId, Word), Refusal> {
    match query::target_at(source, offset, typed) {
        Some(Target::Symbol(symbol, word)) => {
            let kind = typed
                .resolution()
                .symbols
                .get(symbol.0)
                .map(|info| info.kind)
                .ok_or(Refusal::NotAName)?;
            match kind {
                SymbolKind::Function | SymbolKind::Parameter | SymbolKind::Variable(_) => {
                    Ok((symbol, word))
                }
                SymbolKind::Builtin(_) => Err(Refusal::Prelude),
                SymbolKind::Module(_) => Err(Refusal::Qualifier),
                SymbolKind::Enum => Err(Refusal::Type),
            }
        }
        Some(Target::Member { .. }) => Err(Refusal::Member),
        Some(Target::Type(_, _)) => Err(Refusal::Type),
        None => Err(Refusal::NotAName),
    }
}

/// Every place one symbol is written in this program: its declaration and all
/// of its uses, in file and offset order.
pub fn occurrences(typed: &TypedProgram, symbol: SymbolId) -> Vec<Occurrence> {
    let resolution = typed.resolution();
    let Some(info) = resolution.symbols.get(symbol.0) else {
        return Vec::new();
    };
    // A name is written exactly as it was declared, so its length is the
    // declared name's; the tables record only where each one starts.
    let length = info.name.len();
    let mut found: Vec<Occurrence> = resolution
        .declarations
        .iter()
        .chain(resolution.references.iter())
        .filter(|&(_, &named)| named == symbol)
        .map(|(&(file, start), _)| Occurrence {
            file,
            span: Span::new(start, start + length),
        })
        .collect();
    found.sort_unstable_by_key(|occurrence| (occurrence.file.0, occurrence.span.start));
    found.dedup();
    found
}

/// Where a symbol is declared, as the key the tables record it under. It is
/// what identifies the same declaration in another program that includes the
/// same file.
pub fn declaration_key(typed: &TypedProgram, symbol: SymbolId) -> Option<(FileId, usize)> {
    typed
        .resolution()
        .declarations
        .iter()
        .find(|&(_, &declared)| declared == symbol)
        .map(|(&key, _)| key)
}

/// The symbol declared at a position, which is how one program recognises a
/// declaration another program found.
pub fn symbol_declared_at(typed: &TypedProgram, file: FileId, offset: usize) -> Option<SymbolId> {
    typed
        .resolution()
        .declarations
        .get(&(file, offset))
        .copied()
}

/// Whether a new name is a name at all. The lexer decides, rather than a list
/// of keywords copied out of it: a spelling that lexes as one identifier is
/// one, and everything else — a keyword, a number, two words, a symbol — is
/// not.
pub fn is_identifier(name: &str) -> bool {
    let output = skuld_compiler::lex(name);
    output.diagnostics.is_empty()
        && output.tokens.len() == 2
        && matches!(
            output.tokens.first().map(|token| &token.kind),
            Some(skuld_compiler::token::TokenKind::Identifier(text)) if text == name
        )
}

/// Which declaration a name reached.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Site {
    /// Declared in a file, at a byte offset.
    Declared(String, usize),
    /// A prelude binding, which has no declaration in any file and so is
    /// identified by what it is rather than by where it was written.
    Prelude(String),
}

impl Site {
    /// The same site in the coordinates an edit produced.
    fn shifted(&self, shift: &impl Fn(&str, usize) -> usize) -> Self {
        match self {
            Self::Declared(path, offset) => Self::Declared(path.clone(), shift(path, *offset)),
            Self::Prelude(name) => Self::Prelude(name.clone()),
        }
    }
}

/// Where every name in a program resolves, written in terms a second check of
/// the same program can be compared against.
///
/// The key is a file path and a byte offset; the value says which declaration
/// that name reached. Comparing two of these across an edit is what proves a
/// rename changed spelling and nothing else — a new name that captured a use
/// from an outer scope, or lost one to an inner scope, shows up here as a
/// value that moved.
pub fn shape(
    typed: &TypedProgram,
    path_of: &impl Fn(FileId) -> Option<String>,
) -> BTreeMap<(String, usize), Site> {
    let resolution = typed.resolution();
    let mut sites: BTreeMap<SymbolId, Site> = BTreeMap::new();
    for (&(file, offset), &symbol) in &resolution.declarations {
        if let Some(path) = path_of(file) {
            sites.entry(symbol).or_insert(Site::Declared(path, offset));
        }
    }
    let mut shape = BTreeMap::new();
    for (&(file, offset), &symbol) in resolution.references.iter().chain(&resolution.declarations) {
        let Some(path) = path_of(file) else {
            continue;
        };
        let site = match sites.get(&symbol) {
            Some(site) => site.clone(),
            None => Site::Prelude(
                resolution
                    .symbols
                    .get(symbol.0)
                    .map_or_else(|| "?".to_string(), |info| info.name.clone()),
            ),
        };
        shape.insert((path, offset), site);
    }
    shape
}

/// The same shape in the coordinates the edited program uses: every offset
/// after an edit in its file moves by the difference in name lengths.
pub fn shifted(
    shape: &BTreeMap<(String, usize), Site>,
    shift: &impl Fn(&str, usize) -> usize,
) -> BTreeMap<(String, usize), Site> {
    shape
        .iter()
        .map(|((path, offset), site)| ((path.clone(), shift(path, *offset)), site.shifted(shift)))
        .collect()
}

#[cfg(test)]
mod tests;
