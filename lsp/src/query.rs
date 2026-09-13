//! Answering a question about one position: what is this, and where does it
//! come from.
//!
//! Both questions are the same lookup with different answers, so the finding
//! lives here once. Like completion, it reads the last check that succeeded;
//! unlike completion, it is usually asked of a file that parses, because the
//! cursor is resting rather than typing.

use crate::complete::type_name;
use skuld_compiler::{
    module::FileId,
    resolver::{Builtin, SymbolId, SymbolKind},
    span::Span,
    type_checker::TypedProgram,
    types::Type,
};

/// The entry file of a checked program is always its first file.
const ENTRY: FileId = FileId(0);

/// A word in the source, and where it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// What a position turned out to name.
#[derive(Debug)]
pub enum Target {
    /// A resolved name: a binding, a function, an import qualifier.
    Symbol(SymbolId, Word),
    /// A member reached through a `.`, which has no symbol of its own.
    Member { receiver: Type, word: Word },
    /// A declared type, which is not a value name.
    Type(Type, Word),
}

impl Target {
    pub fn word(&self) -> &Word {
        match self {
            Self::Symbol(_, word) | Self::Member { word, .. } | Self::Type(_, word) => word,
        }
    }
}

/// The identifier the offset sits in or immediately after, so that a cursor
/// resting at either end of a word still asks about that word.
pub fn word_at(source: &str, offset: usize) -> Option<Word> {
    let offset = offset.min(source.len());
    let is_part = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = offset;
    while start > 0 {
        let previous = source[..start].chars().next_back()?;
        if is_part(previous) {
            start -= previous.len_utf8();
        } else {
            break;
        }
    }
    let mut end = offset;
    while end < source.len() {
        let next = source[end..].chars().next()?;
        if is_part(next) {
            end += next.len_utf8();
        } else {
            break;
        }
    }
    if start == end {
        return None;
    }
    Some(Word {
        start,
        end,
        text: source[start..end].to_owned(),
    })
}

/// What the position names, if the last good check knows.
pub fn target_at(source: &str, offset: usize, typed: &TypedProgram) -> Option<Target> {
    let word = word_at(source, offset)?;
    let resolution = typed.resolution();
    // A use and a declaration are both answers: hovering the name in
    // `func area()` should say as much as hovering a call to it.
    if let Some(&symbol) = resolution
        .references
        .get(&(ENTRY, word.start))
        .or_else(|| resolution.declarations.get(&(ENTRY, word.start)))
    {
        return Some(Target::Symbol(symbol, word));
    }
    // A member label is not a value name, so it is found through what precedes
    // the dot instead.
    let before = source[..word.start].trim_end();
    if let Some(receiver) = before.strip_suffix('.') {
        let receiver = receiver.trim_end();
        let start = word_at(receiver, receiver.len())?.start;
        if let Some(&symbol) = resolution.references.get(&(ENTRY, start)) {
            return Some(Target::Member {
                receiver: typed.symbol_type(symbol),
                word,
            });
        }
    }
    // A type name: `Point` in an annotation resolves to no symbol at all.
    if let Some(id) = typed
        .structs()
        .iter()
        .position(|info| info.name == word.text)
    {
        return Some(Target::Type(
            Type::Struct(skuld_compiler::types::StructId(id)),
            word,
        ));
    }
    if let Some(id) = typed.enums().iter().position(|info| info.name == word.text) {
        return Some(Target::Type(
            Type::Enum(skuld_compiler::types::EnumId(id)),
            word,
        ));
    }
    None
}

/// What to show above a position: the declaration a reader would have to go
/// and find otherwise.
pub fn hover(source: &str, offset: usize, typed: &TypedProgram) -> Option<(String, Span)> {
    let target = target_at(source, offset, typed)?;
    let span = Span::new(target.word().start, target.word().end);
    Some((describe(typed, &target)?, span))
}

/// How a target reads on one line. It is the hover text, and it is also what
/// signature help falls back to for a name that was never declared in a file.
pub fn describe(typed: &TypedProgram, target: &Target) -> Option<String> {
    Some(match target {
        Target::Symbol(symbol, word) => describe_symbol(typed, *symbol, word),
        Target::Member { receiver, word } => describe_member(typed, *receiver, word)?,
        Target::Type(ty, word) => match ty {
            Type::Struct(id) => {
                let info = typed.structs().get(id.0)?;
                let keyword = if info.reference { "class" } else { "struct" };
                format!("{keyword} {}", word.text)
            }
            Type::Enum(_) => format!("enum {}", word.text),
            other => type_name(typed, *other),
        },
    })
}

