//! Call hierarchy: who calls this function, and what does it call.
//!
//! Both answers are the resolver's use table read with one extra question
//! asked of the syntax — which declaration a use sits inside. The table knows
//! that a name is used at an offset; the syntax knows which function's span
//! holds that offset, and that pairing is the whole of a call graph.

use skuld_compiler::{
    ast::FunctionDecl,
    module::FileId,
    resolver::{SymbolId, SymbolKind},
    span::Span,
    type_checker::TypedProgram,
};

/// LSP `SymbolKind`: a free function and a method are told apart, because a
/// client shows the icon and a reader is looking for one or the other.
pub const FUNCTION: f64 = 12.0;
pub const METHOD: f64 = 6.0;

/// One end of a call: a function, where it is declared, and how it reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub file: FileId,
    pub name: String,
    /// The whole declaration, which is what a client reveals.
    pub range: Span,
    /// Its name alone, which is what a client highlights.
    pub selection: Span,
    pub kind: f64,
    /// The class a method belongs to, when it is one.
    pub detail: Option<String>,
}

/// The function declared at an offset, with the symbol the resolver gave it.
pub fn declared_at(typed: &TypedProgram, file: FileId, offset: usize) -> Option<(SymbolId, Item)> {
    let item = declarations(typed, file)
        .into_iter()
        .find(|(_, item)| item.selection.start == offset)
        .map(|(_, item)| item)?;
    let symbol = *typed
        .resolution()
        .declarations
        .get(&(file, item.selection.start))?;
    Some((symbol, item))
}

/// The function whose declaration holds an offset: the caller, when the offset
/// is a call. A use outside every function body — in a field default, say —
/// has no caller, which is not a failure.
pub fn enclosing(typed: &TypedProgram, file: FileId, offset: usize) -> Option<Item> {
    declarations(typed, file)
        .into_iter()
        .map(|(_, item)| item)
        // The innermost, since a method's span sits inside nothing else but a
        // future nested declaration would.
        .filter(|item| item.range.start <= offset && offset < item.range.end)
        .min_by_key(|item| item.range.end - item.range.start)
}

/// Every place a symbol is used, not counting where it was declared.
pub fn references_to(typed: &TypedProgram, symbol: SymbolId) -> Vec<(FileId, Span)> {
    let resolution = typed.resolution();
    let Some(info) = resolution.symbols.get(symbol.0) else {
        return Vec::new();
    };
    let length = info.name.len();
    let mut found: Vec<(FileId, Span)> = resolution
        .references
        .iter()
        .filter(|&(_, &named)| named == symbol)
        .map(|(&(file, start), _)| (file, Span::new(start, start + length)))
        .collect();
    found.sort_unstable_by_key(|(file, span)| (file.0, span.start));
    found.dedup();
    found
}

/// The functions named inside a span, with where each is named.
///
/// A function named where a value is expected is reported too: Skuld turns a
/// declared function into a function value at such a place, so the body does
/// reach it, and leaving it out would hide half of what a callback does.
pub fn calls_within(typed: &TypedProgram, file: FileId, body: Span) -> Vec<(SymbolId, Span)> {
    let resolution = typed.resolution();
    let mut found: Vec<(SymbolId, Span)> = resolution
        .references
        .iter()
        .filter(|&(&(used_in, start), _)| {
            used_in == file && body.start <= start && start < body.end
        })
        .filter_map(|(&(_, start), &symbol)| {
            let info = resolution.symbols.get(symbol.0)?;
            // A builtin has no declaration to hand back as the other end of a
            // call, so the prelude is left out of the graph.
            matches!(info.kind, SymbolKind::Function)
                .then(|| (symbol, Span::new(start, start + info.name.len())))
        })
        .collect();
    found.sort_unstable_by_key(|(_, span)| span.start);
    found.dedup();
    found
}

/// The item for the declaration of a symbol, wherever it was written.
pub fn item_of(typed: &TypedProgram, symbol: SymbolId) -> Option<Item> {
    let (&(file, offset), _) = typed
        .resolution()
        .declarations
        .iter()
        .find(|(_, declared)| **declared == symbol)?;
    declared_at(typed, file, offset).map(|(_, item)| item)
}

/// Every function and method declared in one file, paired with its name.
fn declarations(typed: &TypedProgram, file: FileId) -> Vec<(&FunctionDecl, Item)> {
    let Some(loaded) = typed.program().files.get(file.0) else {
        return Vec::new();
    };
    let mut declared: Vec<(&FunctionDecl, Item)> = loaded
        .program
        .functions
        .iter()
        .map(|function| (function, item(file, function, FUNCTION, None)))
        .collect();
    for owner in &loaded.program.structs {
        declared.extend(owner.methods.iter().map(|method| {
            (
                method,
                item(file, method, METHOD, Some(owner.name.text.clone())),
            )
        }));
    }
    declared
}

fn item(file: FileId, function: &FunctionDecl, kind: f64, detail: Option<String>) -> Item {
    Item {
        file,
        name: function.name.text.clone(),
        range: function.span,
        selection: function.name.span,
        kind,
        detail,
    }
}

#[cfg(test)]
mod tests;