/// Where a position's name is declared: the file it lives in, and the span of
/// its name there. A prelude binding has nowhere to go, which is not a failure.
pub fn definition(source: &str, offset: usize, typed: &TypedProgram) -> Option<(FileId, Span)> {
    match target_at(source, offset, typed)? {
        Target::Symbol(symbol, _) => declaration_of(typed, symbol),
        Target::Type(Type::Struct(id), _) => {
            let info = typed.structs().get(id.0)?;
            declaring_file(typed, |program| {
                program
                    .structs
                    .iter()
                    .any(|declaration| declaration.span == info.span)
            })
            .map(|file| (file, info.span))
        }
        Target::Type(Type::Enum(id), _) => {
            let name = typed.enums().get(id.0)?.name.clone();
            let mut found = None;
            for (index, file) in typed.program().files.iter().enumerate() {
                if let Some(declaration) = file
                    .program
                    .enums
                    .iter()
                    .find(|declaration| declaration.name.text == name)
                {
                    found = Some((FileId(index), declaration.name.span));
                    break;
                }
            }
            found
        }
        Target::Member {
            receiver: Type::Struct(id),
            word,
        } => {
            let info = typed.structs().get(id.0)?;
            if let Some(method) = info.methods.iter().find(|method| method.name == word.text) {
                return declaration_of(typed, method.id);
            }
            let field = info.fields.iter().find(|field| field.name == word.text)?;
            declaring_file(typed, |program| {
                program
                    .structs
                    .iter()
                    .any(|declaration| declaration.span == info.span)
            })
            .map(|file| (file, field.span))
        }
        // A builtin method belongs to the language, not to a file.
        Target::Member { .. } | Target::Type(_, _) => None,
    }
}

/// The declaration a symbol came from. The resolution records it by position,
/// so the position is what identifies the file.
fn declaration_of(typed: &TypedProgram, symbol: SymbolId) -> Option<(FileId, Span)> {
    let resolution = typed.resolution();
    let (&(file, start), _) = resolution
        .declarations
        .iter()
        .find(|(_, declared)| **declared == symbol)?;
    let span = resolution
        .symbols
        .get(symbol.0)
        .and_then(|info| info.span)
        .unwrap_or_else(|| Span::new(start, start));
    Some((file, span))
}

/// Which file's syntax satisfies a predicate. A type has no file of its own in
/// the checker's tables, so it is found where it was written.
fn declaring_file(
    typed: &TypedProgram,
    matches: impl Fn(&skuld_compiler::ast::Program) -> bool,
) -> Option<FileId> {
    typed
        .program()
        .files
        .iter()
        .position(|file| matches(&file.program))
        .map(FileId)
}

fn describe_symbol(typed: &TypedProgram, symbol: SymbolId, word: &Word) -> String {
    let resolution = typed.resolution();
    let Some(info) = resolution.symbols.get(symbol.0) else {
        return word.text.clone();
    };
    match info.kind {
        SymbolKind::Function => match typed.signature(symbol) {
            Some(signature) => format!(
                "func {}({}) -> {}",
                word.text,
                signature
                    .parameters
                    .iter()
                    .map(|parameter| type_name(typed, *parameter))
                    .collect::<Vec<_>>()
                    .join(", "),
                type_name(typed, signature.return_type)
            ),
            None => format!("func {}", word.text),
        },
        SymbolKind::Parameter => format!(
            "{}: {}",
            word.text,
            type_name(typed, typed.symbol_type(symbol))
        ),
        SymbolKind::Variable(mutability) => format!(
            "{} {}: {}",
            match mutability {
                skuld_compiler::ast::Mutability::Mutable => "var",
                skuld_compiler::ast::Mutability::Immutable => "let",
            },
            word.text,
            type_name(typed, typed.symbol_type(symbol))
        ),
        SymbolKind::Enum => format!("enum {}", word.text),
        SymbolKind::Module(module) => match typed.program().modules.get(module.0) {
            Some(info) => format!("import \"{}\"", info.path),
            None => format!("module {}", word.text),
        },
        SymbolKind::Builtin(builtin) => describe_builtin(builtin, &word.text),
    }
}

/// The prelude has no declaration to point at, so its description is written
/// here — the one place in the server that restates the language.
fn describe_builtin(builtin: Builtin, name: &str) -> String {
    match builtin {
        Builtin::Print => "func print(value) -> void".to_owned(),
        Builtin::Some => "Some(value) -> Option<T>".to_owned(),
        Builtin::None => "None -> Option<T>".to_owned(),
        Builtin::Ok => "Ok(value) -> Result<T, E>".to_owned(),
        Builtin::Err => "Err(error) -> Result<T, E>".to_owned(),
        Builtin::Ptr => "func ptr(value) -> *u8".to_owned(),
        Builtin::BytesToString => "func bytes_to_string([]u8) -> Result<string, string>".to_owned(),
        Builtin::IntConvert(kind) => format!("func {name}(value) -> {}", kind.name()),
    }
}

fn describe_member(typed: &TypedProgram, receiver: Type, word: &Word) -> Option<String> {
    if let Type::Struct(id) = receiver {
        let info = typed.structs().get(id.0)?;
        if let Some(field) = info.fields.iter().find(|field| field.name == word.text) {
            return Some(format!("{}: {}", field.name, type_name(typed, field.ty)));
        }
        if let Some(method) = info.methods.iter().find(|method| method.name == word.text) {
            let signature = typed.signature(method.id)?;
            return Some(format!(
                "{}({}) -> {}",
                method.name,
                signature
                    .parameters
                    .iter()
                    .map(|parameter| type_name(typed, *parameter))
                    .collect::<Vec<_>>()
                    .join(", "),
                type_name(typed, signature.return_type)
            ));
        }
        return None;
    }
    // A builtin method's shape is already written once, for completion.
    crate::complete::members_of(typed, receiver)
        .into_iter()
        .find(|item| item.label == word.text)
        .map(|item| match item.detail {
            Some(detail) => format!("{}{detail}", item.label),
            None => item.label,
        })
}

#[cfg(test)]
mod tests;
